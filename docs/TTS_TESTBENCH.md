# Local TTS Testbench

`scripts/tts_testbench.py` generates comparable German WAV samples for local TTS
engines without changing the chat UI. It is intended for quick A/B listening
before deciding whether a provider belongs in the app.

The controller has no repo-level Python dependencies. XTTS-v2, MMS-DE, and Bark
are optional runtime dependencies that you install only in the Python
environment used for the experiment.

## Quick Check

Preview the planned outputs without importing any engine:

```bash
python3 scripts/tts_testbench.py --engines xtts,mms,bark --dry-run
```

Generated WAV files go to `artifacts/tts-testbench/`, which is ignored by git.

## Engine Setup

Use a throwaway virtual environment because these packages are large:

```bash
python3 -m venv .venv-tts
source .venv-tts/bin/activate
python -m pip install --upgrade pip
```

Install only the engines you want to test:

```bash
# MMS-DE
python -m pip install torch transformers

# XTTS-v2
python -m pip install TTS

# Bark
python -m pip install git+https://github.com/suno-ai/bark.git
```

Each engine may download model weights into its normal cache on first use.

## Run Samples

MMS-DE:

```bash
python3 scripts/tts_testbench.py --engines mms
```

XTTS-v2 needs a local reference speaker WAV:

```bash
python3 scripts/tts_testbench.py --engines xtts --speaker-wav /path/to/reference-voice.wav
```

Bark:

```bash
python3 scripts/tts_testbench.py --engines bark
```

Piper can be included as the current baseline after its voice has been
downloaded once in Donny:

```bash
python3 scripts/tts_testbench.py --engines piper,xtts,mms,bark --speaker-wav /path/to/reference-voice.wav
```

Use custom German samples with stable file names:

```bash
python3 scripts/tts_testbench.py \
  --engines mms,bark \
  --sample begruessung="Hallo, ich teste eine deutsche Stimme." \
  --sample chat="Ich habe die Antwort gefunden und fasse sie kurz zusammen."
```

## Output

The script writes one WAV per engine and sample:

```text
artifacts/tts-testbench/
  mms/01-kurzer-gruss.wav
  bark/01-kurzer-gruss.wav
```

Exit code `0` means every requested job generated audio. Exit code `1` means at
least one job worked and at least one failed. Exit code `2` means nothing was
generated, usually because dependencies or model files are missing.
