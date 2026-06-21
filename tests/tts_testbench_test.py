import importlib.util
import io
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path


def load_module():
    root = Path(__file__).resolve().parents[1]
    module_path = root / "scripts" / "tts_testbench.py"
    spec = importlib.util.spec_from_file_location("tts_testbench", module_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class TtsTestbenchTests(unittest.TestCase):
    def test_parse_engines_expands_all_in_stable_order(self):
        tb = load_module()

        self.assertEqual(tb.parse_engines("all"), ["piper", "xtts", "mms", "bark"])
        self.assertEqual(tb.parse_engines("xtts,mms"), ["xtts", "mms"])

    def test_parse_engines_rejects_unknown_engine(self):
        tb = load_module()

        with self.assertRaisesRegex(ValueError, "unknown TTS engine"):
            tb.parse_engines("xtts,unknown")

    def test_build_jobs_uses_numbered_german_sample_names(self):
        tb = load_module()

        jobs = tb.build_jobs(["xtts", "mms"], tb.DEFAULT_SAMPLES[:2], Path("/tmp/out"))

        self.assertEqual(
            [(job.engine, job.sample_id, job.out_path.as_posix()) for job in jobs],
            [
                ("xtts", "kurzer-gruss", "/tmp/out/xtts/01-kurzer-gruss.wav"),
                ("xtts", "zahlen-und-abkuerzungen", "/tmp/out/xtts/02-zahlen-und-abkuerzungen.wav"),
                ("mms", "kurzer-gruss", "/tmp/out/mms/01-kurzer-gruss.wav"),
                ("mms", "zahlen-und-abkuerzungen", "/tmp/out/mms/02-zahlen-und-abkuerzungen.wav"),
            ],
        )

    def test_dry_run_creates_no_audio_files(self):
        tb = load_module()

        with tempfile.TemporaryDirectory() as tmp:
            with redirect_stdout(io.StringIO()):
                exit_code = tb.main(["--engines", "xtts", "--out-dir", tmp, "--dry-run"])

            self.assertEqual(exit_code, 0)
            self.assertEqual(list(Path(tmp).rglob("*.wav")), [])


if __name__ == "__main__":
    unittest.main()
