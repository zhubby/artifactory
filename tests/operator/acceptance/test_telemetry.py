#!/usr/bin/env python3
"""[INPUT]: 遥测验收入口和本机白名单状态夹具。
[OUTPUT]: 验证池准入、超时收尾与诊断字段隔离。
[POS]: 不访问集群；拒绝把凭据写入验收证据。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('telemetry_acceptance', Path(__file__).with_name('telemetry.py'))
TELEMETRY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TELEMETRY)


class TelemetryTests(unittest.TestCase):
    def test_paused_pool_is_rejected_without_changing_capacity(self):
        acceptance = TELEMETRY.module.Acceptance.__new__(TELEMETRY.module.Acceptance)
        calls = []
        acceptance.evidence = {}
        def kubectl(*args):
            calls.append(args)
            return json.dumps({'metadata': {'generation': 3}, 'spec': {'maxActive': 0},
                               'status': {'phase': 'Paused', 'observedGeneration': 3}})
        acceptance.kubectl = kubectl
        with tempfile.TemporaryDirectory() as temporary:
            acceptance.run = Path(temporary)
            with self.assertRaisesRegex(AssertionError, 'admitting AgentPool'):
                TELEMETRY.require_admission(acceptance)
            self.assertTrue(all(call[0] == 'get' for call in calls))
            self.assertEqual(json.loads((acceptance.run / 'evidence.json').read_text())['telemetry_pool']['max_active'], 0)

    def test_build_runtime_and_resources_use_only_white_list_fields(self):
        run_id = '11111111-1111-4111-8111-111111111111'
        detail = {'id': 'build', 'status': 'failed', 'source_manifest_id': 'manifest',
                  'error': 'raw-credential', 'jobs': [{'id': 'job', 'status': 'failed',
                  'attempts': [{'id': 'attempt', 'status': 'failed', 'failure_kind': 'source',
                                'lease_token': 'raw-credential'}]}]}
        resources = [{'kind': kind, 'metadata': {'name': name, 'uid': name, 'annotations': {'secret': 'raw-credential'}},
                      'spec': spec, 'status': {'phase': 'Pending', 'message': 'raw-credential'}} for kind, name, spec in [
            ('SourceTask', 'source', {'taskId': 'build', 'secret': 'raw-credential'}),
            ('AgentRun', 'run', {'runId': run_id, 'leaseToken': 'raw-credential'}),
            ('AgentPool', 'default', {'maxActive': 0}),
            ('SourceTask', 'unrelated', {'taskId': 'other'})]]
        with tempfile.TemporaryDirectory() as temporary:
            acceptance = SimpleNamespace(run=Path(temporary), evidence={},
                api=lambda path: {'runs': [{'id': run_id, 'status': 'queued', 'secret': 'raw-credential'}]},
                sql=lambda *_args: json.dumps({'id': run_id, 'status': 'queued', 'generation': 1, 'secret': 'raw-credential'}),
                kubectl=lambda *_args: json.dumps({'items': resources}))
            TELEMETRY.record_build(acceptance, 'build', detail)
            raw = (acceptance.run / 'evidence.json').read_text()
            snapshot = json.loads((acceptance.run / 'build.json').read_text())
            self.assertNotIn('raw-credential', raw)
            self.assertNotIn('unrelated', raw)
            self.assertEqual(snapshot['jobs'][0]['attempts'][0]['failure_kind'], 'source')
            self.assertEqual(snapshot['current_run']['status'], 'queued')
            self.assertEqual(len(snapshot['resources']), 3)

    def test_timeout_records_last_observation_and_finally_snapshot(self):
        with tempfile.TemporaryDirectory() as temporary:
            reads = []
            fixture = SimpleNamespace(run=Path(temporary), evidence={}, project='project', arch='amd64',
                                      resume=lambda: None, cleanup=lambda: reads.append('cleanup'),
                                      service_request=lambda *_args: None)
            fixture.check = lambda _name, value: self.assertTrue(value)
            fixture.sql = lambda _db, query: '2.32.0' if 'extversion' in query else '0'
            def api(path, body=None):
                if path == '/api/v1/nodes':return [{'id': 'node', 'name': 'operator-native-device', 'online': True}]
                if path.endswith('/telemetry'):
                    return {'host': {'fresh': True}, 'inventory': {'inventory': {
                        'logical_cpus': 1, 'physical_memory_bytes': 1, 'tools': {'git': {'available': True}}, 'collectors': []}}}
                if body is not None:return {'id': 'telemetry-build'}
                if path.endswith('/plans'):return {'runs': []}
                reads.append('build')
                return {'id': 'telemetry-build', 'status': 'planning' if reads.count('build') == 1 else 'queued', 'jobs': []}
            fixture.api = api
            def kubectl(*args):
                if args[:2] == ('get', 'agentpool'):
                    return json.dumps({'metadata': {'generation': 1}, 'spec': {'maxActive': 2},
                                       'status': {'phase': 'Ready', 'observedGeneration': 1}})
                return json.dumps({'items': []})
            fixture.kubectl = kubectl
            def wait(callback, seconds=180):
                value = callback()
                if seconds == 600:
                    self.assertIsNone(value)
                    raise TimeoutError('fixture terminal timeout')
                self.assertTrue(value)
                return value
            fixture.wait = wait
            with patch.object(TELEMETRY.module, 'Acceptance', return_value=fixture):
                with self.assertRaisesRegex(TimeoutError, 'fixture terminal timeout'):
                    TELEMETRY.run(SimpleNamespace(context='fixture', namespace='fixture'))
            self.assertEqual(reads.count('build'), 2)
            self.assertIn('cleanup', reads)
            self.assertEqual(json.loads((fixture.run / 'build.json').read_text())['status'], 'queued')


if __name__ == '__main__':
    unittest.main()
