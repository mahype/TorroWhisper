#!/usr/bin/env python3
"""
Generate comparable local TTS samples for Piper, XTTS-v2, MMS-DE, and Bark.

The controller itself uses only Python's standard library. Each neural engine is
loaded only when requested, so the repo's normal tests do not need heavyweight
TTS packages installed.
"""

import argparse
import os
import re
import struct
import subprocess
import sys
import wave
from pathlib import Path
from typing import NamedTuple


ENGINE_ORDER = ["piper", "xtts", "mms", "bark"]
SHERPA_VERSION = "1.13.2"
DEFAULT_PIPER_VOICE = "de_DE-thorsten-high"


class Sample(NamedTuple):
    sample_id: str
    text: str


class TtsJob(NamedTuple):
    engine: str
    sample_index: int
    sample_id: str
    text: str
    out_path: Path


class EngineUnavailable(RuntimeError):
    pass


DEFAULT_SAMPLES = [
    Sample(
        "kurzer-gruss",
        "Hallo, ich bin DonnyWhisper. Diese Stimme wird komplett lokal erzeugt.",
    ),
    Sample(
        "zahlen-und-abkuerzungen",
        "Am 16. Juni 2026 testen wir TTS, Abkuerzungen wie z. B. und Zahlen wie 42.",
    ),
    Sample(
        "chat-antwort",
        "Ich habe drei Optionen gefunden. Die erste ist schnell, die zweite klingt natuerlicher, und die dritte ist eher experimentell.",
    ),
]


def parse_engines(value):
    raw = [part.strip().lower() for part in value.split(",") if part.strip()]
    if not raw or raw == ["all"]:
        return list(ENGINE_ORDER)

    engines = []
    for engine in raw:
        if engine not in ENGINE_ORDER:
            raise ValueError(
                f"unknown TTS engine '{engine}'. Use one of: {', '.join(ENGINE_ORDER)}, all"
            )
        if engine not in engines:
            engines.append(engine)
    return engines


def parse_samples(values):
    if not values:
        return list(DEFAULT_SAMPLES)

    samples = []
    for index, value in enumerate(values, start=1):
        if "=" in value:
            sample_id, text = value.split("=", 1)
            sample_id = slugify(sample_id)
        else:
            sample_id = f"custom-{index}"
            text = value
        text = text.strip()
        if not text:
            raise ValueError("--sample text must not be empty")
        samples.append(Sample(sample_id or f"custom-{index}", text))
    return samples


def slugify(value):
    value = value.lower().strip()
    value = re.sub(r"[^a-z0-9]+", "-", value)
    return value.strip("-") or "sample"


def build_jobs(engines, samples, out_dir):
    jobs = []
    for engine in engines:
        for index, sample in enumerate(samples, start=1):
            filename = f"{index:02d}-{slugify(sample.sample_id)}.wav"
            jobs.append(
                TtsJob(
                    engine=engine,
                    sample_index=index,
                    sample_id=sample.sample_id,
                    text=sample.text,
                    out_path=out_dir / engine / filename,
                )
            )
    return jobs


def run_piper(job, args):
    voice = args.piper_voice
    tts_root = Path(args.piper_root).expanduser()
    cli_root = tts_root / f"sherpa-onnx-v{SHERPA_VERSION}-osx-arm64-shared"
    bin_path = cli_root / "bin" / "sherpa-onnx-offline-tts"
    lib_path = cli_root / "lib"
    model_root = tts_root / f"vits-piper-{voice}"
    onnx = model_root / f"{voice}.onnx"
    tokens = model_root / "tokens.txt"
    data_dir = model_root / "espeak-ng-data"

    missing = [path for path in [bin_path, onnx, tokens, data_dir] if not path.exists()]
    if missing:
        lines = "\n".join(f"  - {path}" for path in missing)
        raise EngineUnavailable(
            "Piper assets are missing. Download the voice once in DonnyWhisper, "
            f"or pass --piper-root. Missing:\n{lines}"
        )

    job.out_path.parent.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env["DYLD_LIBRARY_PATH"] = str(lib_path)
    subprocess.run(
        [
            str(bin_path),
            f"--vits-model={onnx}",
            f"--vits-tokens={tokens}",
            f"--vits-data-dir={data_dir}",
            f"--vits-length-scale={1.0 / max(args.rate, 0.1)}",
            "--num-threads=4",
            f"--output-filename={job.out_path}",
            job.text,
        ],
        check=True,
        env=env,
    )


def run_xtts(job, args):
    if not args.speaker_wav:
        raise EngineUnavailable(
            "XTTS-v2 needs --speaker-wav /path/to/reference.wav for a German reference voice."
        )
    speaker_wav = Path(args.speaker_wav).expanduser()
    if not speaker_wav.exists():
        raise EngineUnavailable(f"XTTS-v2 speaker WAV does not exist: {speaker_wav}")

    try:
        from TTS.api import TTS
    except ImportError as err:
        raise EngineUnavailable(
            "XTTS-v2 needs Coqui TTS installed in the active Python environment: "
            "python3 -m pip install TTS"
        ) from err

    job.out_path.parent.mkdir(parents=True, exist_ok=True)
    tts = TTS("tts_models/multilingual/multi-dataset/xtts_v2")
    tts.tts_to_file(
        text=job.text,
        language="de",
        speaker_wav=str(speaker_wav),
        file_path=str(job.out_path),
    )


