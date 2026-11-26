"""[INPUT]: 原生磁盘阶段与事务回执的有限证据。
[OUTPUT]: 拒绝缺失服务连续性、恢复样本ACK及清理证明。
[POS]: Linux原生磁盘门禁的反例回归，不安装服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import copy
import unittest
import uuid

import check_emissary_native_disk as gate


class NativeDiskEvidenceTests(unittest.TestCase):
    def proof(self):
        value = {key: True for key in (
            "passed", "native_service_executed", "real_business_migrations",
            "transaction_fence_observed", "source_bytes_retained_before_commit",
            "business_ack_removed_sources", "terminal_operation_unchanged",
            "service_account_directory_removed", "database_removed",
            "resumed_sample_business_ack_verified")}
        value.update(source_batches=2, source_inventories=3, native_startups_registered=5,
                     resumed_sample_batch_id=str(uuid.uuid4()))
        value["native_disk_fault"] = {key: True for key in (
            "passed", "native_service_executed", "actual_enospc", "one_byte_write_enospc",
            "repeated_collection_failures_observed", "service_pid_unchanged",
            "runtime_session_unchanged", "launcher_instance_unchanged",
            "identity_and_permissions_unchanged", "original_sources_retained",
            "same_service_resumed_after_space_release", "health_confirmation_resumed")}
        value["native_disk_fault"]["observed_disk_errors"] = 2
        return value

    def test_missing_transaction_or_cleanup_proof_blocks_success(self):
        proof = self.proof()
        gate.validate_proof(proof)
        for key in ("transaction_fence_observed", "business_ack_removed_sources",
                    "resumed_sample_business_ack_verified", "database_removed",
                    "service_account_directory_removed"):
            broken = copy.deepcopy(proof)
            broken[key] = False
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                gate.validate_proof(broken)

    def test_ordinary_process_or_changed_session_cannot_pass(self):
        for key in ("native_service_executed", "actual_enospc", "service_pid_unchanged",
                    "runtime_session_unchanged", "original_sources_retained", "health_confirmation_resumed"):
            proof = self.proof()
            proof["native_disk_fault"][key] = False
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                gate.validate_proof(proof)
        for count in (None, True, 0, 1):
            proof = self.proof()
            proof["native_disk_fault"]["observed_disk_errors"] = count
            with self.subTest(count=count), self.assertRaises(RuntimeError):
                gate.validate_proof(proof)


if __name__ == "__main__":
    unittest.main()
