#!/usr/bin/env python3
"""Run controlled Whisper benchmarks; keep raw JSON and backend logs.

Suites: `tuning` (default, #88) compares flash attention and a fitted audio_ctx
on a long and a short clip; `baseline` repeats #43's CPU/Metal/thread comparison.

Build first: cargo build --release -p torrowhisper-bridge --example whisper_benchmark
Run: python3 scripts/benchmark-whisper.py --binary target/release/examples/whisper_benchmark --output /tmp/whisper-results
"""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]


def output(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


TURBO_MODELS = ("large_v3_turbo_q5_0", "large_v3_turbo")
TUNING_VARIANTS = [
    (fa, ctx, clip)
    for fa in ("nofa", "fa")
    for ctx in ("noctx", "ctx")
    for clip in ("short", "long")
]


def baseline_configs():
    """#43's comparison, pinned to the previous decoding (no FA, full window)."""
    configs = []
    for model in TURBO_MODELS:
        configs.append((model, "cpu", 6, "de", "multi"))
        # Each configuration is a new process. Alternate thread-count order
        # between models to reduce a simple chronological/thermal bias.
        counts = (1, 2, 4, 6, 8) if model.endswith("q5_0") else (8, 6, 4, 2, 1)
        configs.extend((model, "metal", n, "de", "multi") for n in counts)
    configs.extend([
        ("standard", "cpu", 6, "de", "multi"),
        ("standard", "metal", 6, "de", "multi"),
        ("large_v3_turbo", "metal", 6, "auto", "multi"),
        ("large_v3_turbo", "metal", 6, "de", "single"),
    ])
    return [config + ("nofa", "noctx", "long") for config in configs]


def tuning_configs():
    """#88: flash attention x fitted audio_ctx x clip, against nofa-noctx."""
    configs = []
    for index, model in enumerate(TURBO_MODELS):
        # Reverse the variant order between models against thermal/order bias.
        variants = TUNING_VARIANTS if index == 0 else TUNING_VARIANTS[::-1]
        for backend in ("metal", "cpu"):
            configs.extend((model, backend, 6, "de", "multi") + v for v in variants)
    # Auto language detection on a shortened window could misdetect the language.
    configs.extend(
        ("large_v3_turbo_q5_0", "metal", 6, "auto", "multi") + v for v in TUNING_VARIANTS
    )
    # Optional streaming_thread_count question: fewer threads on Metal?
    configs.extend(
        (model, "metal", threads, "de", "multi", "nofa", "noctx", "short")
        for model in TURBO_MODELS
        for threads in (2, 4)
    )
    return configs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--suite", choices=("tuning", "baseline"), default="tuning")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("the CPU/Metal comparison requires macOS")
    if not 1 <= args.repetitions <= 20:
        parser.error("repetitions must be 1..20")
    binary = args.binary.resolve(strict=True)
    # Never silently replace an earlier measurement series.
    args.output.mkdir(parents=True, exist_ok=False)
    metadata = {
        "source_commit": output("git", "rev-parse", "HEAD"),
        "source_dirty": bool(output("git", "status", "--porcelain")),
        "os": output("sw_vers", "-productVersion"),
        "hardware": output("sysctl", "-n", "hw.model"),
        "chip": output("sysctl", "-n", "machdep.cpu.brand_string"),
        "memory_bytes": int(output("sysctl", "-n", "hw.memsize")),
        "rustc": output("rustc", "--version"),
        "binary_sha256": sha256(binary),
        "suite": args.suite,
        "reference_sha256": sha256(ROOT / "crates/torrowhisper-bridge/resources/benchmark-de.wav"),
        "reference_short_sha256": sha256(
            ROOT / "crates/torrowhisper-bridge/resources/benchmark-de-short.wav"
        ),
        "source_sha256": {
            path: sha256(ROOT / path) for path in (
                "crates/torrowhisper-bridge/src/benchmark.rs",
                "crates/torrowhisper-bridge/src/benchmark/validation.rs",
                "crates/torrowhisper-bridge/src/dictation.rs",
                "crates/torrowhisper-bridge/src/streaming_transcription.rs",
                "crates/torrowhisper-bridge/examples/whisper_benchmark.rs",
                "scripts/benchmark-whisper.py",
                "Cargo.lock",
            )
        },
        "repetitions": args.repetitions,
        "scope": "Whisper only; excludes capture, VAD trimming, LLM post-processing and insertion",
        "baseline": "Same binary and decoding, use_gpu=false; CPU backend may use Accelerate/BLAS. Not a historical-build replay.",
    }
    model_dir = Path.home() / "Library/Application Support/com.gettorro.TorroWhisper/models"
    metadata["models_sha256"] = {
        filename: sha256(model_dir / filename) for filename in (
            "ggml-large-v3-turbo-q5_0.bin", "ggml-large-v3-turbo.bin", "ggml-small.bin",
        )
    }
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")

    configs = tuning_configs() if args.suite == "tuning" else baseline_configs()
    summary = []
    for model, backend, threads, language, segments, fa, ctx, clip in configs:
        key = f"{model}-{backend}-{threads}-{language}-{segments}-{fa}-{ctx}-{clip}"
        print(f"Measuring {key}", flush=True)
        command = [
            str(binary), model, backend, str(threads), str(args.repetitions),
            language, segments, fa, ctx, clip,
        ]
        with (args.output / f"{key}.log").open("w") as log:
            result = subprocess.run(command, stdout=subprocess.PIPE, stderr=log, text=True, timeout=900)
        (args.output / f"{key}.json").write_text(result.stdout)
        if result.returncode:
            raise RuntimeError(f"{key} failed ({result.returncode}); see backend log")
        report = json.loads(result.stdout)
        if len(report["warm"]) != args.repetitions or any(p["inference_secs"] <= 0 for p in report["warm"]):
            raise RuntimeError(f"{key}: missing real measurements")
        summary.append({
            "configuration": key,
            "clip": report["clip"],
            "audio_secs": report["audio_secs"],
            "flash_attn": report["flash_attn"],
            "audio_ctx": report["audio_ctx"],
            "language": language,
            "load_secs": report["load_secs"],
            "cold_total_secs": report["cold"]["total_secs"],
            "warm_median_secs": statistics.median(p["total_secs"] for p in report["warm"]),
            "warm_min_secs": min(p["total_secs"] for p in report["warm"]),
            "warm_max_secs": max(p["total_secs"] for p in report["warm"]),
            "warm_median_inference_secs": statistics.median(p["inference_secs"] for p in report["warm"]),
            "warm_median_rtf": statistics.median(p["real_time_factor"] for p in report["warm"]),
            "max_wer": max(p["word_error_rate"] for p in report["warm"]),
        })
        (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps(summary[-1]), flush=True)
        time.sleep(2)
    print(f"Completed {len(summary)} configurations in {args.output}")


if __name__ == "__main__":
    main()
