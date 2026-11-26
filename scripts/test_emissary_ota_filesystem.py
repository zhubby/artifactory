"""[INPUT]: 私有磁盘镜像归属与故障证据校验入口。
[OUTPUT]: 验证恢复证据与错身份拒绝，保留变化读回及未确认清理镜像。
[POS]: 文件系统门禁的资源安全回归；普通平台均可执行。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

import check_emissary_ota_filesystem as gate


class FilesystemGateTests(unittest.TestCase):
    def setUp(self):
        # 镜像归属与卸载用例明确验证macOS后端。
        self.platform = patch.object(gate.sys, "platform", "darwin")
        self.platform.start()
        self.addCleanup(self.platform.stop)

    def test_supervisor_requires_real_enospc_and_recovered_state(self):
        fields = ("one_byte_write_enospc", "real_supervisor",
                  "download_failed_after_first_persisted_byte", "current_runtime_kept_running",
                  "pending_state_protection_checked",
                  "failed_state_saved_after_space_release", "protection_released_after_recovery",
                  "target_not_installed", "original_asset_and_source_preserved", "normal_stop_confirmed")
        observed = {**{key: True for key in fields}, "free_bytes_at_failure": 5 << 20,
                    "state_write_probe_enospc": False, "terminal_saved_while_full": True}
        proof = {"passed": True, "nonce": "owned", "case": "supervisor_enospc",
                 "native_service_executed": False, "supervisor": observed}
        gate.validate_proof(proof, "owned", "supervisor_enospc")
        for key in fields:
            for replacement in (None, False, 1, "true"):
                with self.subTest(key=key, replacement=replacement):
                    with self.assertRaises(RuntimeError):
                        gate.validate_proof({**proof, "supervisor": {**observed, key: replacement}},
                                            "owned", "supervisor_enospc")
        for value in (None, True, -1, (200 << 20) + 1):
            with self.assertRaises(RuntimeError):
                gate.validate_proof({**proof, "supervisor": {**observed, "free_bytes_at_failure": value}},
                                    "owned", "supervisor_enospc")
        for key in ("state_write_probe_enospc", "terminal_saved_while_full"):
            for value in (None, 1, "true"):
                with self.assertRaises(RuntimeError):
                    gate.validate_proof({**proof, "supervisor": {**observed, key: value}},
                                        "owned", "supervisor_enospc")

    def proof(self, case):
        proof = {"passed": True, "nonce": "owned", "case": case,
                "actual_mounted_filesystem": True, "state_and_protected_assets_preserved": True,
                "unconfirmed_data_sentinel_preserved": True, "target_not_installed": True,
                "partial_files_removed": True, "native_service_executed": False,
                "actual_enospc": case == "after_precheck"}
        if case == "after_precheck":
            proof["terminal_progress_persisted"] = True
        return proof

    def test_missing_false_or_wrong_type_proofs_are_rejected(self):
        proof = self.proof("after_precheck")
        gate.validate_proof(proof, "owned", "after_precheck")
        for key in proof:
            for replacement in (None, False, 1, "true"):
                if replacement is proof[key]:
                    continue
                if key == "terminal_progress_persisted" and type(replacement) is bool:
                    continue
                changed = {**proof, key: replacement}
                with self.subTest(key=key, replacement=replacement):
                    with self.assertRaises(RuntimeError):
                        gate.validate_proof(changed, "owned", "after_precheck")
        for key in proof:
            missing = proof.copy()
            del missing[key]
            with self.assertRaises(RuntimeError):
                gate.validate_proof(missing, "owned", "after_precheck")
        gate.validate_proof({**proof, "terminal_progress_persisted": False}, "owned", "after_precheck")

    def test_prepare_and_case_identity_are_distinct(self):
        gate.validate_proof({"passed": True, "nonce": "owned"}, "owned", "prepare_readonly")
        with self.assertRaises(RuntimeError):
            gate.validate_proof(self.proof("low_space"), "owned", "readonly")

    def test_image_inventory_matches_exact_path_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            own = {"image-path": str(root / "volume.dmg")}
            other = {"image-path": str(root / "volume.dmg.other")}
            with patch.object(gate, "command", return_value=plistlib.dumps({"images": [other, own]})):
                self.assertEqual(gate.owned_images(root), [own])

    def test_filesystem_readback_requires_type_and_mount_identity(self):
        root = Path("/tmp/owned-fixture")
        for expected, observed in (("HFS+", "hfs"), ("APFS", "apfs")):
            info = {"FilesystemType": observed, "MountPoint": str(root / "mount")}
            with patch.object(gate, "command", return_value=plistlib.dumps(info)), patch.object(Path, "is_mount", return_value=True):
                self.assertEqual(gate.filesystem_proof(root, expected),
                                 {"requested": expected, "observed_type": observed, "mount_verified": True})
            for key, value in (("FilesystemType", "ext4"), ("MountPoint", "/tmp/another-mount")):
                with self.subTest(expected=expected, key=key, value=value):
                    with patch.object(gate, "command", return_value=plistlib.dumps({**info, key: value})), patch.object(Path, "is_mount", return_value=True):
                        with self.assertRaises(RuntimeError):
                            gate.filesystem_proof(root, expected)
            for key in info:
                with patch.object(gate, "command", return_value=plistlib.dumps({k: v for k, v in info.items() if k != key})), patch.object(Path, "is_mount", return_value=True):
                    with self.assertRaises(RuntimeError):
                        gate.filesystem_proof(root, expected)
            with patch.object(gate, "command", return_value=plistlib.dumps(info)), patch.object(Path, "is_mount", return_value=False):
                with self.assertRaises(RuntimeError):
                    gate.filesystem_proof(root, expected)

    def test_detach_refuses_ambiguous_inventory(self):
        root = Path("/tmp/owned-fixture")
        with patch.object(gate, "owned_images", return_value=[{}, {}]), patch.object(gate, "command") as run:
            with self.assertRaises(RuntimeError):
                gate.detach(root)
            run.assert_not_called()

    def test_detach_requires_absence_readback(self):
        root = Path("/tmp/owned-fixture")
        image = {"system-entities": [{"dev-entry": "/dev/disk999"}]}
        with patch.object(gate, "owned_images", side_effect=[[image], [image]]), patch.object(gate, "command") as run:
            with self.assertRaises(RuntimeError):
                gate.detach(root)
            run.assert_called_once_with(["hdiutil", "detach", "/dev/disk999"])

    def test_detach_retries_busy_only_after_exact_identity_readback(self):
        root = Path("/tmp/owned-fixture")
        image = {"system-entities": [{"dev-entry": "/dev/disk999"}]}
        busy = subprocess.CalledProcessError(16, ["hdiutil", "detach", "/dev/disk999"])
        with patch.object(gate, "owned_images", side_effect=[[image], [image], []]), patch.object(gate, "command", side_effect=[busy, b""]) as run, patch.object(gate.time, "sleep"), patch.object(Path, "is_mount", return_value=False):
            gate.detach(root)
            self.assertEqual(run.call_count, 2)
        with patch.object(gate, "owned_images", side_effect=[[image], []]), patch.object(gate, "command", side_effect=busy) as run, patch.object(gate.time, "sleep"), patch.object(Path, "is_mount", return_value=True), patch.object(gate, "private_json") as save:
            with self.assertRaisesRegex(RuntimeError, "identity changed"):
                gate.detach(root)
            run.assert_called_once()
            save.assert_called_once_with(root / "detach-identity.json", {"before": [image], "after": []})

    def test_failed_process_cleanup_preserves_image_and_skips_detach(self):
        for raises in (False, True):
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                image = root / "volume.dmg"
                image.write_bytes(b"recovery")
                supervisor = Mock()
                supervisor.cleanup_confirmed.return_value = False
                if raises:
                    supervisor.cleanup.side_effect = RuntimeError("cleanup unconfirmed")
                evidence = {}
                with patch.object(gate, "detach") as detach:
                    failures = gate.cleanup_owned(supervisor, root, evidence)
                self.assertTrue(failures)
                self.assertFalse(evidence["process_cleanup_confirmed"])
                self.assertTrue(evidence["recovery_image_retained"])
                self.assertEqual(image.read_bytes(), b"recovery")
                detach.assert_not_called()

    def test_failed_detach_preserves_recovery_image(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "volume.dmg"
            image.write_bytes(b"recovery")
            supervisor = Mock()
            supervisor.cleanup_confirmed.return_value = True
            evidence = {}
            with patch.object(gate, "detach", side_effect=RuntimeError("still attached")):
                self.assertEqual(gate.cleanup_owned(supervisor, root, evidence), ["owned_image"])
            self.assertTrue(evidence["recovery_image_retained"])
            self.assertTrue(image.exists())

    def test_spool_recovery_requires_each_real_transaction_proof(self):
        proof = {"passed": True, "case": "all", "source_batches": 2, "source_inventories": 3,
                 "queue_records": self.queues(),
                 "native_service_executed": False, "authorization_acl_proven": False,
                 **{key: True for key in ("real_business_migrations", "transaction_fence_observed",
                     "fresh_pinned_by_real_sender", "source_bytes_retained_before_commit",
                     "business_ack_removed_sources", "original_ids_digests_times_verified",
                     "terminal_operation_unchanged", "identity_configuration_unchanged", "database_removed")}}
        gate.validate_recovery(proof, "all")
        for key, original in proof.items():
            for replacement in (None, False, True, 1, "true"):
                if type(original) is type(replacement) and original == replacement:
                    continue
                with self.subTest(key=key, replacement=replacement):
                    with self.assertRaises(RuntimeError):
                        gate.validate_recovery({**proof, key: replacement}, "all")
            missing = proof.copy()
            del missing[key]
            with self.assertRaises(RuntimeError):
                gate.validate_recovery(missing, "all")

    def queues(self):
        return [{"category": category, "count": 1, "records": [{"bytes": 256, "sha256": "a" * 64}]}
                for category in ("history_batch", "ordinary_batch", "legacy_inventory",
                                 "startup_inventory", "refresh_inventory", "terminal_operation")]

    def test_queue_matrix_rejects_missing_duplicate_and_invalid_digests(self):
        records = self.queues()
        gate.validate_queues(records)
        invalid = [None, [], records[:-1], [records[0]] * 6]
        for key, value in (("count", True), ("count", 2), ("records", []),
                           ("category", "unexpected")):
            invalid.append([{**records[0], key: value}, *records[1:]])
        for key, value in (("bytes", True), ("bytes", 0), ("bytes", (1 << 20) + 1),
                           ("sha256", "z" * 64), ("sha256", "a" * 63)):
            invalid.append([{**records[0], "records": [{**records[0]["records"][0], key: value}]}, *records[1:]])
        for changed in invalid:
            with self.assertRaises(RuntimeError):
                gate.validate_queues(changed)

    def test_database_cleanup_failure_preserves_image_and_skips_detach(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "volume.dmg"
            image.write_bytes(b"original recovery volume")
            supervisor = Mock()
            supervisor.cleanup_confirmed.return_value = True
            cleanup_database = Mock(side_effect=RuntimeError("database readback failed"))
            evidence = {}
            with patch.object(gate, "detach") as detach:
                failures = gate.cleanup_owned(supervisor, root, evidence,
                                               database_cleanup=cleanup_database)
            self.assertEqual(failures, ["owned_database_cleanup_unconfirmed"])
            cleanup_database.assert_called_once_with()
            detach.assert_not_called()
            self.assertTrue(evidence["recovery_image_retained"])
            self.assertEqual(image.read_bytes(), b"original recovery volume")

    def test_active_collection_requires_real_failure_and_recovery_proofs(self):
        proof = {"passed": True, "actual_enospc": True, "one_byte_write_enospc": True,
                 "free_bytes_at_failure": 0, "filesystem_total_bytes": 160 << 20,
                 "observed_disk_errors": 2, "repeated_collection_failures_observed": True,
                 "original_sources_retained": True, "same_handle_resumed_after_space_release": True,
                 "disk_counters_survive_reopen": True, "resumed_sample_business_ack_verified": True,
                 "resumed_sample_source_removed": True,
                 "resumed_batch_id": "88d9730d-11ee-4856-a3bd-467cb5130c97", "native_service_executed": False}
        gate.validate_active_collection(proof)
        for key, original in proof.items():
            for replacement in (None, False, True, 1, "true"):
                if type(original) is type(replacement) and original == replacement:
                    continue
                if key == "free_bytes_at_failure" and type(replacement) is int and 0 <= replacement <= proof["filesystem_total_bytes"]:
                    continue
                with self.subTest(key=key, replacement=replacement):
                    with self.assertRaises(RuntimeError):
                        gate.validate_active_collection({**proof, key: replacement})
            missing = proof.copy()
            del missing[key]
            with self.assertRaises(RuntimeError):
                gate.validate_active_collection(missing)
        gate.validate_active_collection({**proof, "free_bytes_at_failure": 1})
        gate.validate_active_collection({**proof, "free_bytes_at_failure": 5 << 20})
        for free in (-1, proof["filesystem_total_bytes"] + 1):
            with self.assertRaises(RuntimeError):
                gate.validate_active_collection({**proof, "free_bytes_at_failure": free})
        for total in ((100 << 20) - 1, (200 << 20) + 1):
            with self.assertRaises(RuntimeError):
                gate.validate_active_collection({**proof, "filesystem_total_bytes": total})


if __name__ == "__main__":
    unittest.main()
