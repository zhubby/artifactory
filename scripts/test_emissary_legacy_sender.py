#!/usr/bin/env python3
"""[INPUT]: 反向兼容驱动和有限寿命的伪接收器、发送器与编译子进程。
[OUTPUT]: 验证进程组回收、顺序结果与精确清库回执。
[POS]: 无数据库故障回归；只创建本次私有临时目录和直接子进程。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

import check_emissary_legacy_sender as driver


RECEIVER = '''import json, os, pathlib, time
root = pathlib.Path(os.environ["ARTIFACTORY_LEGACY_SENDER_DIR"])
(root / "receiver.pid").write_text(str(os.getpid()))
for index in range(3):
    test = "prepare_frozen_source" if index == 0 else "frozen_sender_to_current_receiver"
    (root / f"request-{index}.json").write_text(json.dumps({"sequence":index,"test":test}))
    deadline = time.monotonic() + 8
    result = root / f"result-{index}.json"
    while not result.exists() and time.monotonic() < deadline:
        time.sleep(.01)
    assert result.exists(), "supervisor did not publish result"
    assert json.loads(result.read_text()) == {"sequence":index,"success":True,"process_reaped":True}
    sender = int((root / f"sender-{index}.pid").read_text())
    try:
        os.kill(sender, 0)
    except ProcessLookupError:
        pass
    else:
        raise RuntimeError("result preceded sender reaping")
(root / "receiver-completed").write_text("yes")
'''

SENDER = '''import os, pathlib, sys, time
root = pathlib.Path(os.environ["ARTIFACTORY_LEGACY_SENDER_DIR"])
assert all(key not in os.environ for key in (
    "DATABASE_URL", "ARTIFACTORY_TEST_DATABASE_URL", "ARTIFACTORY_BUSINESS_DATABASE_URL",
    "ARTIFACTORY_TELEMETRY_READ_DATABASE_URL", "ARTIFACTORY_PLANNING_DATABASE_URL"))
pending = [index for index in range(3) if (root / f"request-{index}.json").exists()
           and not (root / f"result-{index}.json").exists()]
assert len(pending) == 1
index = pending[0]
(root / f"sender-{index}.pid").write_text(str(os.getpid()))
with (root / "invocations").open("a") as output:
    output.write(str(index) + "\\n")
time.sleep(float(sys.argv[1]))
'''

COMPILER = '''import os, pathlib, subprocess, sys, time
root = pathlib.Path(sys.argv[1])
child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(15)"])
(root / "descendant.pid").write_text(str(child.pid))
(root / "leader.pid").write_text(str(os.getpid()))
time.sleep(float(sys.argv[2]))
'''


@unittest.skipUnless(os.name == "posix" and hasattr(os, "WNOWAIT"), "POSIX group proof required")
class ProcessCleanup(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="legacy-sender-cleanup-")
        self.root = Path(self.temporary.name)
        self.fixture = self.root / "fixture"
        self.fixture.mkdir()
        self.receiver = self.root / "receiver.py"
        self.sender = self.root / "sender.py"
        self.compiler = self.root / "compiler.py"
        self.receiver.write_text(RECEIVER)
        self.sender.write_text(SENDER)
        self.compiler.write_text(COMPILER)
        self.supervisor = driver.Supervisor()
        self.addCleanup(self.temporary.cleanup)
        self.addCleanup(self.supervisor.cleanup)

    def env(self):
        return dict(os.environ, ARTIFACTORY_LEGACY_SENDER_DIR=str(self.fixture))

    def commands(self, duration):
        return [sys.executable, self.sender, str(duration)]

    def delivery(self, duration=0.02, timeout=5):
        driver.run_delivery(self.supervisor, [sys.executable, self.receiver], self.commands(duration),
                            cwd=self.root, env=self.env(), fixture=self.fixture, logs=self.root,
                            timeout=timeout)

    def assert_owned_processes_absent(self):
        self.assertTrue(self.supervisor.children)
        for child in self.supervisor.children:
            self.assertTrue(child.confirmed, child.label)
            self.assertEqual(child.live_group(), [], child.label)
            with self.assertRaises(ProcessLookupError):
                os.kill(child.process.pid, 0)

    def test_three_invocations_publish_results_after_reaping_without_credentials(self):
        credentials = {name: "private-test-value" for name in driver.DATABASE_ENV}
        with patch.dict(os.environ, credentials):
            self.delivery()
        self.assertEqual((self.fixture / "invocations").read_text(), "0\n1\n2\n")
        self.assertTrue((self.fixture / "receiver-completed").exists())
        self.assert_owned_processes_absent()

    def test_receiver_timeout_reaps_receiver_and_active_sender(self):
        with self.assertRaisesRegex(RuntimeError, "outer deadline"):
            self.delivery(duration=15, timeout=0.4)
        self.assertTrue((self.fixture / "sender-0.pid").exists())
        self.assertFalse((self.fixture / "result-0.json").exists())
        self.assert_owned_processes_absent()

    def test_interrupt_reaps_receiver_and_active_sender(self):
        original = driver.time.sleep

        def interrupt_after_sender_started(seconds):
            if (self.fixture / "sender-0.pid").exists():
                raise KeyboardInterrupt()
            original(seconds)

        with patch.object(driver.time, "sleep", side_effect=interrupt_after_sender_started):
            with self.assertRaises(KeyboardInterrupt):
                self.delivery(duration=15)
        self.assert_owned_processes_absent()

    def test_compile_timeout_reaps_group_descendant(self):
        child = self.supervisor.start([sys.executable, self.compiler, self.root, "15"],
                                      cwd=self.root, env=self.env(),
                                      log=self.root / "compiler.log", label="fake-compiler")
        with self.assertRaises(subprocess.TimeoutExpired):
            child.wait(0.3)
        self.assertTrue((self.root / "descendant.pid").exists())
        self.assert_owned_processes_absent()

    def test_normal_leader_exit_with_live_descendant_fails_after_cleanup(self):
        child = self.supervisor.start([sys.executable, self.compiler, self.root, "0"],
                                      cwd=self.root, env=self.env(),
                                      log=self.root / "compiler.log", label="fake-compiler")
        with self.assertRaisesRegex(RuntimeError, "live descendants"):
            child.wait(3)
        self.assert_owned_processes_absent()

    def test_interrupt_after_reaping_never_uses_old_numeric_group(self):
        child = self.supervisor.start([sys.executable, "-c", "pass"],
                                      cwd=self.root, env=self.env(),
                                      log=self.root / "post-reap.log", label="post-reap")
        original = child.process.wait

        def interrupted_after_wait(*args, **kwargs):
            result = original(*args, **kwargs)
            raise KeyboardInterrupt()

        with patch.object(child.process, "wait", side_effect=interrupted_after_wait):
            with self.assertRaises(KeyboardInterrupt):
                child.wait(3)
        with patch.object(child, "live_group", side_effect=AssertionError("old PGID lookup")), \
                patch.object(driver.os, "killpg", side_effect=AssertionError("old PGID signal")):
            child.close()
        self.assertTrue(child.confirmed)

    def test_cleanup_failure_is_not_overwritten_by_later_success(self):
        child = self.supervisor.start([sys.executable, "-c", "import time; time.sleep(15)"],
                                      cwd=self.root, env=self.env(),
                                      log=self.root / "process.log", label="owned-child")
        with patch.object(child, "close", side_effect=RuntimeError("injected group failure")):
            with self.assertRaisesRegex(RuntimeError, "cleanup_unconfirmed"):
                self.supervisor.cleanup()
        self.supervisor.cleanup()
        self.assertTrue(self.supervisor.cleanup_failed)
        self.assertFalse(self.supervisor.cleanup_confirmed())
        self.assert_owned_processes_absent()

    def test_failed_close_remains_unconfirmed_after_cleanup_retries(self):
        child = self.supervisor.start([sys.executable, "-c", "import time; time.sleep(15)"],
                                      cwd=self.root, env=self.env(),
                                      log=self.root / "close-failure.log", label="owned-child")
        original = child.live_group
        failed = False

        def fail_once():
            nonlocal failed
            if not failed:
                failed = True
                raise RuntimeError("injected first close failure")
            return original()

        with patch.object(child, "live_group", side_effect=fail_once):
            with self.assertRaisesRegex(RuntimeError, "first close failure"):
                child.close()
        self.supervisor.cleanup()
        self.assertFalse(self.supervisor.cleanup_confirmed())

    def test_signal_at_spawn_boundary_registers_child_before_interrupt(self):
        popen = subprocess.Popen

        def signal_after_spawn(*args, **kwargs):
            child = popen(*args, **kwargs)
            os.kill(os.getpid(), signal.SIGINT)
            return child

        with patch.object(driver.subprocess, "Popen", side_effect=signal_after_spawn):
            with self.assertRaises(KeyboardInterrupt):
                self.supervisor.start([sys.executable, "-c", "import time; time.sleep(15)"],
                                      cwd=self.root, env=self.env(),
                                      log=self.root / "spawn-interrupt.log", label="spawned-child")
        self.supervisor.cleanup()
        self.assert_owned_processes_absent()

    def test_no_group_lookup_or_signal_after_reaping(self):
        child = self.supervisor.start([sys.executable, "-c", "pass"],
                                      cwd=self.root, env=self.env(),
                                      log=self.root / "reaped.log", label="reaped-child")
        self.assertEqual(child.wait(3), 0)
        with patch.object(child, "live_group", side_effect=AssertionError("reaped group lookup")), \
                patch.object(os, "killpg", side_effect=AssertionError("reaped group signal")):
            child.close()
            self.supervisor.cleanup()

    def test_child_exit_signals_are_unblocked(self):
        child = self.supervisor.start([sys.executable, "-c",
            "import signal; mask=signal.pthread_sigmask(signal.SIG_BLOCK, set()); "
            "assert signal.SIGTERM not in mask and signal.SIGINT not in mask"],
            cwd=self.root, env=self.env(), log=self.root / "signals.log", label="signal-mask")
        self.assertEqual(child.wait(3), 0)


class DatabaseCleanupReceipt(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="legacy-cleanup-receipt-")
        self.root = Path(self.temporary.name)
        (self.root / "fixture").mkdir()
        self.addCleanup(self.temporary.cleanup)
        self.name = "durable_receiver_" + "a" * 32
        self.env = {"ARTIFACTORY_LEGACY_DATABASE_NAME": self.name}
        self.supervisor = Mock()
        self.supervisor.start.return_value.wait.return_value = 0

    def cleanup(self, name=None):
        driver.cleanup_database(self.supervisor, "receiver", cwd=self.root,
            env=self.env, temporary=self.root, name=name or self.name)

    def test_exact_cleanup_receipt_required(self):
        receipt = self.root / "fixture/database-cleaned.json"
        driver.private_json(receipt, {"name": self.name, "database_removed": True})
        self.cleanup()
        self.supervisor.start.return_value.wait.assert_called_once_with(45)
        driver.private_json(receipt, {"name": "durable_receiver_" + "b" * 32,
                                      "database_removed": True})
        with self.assertRaisesRegex(RuntimeError, "read-back mismatch"):
            self.cleanup()

    def test_invalid_or_different_target_never_starts_cleanup(self):
        for name in ("production", "durable_receiver_" + "b" * 32):
            with self.assertRaises(RuntimeError):
                self.cleanup(name)
        self.supervisor.start.assert_not_called()


if __name__ == "__main__":
    unittest.main()
