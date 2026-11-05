//! Band-limited, incremental resampling of microphone audio to Whisper's
//! 16 kHz (#89).
//!
//! The live preview resamples each newly captured chunk exactly once and
//! appends it ([`StreamResampler::push`]), instead of resampling the whole take
//! on every streaming pass. The final pass uses the same filter in one shot
//! (`dictation::resample_to_16khz`), so the preview and the inserted text hear
//! identical audio.
//!
//! The filter is a polyphase Kaiser-windowed sinc: a low-pass below the lower
//! of the two Nyquist frequencies, which removes the aliasing the former linear
//! interpolation produced at 48 kHz → 16 kHz. An output sample is emitted as
//! soon as its whole filter window is available, so chunked pushes produce
//! bit-identical output to a single push of the same samples.

/// Whisper's input rate.
const TARGET_RATE: u32 = 16_000;
/// Kaiser window shape: ~80 dB stop-band attenuation.
const KAISER_BETA: f64 = 8.0;
/// Filter half-length in samples of the lower of the two rates.
const ZERO_CROSSINGS: usize = 16;
/// Cutoff as a fraction of the lower Nyquist frequency; leaves room for the
/// transition band below it.
const CUTOFF_FRACTION: f64 = 0.92;
/// Upper bound for the number of polyphase filters. Exotic rates whose reduced
/// ratio needs more phases are approximated by the nearest of these.
const MAX_PHASES: u64 = 4_096;

pub(crate) struct StreamResampler {
    mode: Mode,
}

enum Mode {
    /// Rate 0: no meaningful audio, every push yields nothing.
    Empty,
    /// The source already runs at 16 kHz.
    Passthrough,
    Filter(Box<Polyphase>),
}

struct Polyphase {
    source_rate: u32,
    /// Output sample `n` sits at source position `n * step_num / step_den`.
    step_num: u64,
    step_den: u64,
    /// Number of filter phases; `step_den` unless quantized to [`MAX_PHASES`].
    phases: u64,
    quantized: bool,
    /// Taps per side; each phase has `2 * half` taps.
    half: usize,
    /// `phases × 2·half` coefficients, each phase normalized to sum 1.
    taps: Vec<f32>,
    /// Source samples still needed by upcoming outputs; `history[0]` is the
    /// absolute source index `history_base`.
    history: Vec<f32>,
    history_base: u64,
    /// Total source samples pushed so far.
    input_len: u64,
    /// Index of the next output sample to emit.
    next_out: u64,
}

impl StreamResampler {
    pub(crate) fn new(source_rate: u32) -> Self {
        let mode = match source_rate {
            0 => Mode::Empty,
            TARGET_RATE => Mode::Passthrough,
            rate => Mode::Filter(Box::new(Polyphase::new(rate))),
        };
        Self { mode }
    }

    /// Appends every output sample whose filter window is now complete.
    pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        match &mut self.mode {
            Mode::Empty => {}
            Mode::Passthrough => out.extend_from_slice(input),
            Mode::Filter(filter) => filter.push(input, out),
        }
    }

    /// Emits the remaining output samples, treating the missing future as
    /// silence, so the total output length is `round(n_in × 16000 / rate)`.
    pub(crate) fn finish(self, out: &mut Vec<f32>) {
        if let Mode::Filter(filter) = self.mode {
            filter.finish(out);
        }
    }
}