def run_mms(job, args):
    try:
        import torch
        from transformers import AutoTokenizer, VitsModel
    except ImportError as err:
        raise EngineUnavailable(
            "MMS-DE needs torch and transformers installed in the active Python environment: "
            "python3 -m pip install torch transformers"
        ) from err

    model_name = args.mms_model
    tokenizer = AutoTokenizer.from_pretrained(model_name)
    model = VitsModel.from_pretrained(model_name)
    inputs = tokenizer(job.text, return_tensors="pt")

    with torch.no_grad():
        waveform = model(**inputs).waveform.squeeze().cpu().tolist()

    sample_rate = int(model.config.sampling_rate)
    job.out_path.parent.mkdir(parents=True, exist_ok=True)
    write_wav(job.out_path, waveform, sample_rate)


def run_bark(job, args):
    try:
        from bark import SAMPLE_RATE, generate_audio, preload_models
    except ImportError as err:
        raise EngineUnavailable(
            "Bark needs the Bark package installed in the active Python environment."
        ) from err

    preload_models()
    waveform = generate_audio(job.text)
    job.out_path.parent.mkdir(parents=True, exist_ok=True)
    write_wav(job.out_path, waveform, int(SAMPLE_RATE))


def write_wav(path, waveform, sample_rate):
    values = [float(value) for value in waveform]
    peak = max([abs(value) for value in values] + [1.0])
    frames = bytearray()
    for value in values:
        sample = int(max(-1.0, min(1.0, value / peak)) * 32767)
        frames.extend(struct.pack("<h", sample))

    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(sample_rate)
        out.writeframes(bytes(frames))


def run_job(job, args):
    runners = {
        "piper": run_piper,
        "xtts": run_xtts,
        "mms": run_mms,
        "bark": run_bark,
    }
    runners[job.engine](job, args)


def default_piper_root():
    return str(
        Path.home()
        / "Library"
        / "Application Support"
        / "dev.awesome.donnywhisper"
        / "tts"
    )


def build_parser():
    parser = argparse.ArgumentParser(
        description="Generate local German TTS comparison WAVs.",
    )
    parser.add_argument(
        "--engines",
        default="all",
        help="Comma-separated engines: piper,xtts,mms,bark, or all.",
    )
    parser.add_argument(
        "--out-dir",
        default="artifacts/tts-testbench",
        help="Output directory for generated WAV files.",
    )
    parser.add_argument(
        "--sample",
        action="append",
        help="Sample text. Use id=text for stable file names. Repeat for multiple samples.",
    )
    parser.add_argument(
        "--speaker-wav",
        help="Reference speaker WAV for XTTS-v2.",
    )
    parser.add_argument(
        "--piper-root",
        default=default_piper_root(),
        help="DonnyWhisper TTS asset directory containing the sherpa-onnx Piper bundle.",
    )
    parser.add_argument(
        "--piper-voice",
        default=DEFAULT_PIPER_VOICE,
        help="Piper voice id already downloaded by DonnyWhisper.",
    )
    parser.add_argument(
        "--mms-model",
        default="facebook/mms-tts-deu",
        help="Hugging Face model id or local path for MMS-DE.",
    )
    parser.add_argument(
        "--rate",
        type=float,
        default=1.0,
        help="Approximate Piper speed multiplier used only for the Piper CLI.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print planned outputs without importing engines or writing audio.",
    )
    parser.add_argument(
        "--fail-fast",
        action="store_true",
        help="Stop after the first engine failure instead of continuing.",
    )
    return parser


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)

    try:
        engines = parse_engines(args.engines)
        samples = parse_samples(args.sample)
    except ValueError as err:
        parser.error(str(err))

    out_dir = Path(args.out_dir).expanduser()
    jobs = build_jobs(engines, samples, out_dir)

    if args.dry_run:
        for job in jobs:
            print(f"{job.engine:5s} -> {job.out_path}: {job.text}")
        return 0

    generated = 0
    failures = 0
    for job in jobs:
        print(f"[{job.engine}] {job.sample_id} -> {job.out_path}", flush=True)
        try:
            run_job(job, args)
        except EngineUnavailable as err:
            failures += 1
            print(f"skip: {err}", file=sys.stderr)
            if args.fail_fast:
                break
        except (OSError, subprocess.CalledProcessError, RuntimeError) as err:
            failures += 1
            print(f"error: {job.engine} failed: {err}", file=sys.stderr)
            if args.fail_fast:
                break
        else:
            generated += 1

    print(f"generated {generated} WAV file(s); {failures} job(s) skipped or failed")
    if failures:
        return 2 if generated == 0 else 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
