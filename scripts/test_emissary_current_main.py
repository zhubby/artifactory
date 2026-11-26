#!/usr/bin/env python3
"""[INPUT]: 完整main驱动及带就绪握手的有界测试服务。
[OUTPUT]: 验证公平发送证据、阶段顺序、冻结源码与超时回收。
[POS]: 无数据库驱动回归；不代替真实双库与ACL证据。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
import hashlib
import os
from pathlib import Path
import signal
import sys
import tempfile
import unittest
from unittest.mock import patch

import check_emissary_current_main as driver


TEST = '''#!/usr/bin/env python3
import json, os, pathlib, time
root = pathlib.Path(os.environ["ARTIFACTORY_CURRENT_MAIN_DIR"])
(root / "test.pid").write_text(str(os.getpid()))
for index, phase in enumerate(("missing_startup_grant", "authorized_main", "stop_main")):
    (root / f"request-{index}.json").write_text(json.dumps({"phase":phase}))
    result = root / f"result-{index}.json"
    deadline = time.monotonic() + 8
    while not result.exists() and time.monotonic() < deadline: time.sleep(.01)
    assert json.loads(result.read_text()) == {"phase":phase,"success":True}
    if index == 1:
        while not (root / "main-second.pid").exists() and time.monotonic() < deadline: time.sleep(.01)
        assert (root / "main-second.pid").exists()
'''

SERVICE = '''#!/usr/bin/env python3
import os, pathlib, signal, time
root = pathlib.Path(os.environ["ARTIFACTORY_CURRENT_MAIN_DIR"])
index = 1 if (root / "main-first.pid").exists() else 0
if index == 0:
    (root / "main-first.pid").write_text(str(os.getpid()))
    print(os.environ.get("TEST_REJECTION", "SELECT on node_startups is required"), flush=True)
    raise SystemExit(1)
signal.signal(signal.SIGINT, lambda *_: exit(0))
(root / "main-second.pid").write_text(str(os.getpid()))
time.sleep(15)
'''


class FairnessEvidence(unittest.TestCase):
    def test_rejects_missing_false_and_out_of_budget_measurements(self):
        valid = {field: True for field in ("passed", "actual_current_main", "authorization_acl_proven",
            "history_continuously_backlogged", "original_history_and_inflight_bytes_preserved",
            "sender_stopped", "isolated_delay_removed")}
        valid.update(first_projection_ms=8336, max_observed_fresh_ack_ms=19347,
            max_projection_age_ms=45881, fresh_business_acks=5, historical_business_acks=8,
            current_startup_queries=388)
        driver.validate_fairness({"fairness": valid})
        with self.assertRaises(RuntimeError):
            driver.validate_fairness({})
        for field in valid:
            for bad in (None, False, "1"):
                with self.subTest(field=field, bad=bad), self.assertRaises(RuntimeError):
                    driver.validate_fairness({"fairness": dict(valid, **{field: bad})})
        for field, bad in (("first_projection_ms", 45000), ("max_observed_fresh_ack_ms", 60001),
                           ("max_projection_age_ms", 90000), ("fresh_business_acks", 2),
                           ("historical_business_acks", 2), ("current_startup_queries", 0)):
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                driver.validate_fairness({"fairness": dict(valid, **{field: bad})})


@unittest.skipUnless(os.name == "posix" and hasattr(os, "WNOWAIT"), "POSIX group proof required")
class MainDriver(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="current-main-driver-test-")
        self.root = Path(self.temporary.name)
        self.test = self.root / "acceptance"
        self.service = self.root / "service"
        for path, content in ((self.test, TEST), (self.service, SERVICE)):
            path.write_text(content.replace("#!/usr/bin/env python3", "#!" + sys.executable))
            path.chmod(0o700)
        self.env = dict(os.environ, ARTIFACTORY_CURRENT_MAIN_DIR=str(self.root))
        driver.private_json(self.root / "service-env.json", {"ARTIFACTORY_CURRENT_MAIN_DIR": str(self.root)})
        self.supervisor = driver.Supervisor()
        self.addCleanup(self.temporary.cleanup)
        self.addCleanup(self.supervisor.cleanup)

    def run_driver(self, timeout=8):
        driver.run_delivery(self.supervisor, self.test, self.service, env=self.env,
                            directory=self.root, timeout=timeout)

    def assert_clean(self):
        self.assertTrue(self.supervisor.cleanup_confirmed())
        for child in self.supervisor.children:
            with self.assertRaises(ProcessLookupError):
                os.kill(child.process.pid, 0)

    def test_all_phases_require_expected_rejection_and_graceful_shutdown(self):
        self.run_driver()
        self.assertTrue((self.root / "result-2.json").exists())
        self.assert_clean()

    def test_unexpected_startup_failure_is_not_accepted(self):
        driver.private_json(self.root / "service-env.json", {"ARTIFACTORY_CURRENT_MAIN_DIR": str(self.root), "TEST_REJECTION": "unrelated failure"})
        with self.assertRaisesRegex(RuntimeError, "unexpected startup reason"):
            self.run_driver()
        self.assertFalse((self.root / "result-0.json").exists())
        self.assert_clean()

    def test_outer_timeout_reaps_test_and_running_main(self):
        # 第二阶段后保持测试等待，不发布正常停止请求。
        self.test.write_text(("#!" + sys.executable + "\n" + TEST.split("\n", 1)[1]).replace(
            '    (root / f"request-{index}.json").write_text',
            '    if index == 2: time.sleep(15)\n    (root / f"request-{index}.json").write_text'))
        monotonic = driver.time.monotonic

        def after_main_started():
            elapsed = 20 if (self.root / "main-second.pid").exists() else 0
            return monotonic() + elapsed

        with patch.object(driver.time, "monotonic", side_effect=after_main_started):
            with self.assertRaisesRegex(RuntimeError, "outer deadline"):
                self.run_driver(timeout=10)
        self.assertTrue((self.root / "main-second.pid").exists())
        self.assert_clean()

    def prepare_legacy(self, *, pause=False):
        code = '''    if index == 2:
        frozen = root / "frozen"
        frozen.mkdir()
        for sequence in range(3):
            name = "prepare_frozen_source" if sequence == 0 else "frozen_sender_to_current_receiver"
            (frozen / f"request-{sequence}.json").write_text(json.dumps({"sequence":sequence,"test":name}))
            result = frozen / f"result-{sequence}.json"
            while not result.exists(): time.sleep(.01)
            assert json.loads(result.read_text()) == {"sequence":sequence,"success":True,"process_reaped":True}
'''
        self.test.write_text(TEST.replace("#!/usr/bin/env python3", "#!" + sys.executable).replace(
            '    (root / f"request-{index}.json").write_text', code + '    (root / f"request-{index}.json").write_text'))
        sender = self.root / "sender"
        sender.write_text("#!" + sys.executable + '''
import json,os,pathlib,time
root=pathlib.Path(os.environ["ARTIFACTORY_LEGACY_SENDER_DIR"])
assert not any(key in os.environ for key in ("DATABASE_URL","ARTIFACTORY_TEST_DATABASE_URL","ARTIFACTORY_BUSINESS_DATABASE_URL"))
pending=[i for i in range(3) if (root/f"request-{i}.json").exists() and not (root/f"result-{i}.json").exists()]
assert len(pending)==1
(root/f"sender-{pending[0]}.pid").write_text(str(os.getpid()))
''' + ("time.sleep(15)\n" if pause else ""))
        sender.chmod(0o700)
        return sender

    def test_three_frozen_invocations_are_directly_reaped_without_credentials(self):
        sender = self.prepare_legacy()
        with patch.dict(os.environ, {"ARTIFACTORY_TEST_DATABASE_URL":"private-test-value"}):
            driver.run_delivery(self.supervisor,self.test,self.service,env=self.env,
                                directory=self.root,sender=sender,timeout=10)
        for index in range(3):
            self.assertTrue((self.root / f"frozen/result-{index}.json").exists())
        self.assert_clean()

    def test_outer_timeout_reaps_active_frozen_sender_and_main(self):
        sender = self.prepare_legacy(pause=True)
        monotonic = driver.time.monotonic

        def after_sender_started():
            return monotonic() + (30 if (self.root / "frozen/sender-0.pid").exists() else 0)

        with patch.object(driver.time,"monotonic",side_effect=after_sender_started):
            with self.assertRaisesRegex(RuntimeError,"outer deadline"):
                driver.run_delivery(self.supervisor,self.test,self.service,env=self.env,
                                    directory=self.root,sender=sender,timeout=10)
        self.assertTrue((self.root / "frozen/sender-0.pid").exists())
        self.assert_clean()

    def test_frozen_product_file_change_is_rejected(self):
        old = self.root / "old"
        old.mkdir()
        (old / "source").write_text("original")
        driver.private_json(self.root / "frozen-manifest.json", {"source":hashlib.sha256(b"original").hexdigest()})
        self.assertEqual(len(driver.verify_frozen_tree(self.root)),64)
        (old / "source").write_text("changed")
        with self.assertRaisesRegex(RuntimeError,"frozen product source changed"):
            driver.verify_frozen_tree(self.root)


if __name__ == "__main__":
    unittest.main()