impl Polyphase {
    fn new(source_rate: u32) -> Self {
        let divisor = gcd(u64::from(TARGET_RATE), u64::from(source_rate));
        // n * step_num / step_den = n * source_rate / 16000, reduced.
        let step_num = u64::from(source_rate) / divisor;
        let step_den = u64::from(TARGET_RATE) / divisor;
        let quantized = step_den > MAX_PHASES;
        let phases = if quantized { MAX_PHASES } else { step_den };

        let lower_rate = source_rate.min(TARGET_RATE);
        // Cutoff in cycles per source sample.
        let cutoff = CUTOFF_FRACTION * f64::from(lower_rate) / 2.0 / f64::from(source_rate);
        // Half-length in source samples: ZERO_CROSSINGS samples of the lower rate.
        let half_width = ZERO_CROSSINGS as f64 * f64::from(source_rate) / f64::from(lower_rate);
        let half = half_width.ceil() as usize;
        let taps_per_phase = 2 * half;
        let kaiser_norm = bessel_i0(KAISER_BETA);

        let mut taps = Vec::with_capacity(phases as usize * taps_per_phase);
        for phase in 0..phases {
            let fraction = phase as f64 / phases as f64;
            let row_start = taps.len();
            // Tap k reads source index i - half + 1 + k for an output at i + fraction.
            for k in 0..taps_per_phase {
                let offset = fraction + (half as f64 - 1.0) - k as f64;
                let ratio = offset / half_width;
                let coefficient = if ratio.abs() >= 1.0 {
                    0.0
                } else {
                    let window =
                        bessel_i0(KAISER_BETA * (1.0 - ratio * ratio).sqrt()) / kaiser_norm;
                    2.0 * cutoff * sinc(2.0 * cutoff * offset) * window
                };
                taps.push(coefficient);
            }
            let sum: f64 = taps[row_start..].iter().sum();
            let gain = if sum.abs() > f64::EPSILON {
                1.0 / sum
            } else {
                1.0
            };
            for tap in &mut taps[row_start..] {
                *tap *= gain;
            }
        }

        Self {
            source_rate,
            step_num,
            step_den,
            phases,
            quantized,
            half,
            taps: taps.into_iter().map(|tap| tap as f32).collect(),
            history: Vec::new(),
            history_base: 0,
            input_len: 0,
            next_out: 0,
        }
    }

    /// Source index left of output `n` and the filter phase to use.
    fn locate(&self, n: u64) -> (u64, usize) {
        let position = n * self.step_num;
        let mut index = position / self.step_den;
        let remainder = position % self.step_den;
        if !self.quantized {
            return (index, remainder as usize);
        }
        let mut phase = (remainder * self.phases + self.step_den / 2) / self.step_den;
        if phase == self.phases {
            index += 1;
            phase = 0;
        }
        (index, phase as usize)
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.history.extend_from_slice(input);
        self.input_len += input.len() as u64;
        loop {
            let (index, phase) = self.locate(self.next_out);
            // The window reaches up to index + half; emit only once it is in.
            if index + self.half as u64 >= self.input_len {
                break;
            }
            out.push(self.output_at(index, phase));
            self.next_out += 1;
        }
        self.drop_consumed_history();
    }

    fn finish(mut self, out: &mut Vec<f32>) {
        let target = (self.input_len * u64::from(TARGET_RATE) + u64::from(self.source_rate) / 2)
            / u64::from(self.source_rate);
        while self.next_out < target {
            let (index, phase) = self.locate(self.next_out);
            out.push(self.output_at(index, phase));
            self.next_out += 1;
        }
    }

    /// One output sample; source samples before index 0 or not yet pushed
    /// count as silence.
    fn output_at(&self, index: u64, phase: usize) -> f32 {
        let taps_per_phase = 2 * self.half;
        let row = &self.taps[phase * taps_per_phase..(phase + 1) * taps_per_phase];
        let start = index as i64 - self.half as i64 + 1;
        let base = self.history_base as i64;
        let available_end = base + self.history.len() as i64;
        if start >= base && start + taps_per_phase as i64 <= available_end {
            let from = (start - base) as usize;
            let window = &self.history[from..from + taps_per_phase];
            return row
                .iter()
                .zip(window)
                .map(|(tap, sample)| tap * sample)
                .sum();
        }
        row.iter()
            .enumerate()
            .map(|(k, tap)| {
                let source = start + k as i64;
                if source < base || source >= available_end {
                    0.0
                } else {
                    tap * self.history[(source - base) as usize]
                }
            })
            .sum()
    }

    /// Keeps only the samples the next output still reads.
    fn drop_consumed_history(&mut self) {
        let (index, _) = self.locate(self.next_out);
        let needed_from = (index + 1).saturating_sub(self.half as u64);
        if needed_from > self.history_base {
            let drop = ((needed_from - self.history_base) as usize).min(self.history.len());
            self.history.drain(..drop);
            self.history_base += drop as u64;
        }
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        let arg = std::f64::consts::PI * x;
        arg.sin() / arg
    }
}

