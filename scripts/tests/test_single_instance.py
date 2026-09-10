#!/usr/bin/env python3
"""Compile the production startup gate and exercise it in independent processes.

No GUI, bridge, microphone, user settings or installed app is initialized.
Run with: python3 scripts/tests/test_single_instance.py
"""

import concurrent.futures
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import tempfile
import threading
import unittest


class SingleInstanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory(prefix="torro-instance-build-")
        cls.addClassCleanup(cls.build.cleanup)
        root = Path(__file__).resolve().parents[2]
        cls.binary = Path(cls.build.name) / "TorroWhisperProbe"
        subprocess.run(
            [
                "swiftc", "-swift-version", "6", "-O", "-parse-as-library",
                str(root / "apps/torrowhisper-macos/Sources/TorroWhisper/SingleInstanceGuard.swift"),
                str(root / "scripts/tests/SingleInstanceProbe.swift"),
                "-o", str(cls.binary),
            ],
            check=True, timeout=120,
        )

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="torro-instance-test-")
        self.addCleanup(self.directory.cleanup)
        self.lock = Path(self.directory.name) / "data" / "instance.lock"

    def start(self, binary=None, child=False):
        command = [str(binary or self.binary), str(self.lock)]
        if child:
            command.append("child")
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
        self.addCleanup(self.cleanup_process, process)
        return process

    @staticmethod
    def cleanup_process(process):
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=10)

    def read_line(self, process):
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            self.assertTrue(selector.select(timeout=10), "Probe failed to respond")
        return process.stdout.readline()

    def assert_started(self, process):
        self.assertEqual(self.read_line(process), b"started\n")
        self.assertIsNone(process.poll())

    def assert_rejected(self, process, status=0):
        out, err = process.communicate(timeout=10)
        self.assertEqual(process.returncode, status, err.decode())
        self.assertEqual(out, b"", "Rejected process entered the app body")
        return err

    def test_second_launch_never_enters_app_body_and_preserves_holder(self):
        holder = self.start()
        self.assert_started(holder)
        inode = self.lock.stat().st_ino
        self.assert_rejected(self.start())
        self.assertIsNone(holder.poll())
        self.assertEqual(self.lock.stat().st_ino, inode)

    def test_normal_exit_and_existing_lock_file_allow_restart(self):
        holder = self.start()
        self.assert_started(holder)
        inode = self.lock.stat().st_ino
        holder.communicate(input=b"quit\n", timeout=10)
        self.assertEqual(holder.returncode, 0)
        self.assertEqual(self.lock.stat().st_ino, inode)
        self.assert_started(self.start())

    def test_sigkill_releases_lock(self):
        holder = self.start()
        self.assert_started(holder)
        holder.kill()
        holder.wait(timeout=10)
        self.assert_started(self.start())

    def test_concurrent_launches_have_exactly_one_winner(self):
        barrier = threading.Barrier(8)

        def launch(_):
            barrier.wait(timeout=10)
            return subprocess.Popen(
                [str(self.binary), str(self.lock)],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0,
            )

        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            processes = list(pool.map(launch, range(8)))
        for process in processes:
            self.addCleanup(self.cleanup_process, process)
        winners = []
        for process in processes:
            line = self.read_line(process)
            if line == b"started\n":
                winners.append(process)
            else:
                self.assertEqual(line, b"")
                self.assert_rejected(process)
        self.assertEqual(len(winners), 1)
        self.assertIsNone(winners[0].poll())

    def test_different_binary_paths_share_lock_in_both_orders(self):
        copy = Path(self.directory.name) / "OtherCopy"
        shutil.copy2(self.binary, copy)
        for first, second in [(self.binary, copy), (copy, self.binary)]:
            with self.subTest(first=first):
                holder = self.start(binary=first)
                self.assert_started(holder)
                self.assert_rejected(self.start(binary=second))
                holder.communicate(input=b"quit\n", timeout=10)
                self.assertEqual(holder.returncode, 0)

    def test_lock_error_fails_closed(self):
        self.lock.parent.parent.joinpath("data").write_text("not a directory")
        err = self.assert_rejected(self.start(), status=1)
        self.assertIn(b"could not acquire its instance lock", err)

    def test_symlink_lock_fails_closed_without_touching_target(self):
        self.lock.parent.mkdir()
        target = Path(self.directory.name) / "settings-sentinel"
        target.write_text("unchanged")
        self.lock.symlink_to(target)
        self.assert_rejected(self.start(), status=1)
        self.assertEqual(target.read_text(), "unchanged")

    def test_executed_child_does_not_keep_lock_alive(self):
        holder = self.start(child=True)
        self.assert_started(holder)
        child_line = self.read_line(holder)
        self.assertTrue(child_line.startswith(b"child="))
        child_pid = int(child_line.removeprefix(b"child="))
        self.addCleanup(self.stop_child, child_pid)
        holder.kill()
        holder.wait(timeout=10)
        os.kill(child_pid, 0)
        self.assert_started(self.start())

    @staticmethod
    def stop_child(pid):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass


if __name__ == "__main__":
    unittest.main(verbosity=2)
