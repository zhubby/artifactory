#!/usr/bin/env python3
"""[INPUT]: 临时 Git 出口、就绪探针与写入后丢失响应的 Kubernetes 替身。
[OUTPUT]: 验证节点与端口范围，异常时撤销策略和受限探针。
[POS]: 不访问集群的原生验收出口回归。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location('native_acceptance', Path(__file__).with_name('native.py'))
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


class Acceptance:
    ns = 'isolated-native'

    def __init__(self, fail_apply=False):
        self.policy = None
        self.deleted = []
        self.fail_apply = fail_apply

    def apply(self, policy):
        self.policy = policy
        if self.fail_apply:
            raise OSError('response lost after write')

    def kubectl(self, *args):
        self.deleted.append(args)


class NativeGitEgressTests(unittest.TestCase):
    def assert_removed(self, a):
        self.assertEqual(a.deleted, [('delete', 'networkpolicy',
                                     a.policy['metadata']['name'], '--ignore-not-found')])

    def test_limits_egress_to_one_node_and_port_and_restores(self):
        a = Acceptance()
        with NATIVE.native_git_egress(a, '192.0.2.3', 30443):
            self.assertEqual(a.policy['metadata']['namespace'], a.ns)
            spec = a.policy['spec']
            self.assertEqual(spec['policyTypes'], ['Egress'])
            self.assertEqual(spec['podSelector']['matchExpressions'], [{
                'key': 'app.kubernetes.io/name', 'operator': 'In',
                'values': ['artifactory-pi', 'artifactory-source-task'],
            }])
            self.assertEqual(spec['egress'], [{
                'to': [{'ipBlock': {'cidr': '192.0.2.3/32'}}],
                'ports': [{'protocol': 'TCP', 'port': 30443}],
            }])
            self.assertEqual(a.deleted, [])
        self.assert_removed(a)

    def test_task_exception_still_restores_policy(self):
        a = Acceptance()
        with self.assertRaisesRegex(RuntimeError, 'task failed'):
            with NATIVE.native_git_egress(a, '192.0.2.3', 30443):
                raise RuntimeError('task failed')
        self.assert_removed(a)

    def test_apply_response_loss_still_removes_written_policy(self):
        a = Acceptance(fail_apply=True)
        with self.assertRaisesRegex(OSError, 'response lost'):
            with NATIVE.native_git_egress(a, '192.0.2.3', 30443):
                self.fail('Apply failure must prevent the native task')
        self.assert_removed(a)

    def readiness_acceptance(self, directory, fail_wait=False):
        a = Acceptance()
        a.run = Path(directory)
        (a.run / 'pki').mkdir()
        (a.run / 'pki/ca.pem').write_text('public test CA')
        a.wait = Mock(side_effect=[True, TimeoutError('probe deadline')] if fail_wait else [True, True])
        a.kubectl = Mock(side_effect=[json.dumps({'spec': {'image': 'pi-fixture'}}), '']
                         if fail_wait else [json.dumps({'spec': {'image': 'pi-fixture'}}),
                                            json.dumps({'connected': True, 'attempts': 1}), ''])
        return a

    def test_readiness_pod_is_restricted_and_removed_after_success(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(NATIVE.ssl, 'create_default_context'):
            a = self.readiness_acceptance(directory)
            result = NATIVE.native_git_ready(a, 'https://192.0.2.3:30443/repo.git')
            self.assertTrue(result['host_refs'] and result['restricted_pod_refs'])
            self.assertEqual(a.policy['metadata']['labels'],
                             {'app.kubernetes.io/name': 'artifactory-source-task'})
            self.assertFalse(a.policy['spec']['automountServiceAccountToken'])
            self.assertEqual(a.kubectl.call_args.args,
                             ('delete', 'pod', a.policy['metadata']['name'], '--ignore-not-found'))

    def test_readiness_timeout_removes_probe(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(NATIVE.ssl, 'create_default_context'):
            a = self.readiness_acceptance(directory, fail_wait=True)
            with self.assertRaisesRegex(TimeoutError, 'probe deadline'):
                NATIVE.native_git_ready(a, 'https://192.0.2.3:30443/repo.git')
            self.assertEqual(a.kubectl.call_args.args,
                             ('delete', 'pod', a.policy['metadata']['name'], '--ignore-not-found'))


if __name__ == '__main__':
    unittest.main()