/// Modified Bessel function of the first kind, order 0 (power series).
fn bessel_i0(x: f64) -> f64 {
    let half_squared = (x / 2.0) * (x / 2.0);
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..64 {
        term *= half_squared / (k as f64 * k as f64);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_shot(input: &[f32], rate: u32) -> Vec<f32> {
        let mut resampler = StreamResampler::new(rate);
        let mut out = Vec::new();
        resampler.push(input, &mut out);
        resampler.finish(&mut out);
        out
    }

    fn chunked(input: &[f32], rate: u32, sizes: &mut dyn FnMut() -> usize) -> Vec<f32> {
        let mut resampler = StreamResampler::new(rate);
        let mut out = Vec::new();
        let mut fed = 0;
        while fed < input.len() {
            let end = (fed + sizes().max(1)).min(input.len());
            resampler.push(&input[fed..end], &mut out);
            fed = end;
        }
        resampler.finish(&mut out);
        out
    }

    /// Deterministic xorshift, so the random chunking is reproducible.
    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn noise(len: usize, seed: u64) -> Vec<f32> {
        let mut rng = XorShift(seed);
        (0..len)
            .map(|_| (rng.next() % 20_000) as f32 / 10_000.0 - 1.0)
            .collect()
    }

    fn sine(len: usize, rate: u32, frequency: f64, amplitude: f64) -> Vec<f32> {
        (0..len)
            .map(|i| {
                (amplitude
                    * (2.0 * std::f64::consts::PI * frequency * i as f64 / f64::from(rate)).sin())
                    as f32
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        (sum / samples.len().max(1) as f64).sqrt()
    }

    #[test]
    fn chunked_pushes_are_bit_identical_to_one_shot() {
        for rate in [48_000, 44_100, 8_000] {
            let input = noise(rate as usize / 2 + 123, 7);
            let reference = one_shot(&input, rate);
            for size in [1usize, 7, 480, 4_096] {
                let out = chunked(&input, rate, &mut || size);
                assert_eq!(out, reference, "rate {rate}, chunk {size}");
            }
            let mut rng = XorShift(42);
            let out = chunked(&input, rate, &mut || (rng.next() % 2_000) as usize);
            assert_eq!(out, reference, "rate {rate}, random chunks");
        }
    }

    #[test]
    fn output_length_matches_the_rate_ratio() {
        for (rate, len, expected) in [
            (48_000, 48_000, 16_000),
            (48_000, 1, 0),
            (48_000, 2, 1),
            (44_100, 44_100, 16_000),
            (44_100, 1_000, 363),
            (8_000, 1_001, 2_002),
            (16_000, 777, 777),
            (22_051, 22_051, 16_000),
        ] {
            assert_eq!(
                one_shot(&vec![0.1; len], rate).len(),
                expected,
                "rate {rate}, len {len}"
            );
        }
        assert!(one_shot(&[0.5; 100], 0).is_empty(), "rate 0 yields nothing");
        assert!(one_shot(&[], 48_000).is_empty());
    }

    #[test]
    fn passthrough_keeps_samples() {
        let input = noise(1_000, 3);
        assert_eq!(one_shot(&input, 16_000), input);
    }

    #[test]
    fn low_tone_survives_downsampling() {
        let input = sine(48_000, 48_000, 1_000.0, 0.5);
        let out = one_shot(&input, 48_000);
        let expected = sine(out.len(), 16_000, 1_000.0, 0.5);
        let edge = 64;
        let error: Vec<f32> = out[edge..out.len() - edge]
            .iter()
            .zip(&expected[edge..expected.len() - edge])
            .map(|(actual, wanted)| actual - wanted)
            .collect();
        assert!(rms(&error) < 1e-3, "rms error {}", rms(&error));
    }

    #[test]
    fn tone_above_target_nyquist_is_filtered() {
        let input = sine(48_000, 48_000, 12_000.0, 0.5);
        let out = one_shot(&input, 48_000);
        let ratio = rms(&out) / rms(&input);
        assert!(ratio < 0.01, "12 kHz leaked through with ratio {ratio}");
    }

    #[test]
    fn exotic_rates_quantize_their_phases() {
        let filter = Polyphase::new(44_101);
        assert!(filter.quantized);
        assert_eq!(filter.phases, MAX_PHASES);
        let input = noise(44_101, 9);
        let reference = one_shot(&input, 44_101);
        assert_eq!(chunked(&input, 44_101, &mut || 333), reference);
    }
}
