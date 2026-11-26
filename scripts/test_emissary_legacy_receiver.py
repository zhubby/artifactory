#!/usr/bin/env python3
"""[INPUT]: 旧接收器驱动、伪造连接配置与受控子进程。
[OUTPUT]: 验证回环准入、目标覆盖拒绝和只停止本次夹具。
[POS]: 本地门禁安全回归，不连接数据库或容器。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

import check_emissary_legacy_receiver as driver


class Isolation(unittest.TestCase):
    def test_loopback_and_query_target_admission(self):
        for value in (
            "postgresql://user:secret@127.0.0.1:15432/test",
            "postgres://localhost/test?sslmode=disable",
            "postgresql://[::1]/test",
        ):
            self.assertEqual(driver.validate_database_url(value), value)
        for value in (
            "postgresql://database.example/test",
            "postgresql://127.0.0.1/test?host=database.example",
            "postgresql://127.0.0.1/test?hostaddr=192.0.2.1",
            "postgresql://127.0.0.1/test?service=production",
        ):
            with self.assertRaises(RuntimeError):
                driver.validate_database_url(value)

    def test_missing_private_configuration_fails_before_runtime(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(subprocess, "check_output") as execute:
            with self.assertRaises(RuntimeError):
                driver.database_url(None)
            execute.assert_not_called()

    def test_graceful_stop_only_signals_its_fixture(self):
        process = Mock()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            driver.stop_receiver(process, root)
            self.assertTrue((root / "stop").is_file())
            process.wait.assert_called_once_with(timeout=12)
            process.terminate.assert_not_called()
            process.kill.assert_not_called()

    def test_forced_stop_never_claims_database_cleanup(self):
        process = Mock()
        process.wait.side_effect = [subprocess.TimeoutExpired("fixture", 12), None]
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(RuntimeError, "cleanup unconfirmed"):
                driver.stop_receiver(process, Path(temporary))
        process.terminate.assert_called_once()
        process.kill.assert_not_called()


if __name__ == "__main__":
    unittest.main()
