"""[INPUT]: 专用 kind 目标与 Runtime 替身契约。
[OUTPUT]: 验证版本下限、故障围栏和原始执行身份。
[POS]: 生命周期 harness 的离线安全与身份测试。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import unittest
from pathlib import Path
import tempfile
from unittest.mock import Mock
from run import Harness, kind_target, namespace_name, require_kubernetes_minimum
from runtime import Runtime


class HarnessTests(unittest.TestCase):
    def test_kubernetes_minimum_accepts_newer_releases_and_rejects_older_versions(self):
        for version in ('v1.28.0', 'v1.28.13', 'v1.29.0', 'v1.33.2', 'v1.37.0',
                        'v1.40.1', 'v1.31.3+k3s1', 'v1.28.9-gke.1'):
            with self.subTest(version=version):
                require_kubernetes_minimum(version)
        for version in ('v1.27.99', 'v1.9.0', 'v0.28.0', 'v1.28', 'v1.28.0-invalid!', 'unknown'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                require_kubernetes_minimum(version)

    def test_namespace_rejects_existing_system_names_and_ownership_changes(self):
        for name in ('default', 'kube-system', 'fixture', 'artifactory-pod-lifecycle-', 'artifactory-pod-lifecycle-A'):
            with self.assertRaises(ValueError):
                namespace_name(name)
        self.assertEqual(namespace_name('artifactory-pod-lifecycle-fixture'), 'artifactory-pod-lifecycle-fixture')
        harness = object.__new__(Harness)
        harness.ns, harness.namespace_uid, harness.namespace_nonce = 'artifactory-pod-lifecycle-fixture', 'created', 'nonce'
        harness.get = Mock(return_value={'metadata': {'uid': 'replacement', 'labels': {'tests.artifactory.dev/lifecycle': 'nonce'}}})
        harness.kubectl = Mock()
        with self.assertRaises(ValueError):
            harness.cleanup_namespace()
        harness.kubectl.assert_not_called()

    def test_failed_node_restoration_still_stops_helpers_and_writes_evidence(self):
        harness = object.__new__(Harness)
        harness.restore_node = Mock(side_effect=RuntimeError('node unavailable'))
        harness.runtime = Mock()
        harness.runtime.snapshot.return_value = []
        harness.processes = {'agent': Mock(), 'image': Mock()}
        harness.stop = Mock(side_effect=[RuntimeError('agent stop failed'), None])
        harness.logs = [Mock()]
        harness.node, harness.frozen_pid, harness.kubelet_stopped = 'fixture-control-plane', 42, True
        harness.evidence = {}
        with tempfile.TemporaryDirectory() as folder:
            harness.out = Path(folder)
            with self.assertRaises(RuntimeError):
                harness.finish()
            self.assertIn('node unavailable', (harness.out / 'evidence.json').read_text())
            self.assertEqual(harness.stop.call_count, 2)
            harness.runtime.close.assert_called_once()
            harness.logs[0].close.assert_called_once()
            self.assertEqual(harness.evidence['recovery_identity']['frozen_pid'], 42)

    def test_resume_failure_keeps_pid_and_still_restores_kubelet(self):
        harness = object.__new__(Harness)
        harness.node, harness.frozen_pid, harness.kubelet_stopped = 'fixture-control-plane', 42, True
        harness.docker = Mock(side_effect=[RuntimeError('resume failed'), ''])
        harness.kubectl = Mock(return_value='')
        with self.assertRaises(RuntimeError):
            harness.restore_node()
        self.assertEqual(harness.frozen_pid, 42)
        self.assertFalse(harness.kubelet_stopped)
        harness.docker.assert_any_call('systemctl', 'start', 'kubelet')

    def test_kind_fault_requires_context_label_node_and_exclusive_workloads(self):
        node = {'metadata': {'name': 'fixture-control-plane'}}
        pod = {'metadata': {'namespace': 'lifecycle'}}
        kind_target('kind-fixture', 'fixture', [node], 'fixture', [pod], 'lifecycle')
        cases = [
            ('production', 'fixture', [node], 'fixture', [pod], 'lifecycle'),
            ('kind-fixture', 'fixture', [node], 'other', [pod], 'lifecycle'),
            ('kind-fixture', 'fixture', [node, node], 'fixture', [pod], 'lifecycle'),
            ('kind-fixture', 'fixture', [node], 'fixture', [{'metadata': {'namespace': 'production'}}], 'lifecycle'),
        ]
        for args in cases:
            with self.assertRaises(ValueError):
                kind_target(*args)

    def test_failure_requires_registered_execution_identity(self):
        runtime = Runtime()
        try:
            runtime.add('run')
            execution = {'run_id': 'run', 'generation': 1, 'uid': 'pod-uid', 'name': 'pod', 'namespace': 'test'}
            runtime.request('/v1/register', execution)
            with self.assertRaises(ValueError):
                runtime.request('/v1/execution-failed', {'execution': dict(execution, uid='replacement')})
            response = runtime.request('/v1/execution-failed', {'execution': execution, 'reason': {'kind': 'pod_failed'}})
            self.assertEqual(response, {'status': 'failed', 'pending': False})
            self.assertEqual(runtime.snapshot()[-1]['body']['execution'], execution)
        finally:
            runtime.close()

    def test_runtime_fences_generation_and_preserves_terminal_or_pending(self):
        runtime = Runtime()
        try:
            runtime.add('run')
            execution = {'run_id': 'run', 'generation': 1, 'uid': 'pod', 'name': 'pod', 'namespace': 'fixture'}
            with self.assertRaises(ValueError):
                runtime.request('/v1/register', dict(execution, generation=999))
            with self.assertRaises(ValueError):
                runtime.request('/v1/register', {'run_id': 'run', 'generation': 1})
            runtime.request('/v1/register', execution)
            runtime.runs['run']['status'] = 'running'
            runtime.runs['run']['lifecycle']['pending'] = True
            response = runtime.request('/v1/execution-failed', {'execution': execution, 'reason': {'kind': 'worker_exit', 'exit_code': 23}})
            self.assertEqual(response['status'], 'running')
            self.assertTrue(response['pending'])
            runtime.cancel('run')
            response = runtime.request('/v1/execution-lost', {'run_id': 'run', 'generation': 1,
                                        'execution': execution, 'reason': 'resource_deleted'})
            self.assertEqual(response['status'], 'canceled')
            self.assertFalse(any(e['path'] == '/v1/execution-failed' for e in runtime.snapshot()))
        finally:
            runtime.close()

    def test_runtime_retries_missing_pod_and_fails_deleted_resource(self):
        runtime = Runtime()
        try:
            runtime.add('run')
            execution = {'run_id': 'run', 'generation': 1, 'uid': 'pod', 'name': 'pod', 'namespace': 'fixture'}
            runtime.request('/v1/register', execution)
            response = runtime.request('/v1/execution-lost', {'run_id': 'run', 'generation': 1,
                                        'execution': execution, 'reason': 'pod_missing'})
            self.assertEqual(response, {'status': 'queued', 'pending': False, 'generation': 2})
            self.assertNotIn('execution', runtime.runs['run'])
            with self.assertRaises(ValueError):
                runtime.request('/v1/register', execution)
            with self.assertRaises(ValueError):
                runtime.request('/v1/execution-failed', {'execution': execution, 'reason': {'kind': 'pod_failed'}})
            runtime.runs['run']['status'] = 'provisioning'
            response = runtime.request('/v1/execution-lost', {'run_id': 'run', 'generation': 2,
                                        'execution': None, 'reason': 'resource_deleted'})
            self.assertEqual(response['status'], 'failed')
        finally:
            runtime.close()


if __name__ == '__main__':
    unittest.main()
