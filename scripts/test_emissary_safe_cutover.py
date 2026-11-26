#!/usr/bin/env python3
"""[INPUT]: 切换阶段围栏、临时可执行字节与确切数据库会话。
[OUTPUT]: 验证未停副本及残存会话拒绝进入下一阶段。
[POS]: 切换证据否定门禁；不替代真实普通进程组合。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

from emissary_safe_cutover import attributed, freeze_binaries, require_stopped, require_connections, verify_artifact
import check_emissary_legacy_receiver as legacy


class CutoverBoundaries(unittest.TestCase):
    def test_unreaped_or_failed_replica_forbids_database_transition(self):
        for confirmed,returncode,failed in ((False,None,False),(True,None,False),(True,0,True)):
            child = SimpleNamespace(confirmed=confirmed,process=SimpleNamespace(returncode=returncode),cleanup_failed=failed)
            with self.assertRaisesRegex(RuntimeError,'stop is not confirmed'):
                require_stopped([child],['owned'],lambda *_:self.fail('must reject before SQL'),['db'])

    def test_remaining_owned_database_backend_forbids_transition(self):
        child = SimpleNamespace(confirmed=True,process=SimpleNamespace(returncode=0),cleanup_failed=False)
        with self.assertRaisesRegex(RuntimeError,'database session remains'):
            require_stopped([child],['owned'],lambda *_:'1',['db'])
        seen = []
        require_stopped([child],['owned'],lambda db,query:(seen.append((db,query)) or '0'),['business','telemetry'])
        self.assertEqual([db for db,_ in seen],['business','telemetry'])
        self.assertTrue(all("application_name IN ('owned')" in query for _,query in seen))

    def test_frozen_compatible_artifact_keeps_bytes_in_separate_private_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            names = ('artifactory-control-plane','artifactory-telemetry-service')
            original = {}
            for name in names:
                original[name] = root/name
                original[name].write_bytes(('actual bytes '+name).encode())
            copied = freeze_binaries(original,root/'recovery')
            for name,path in copied.items():
                self.assertNotEqual(path,original[name])
                self.assertEqual(path.read_bytes(),original[name].read_bytes())
                self.assertEqual(path.stat().st_mode & 0o777,0o700)
            original[names[0]].write_bytes(b'new cargo output')
            self.assertNotEqual(copied[names[0]].read_bytes(),original[names[0]].read_bytes())
            with self.assertRaises(FileExistsError):
                freeze_binaries(original,root/'recovery')

    def test_every_rust_database_url_has_unique_application_attribution(self):
        env = {'DATABASE_URL':'postgresql://private@127.0.0.1/db?sslmode=disable',
               'ARTIFACTORY_BUSINESS_DATABASE_URL':'postgresql://private@127.0.0.1/business'}
        value = attributed(env,'owned_replica')
        self.assertTrue(all('application_name=owned_replica' in url for url in value.values()))
        self.assertNotIn('application_name',env['DATABASE_URL'])
        with self.assertRaisesRegex(RuntimeError,'already set'):
            attributed(value,'second_identity')

    def test_missing_actual_database_attribution_cannot_prove_replica(self):
        self.assertFalse(require_connections(lambda *_:'0',['business','telemetry'],'owned'))
        self.assertFalse(require_connections(lambda db,_:'1' if db=='business' else '0',['business','telemetry'],'owned'))
        self.assertTrue(require_connections(lambda *_:'1',['business','telemetry'],'owned'))

    def test_changed_current_or_recovery_executable_forbids_launch(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'executable'
            path.write_bytes(b'original')
            expected=legacy.digest(path)
            verify_artifact(path,expected)
            path.write_bytes(b'replacement')
            with self.assertRaisesRegex(RuntimeError,'executable changed'):
                verify_artifact(path,expected)


if __name__=='__main__':
    unittest.main()
