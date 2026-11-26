"""[INPUT]: Linux私有镜像、loop与挂载身份读回。
[OUTPUT]: 验证错设备拒绝、卸载后读回及故障镜像保留。
[POS]: ext4磁盘故障门禁的资源安全回归；不调用真实mount。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import emissary_linux_filesystem as gate


class LinuxFilesystemTests(unittest.TestCase):
    def test_image_size_budget_is_checked_before_creating_a_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for size in (True, 159, 1025, "768"):
                with self.subTest(size=size), self.assertRaises(RuntimeError):
                    gate.create(root, size_mib=size)
                self.assertFalse((root / "volume.dmg").exists())
            with patch.object(gate, "run"):
                gate.create(root, size_mib=768)
            self.assertEqual((root / "volume.dmg").stat().st_size, 768 << 20)
            self.assertEqual((root / "volume.dmg").stat().st_mode & 0o777, 0o600)

    def test_readback_rejects_wrong_source_type_and_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            mount = root / "mount"
            mount.mkdir()
            good = {"source": "/dev/loop42", "fstype": "ext4", "target": str(mount)}
            for field, value in (("source", "/dev/vdb1"), ("fstype", "btrfs"),
                                 ("target", "/tmp/other")):
                with self.subTest(field=field):
                    with patch.object(gate, "owned_loops", return_value=["/dev/loop42"]), patch.object(
                            gate, "mount_info", return_value={**good, field: value}), patch.object(
                            Path, "is_mount", return_value=True):
                        with self.assertRaises(RuntimeError):
                            gate.filesystem_proof(root)
            with patch.object(gate, "owned_loops", return_value=["/dev/loop42"]), patch.object(
                    gate, "mount_info", return_value=good), patch.object(Path, "is_mount", return_value=True):
                self.assertEqual(gate.filesystem_proof(root)["observed_type"], "ext4")

    def test_inventory_matches_only_exact_owned_backing_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            image = root / "volume.dmg"
            image.touch()
            loops = {"loopdevices": [
                {"name": "/dev/loop3", "back-file": str(image) + ".other"},
                {"name": "/dev/loop42", "back-file": str(image)}]}
            with patch.object(gate, "run", return_value=json.dumps(loops).encode()):
                self.assertEqual(gate.owned_loops(root), ["/dev/loop42"])

    def test_wrong_mount_never_unmounts_or_detaches(self):
        root = Path("/tmp/owned-fixture")
        with patch.object(gate, "owned_loops", return_value=["/dev/loop42"]), patch.object(
                gate, "mount_info", return_value={"source": "/dev/vdb1"}), patch.object(gate, "run") as run:
            with self.assertRaises(RuntimeError):
                gate.detach(root)
            run.assert_not_called()

    def test_mount_elsewhere_prevents_loop_detach(self):
        root = Path("/tmp/owned-fixture")
        with patch.object(gate, "owned_loops", return_value=["/dev/loop42"]), patch.object(
                gate, "mount_info", return_value=None), patch.object(Path, "is_mount", return_value=False), patch.object(
                gate, "run", return_value=b"/tmp/another-mount\n") as run:
            with self.assertRaises(RuntimeError):
                gate.detach(root)
            self.assertEqual(run.call_count, 1)
            self.assertEqual(run.call_args.args[0][:2], ["findmnt", "-rn"])

    def test_success_requires_mount_and_loop_absence_readback(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            mount = root / "mount"
            info = {"source": "/dev/loop42", "fstype": "ext4", "target": str(mount)}
            with patch.object(gate, "owned_loops", side_effect=[["/dev/loop42"], ["/dev/loop42"], []]), patch.object(
                    gate, "mount_info", side_effect=[info, None]), patch.object(
                    Path, "is_mount", return_value=False), patch.object(gate, "run", return_value=b"") as run:
                gate.detach(root)
                self.assertIn(["sudo", "-n", "umount", str(mount)], [c.args[0] for c in run.call_args_list])
                self.assertIn(["sudo", "-n", "losetup", "--detach", "/dev/loop42"],
                              [c.args[0] for c in run.call_args_list])

    def test_mount_and_loop_residue_reject_success(self):
        root = Path("/tmp/owned-fixture")
        info = {"source": "/dev/loop42", "fstype": "ext4", "target": str(root / "mount")}
        with patch.object(gate, "owned_loops", return_value=["/dev/loop42"]), patch.object(
                gate, "mount_info", return_value=info), patch.object(gate, "run", return_value=b""):
            with self.assertRaises(RuntimeError):
                gate.detach(root)
