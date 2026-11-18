"""[INPUT]: 合成镜像归档与模拟 kind、Docker 节点身份。
[OUTPUT]: 验证单平台导入和外部容器拒绝，不操作实际 Docker。
[POS]: 保护 1.28 归档导入边界，证明清单不进入解包路径。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('acceptance_prepare', Path(__file__).with_name('prepare.py'))
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class PrepareTests(unittest.TestCase):
    def test_each_owned_node_imports_only_the_selected_platform(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'image.tar'
            archive.write_bytes(b'archive')
            with patch.object(prepare.subprocess, 'check_output', side_effect=[
                'accept-control-plane\naccept-worker\n', 'accept\n', 'accept\n'
            ]), patch.object(prepare.subprocess, 'run') as command:
                prepare.load_kind_image(archive, 'accept', 'linux/amd64')
            self.assertEqual(command.call_count, 2)
            for call, node in zip(command.call_args_list, ['accept-control-plane', 'accept-worker']):
                args = call.args[0]
                self.assertIn(node, args)
                self.assertNotIn('--all-platforms', args)
                self.assertNotIn('--digests', args)
                self.assertEqual(args[args.index('--platform') + 1], 'linux/amd64')
                self.assertEqual(call.kwargs['stdin'].name, str(archive))

    def test_foreign_node_prevents_all_imports(self):
        with patch.object(prepare.subprocess, 'check_output', side_effect=[
            'accept-control-plane\nforeign\n', 'accept\n', 'other\n'
        ]), patch.object(prepare.subprocess, 'run') as command:
            with self.assertRaises(RuntimeError):
                prepare.load_kind_image(Path('unused.tar'), 'accept', 'linux/amd64')
            command.assert_not_called()


if __name__ == '__main__':
    unittest.main()
