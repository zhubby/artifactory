#!/usr/bin/env python3
"""[INPUT]: 直接 Pod 台账、异步容器状态、模拟服务端点和节点进程。
[OUTPUT]: 验证 Release 构建、失联停机和数据库及 DNS 恢复。
[POS]: 离线执行 cleanup 验收流程，不连接真实集群。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
from contextlib import ExitStack
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import cleanup


class CleanupTests(unittest.TestCase):
    def exercise(self, directory, *, wrong_initial_uid=False, stopped_while_isolated=False,
                 pod_statuses=None, replace_pod_during_startup=False,
                 initial_publish_unready=None, initial_dns_publish_unready=None,
                 fail_while_isolated=False, fail_kubelet_restore=False,
                 fail_service_protection=False):
        state = {'canceled': False, 'isolated': False, 'restored': False,
                 'signals': [], 'signal_targets': [], 'pod_reads': 0,
                 'pool_capacities': [], 'helpers_stopped': False,
                 'services': {('artifact-cleanup', 'artifactory-timescaledb'): initial_publish_unready,
                              ('kube-system', 'kube-dns'): initial_dns_publish_unready},
                 'service_policies': []}
        self.latest_state = state
        life = {'generation': 1, 'calls': 1, 'deadline_ms': 123456}
        execution = {'name': 'pi-pod', 'uid': 'pi-uid'}
        before = {'id': 'planning-run', 'life': life, 'execution': execution}
        pod = {'metadata': {'name': 'pi-pod', 'uid': 'pi-uid'}, 'status': {
            'containerStatuses': [{'name': 'planning-worker', 'containerID': 'containerd://pi-container',
                                   'state': {'running': {'startedAt': '2026-11-19T00:00:00Z'}}}]}}
        ready = lambda: {'metadata': {'name': 'operator-control-plane'}, 'status': {
            'conditions': [{'type': 'Ready', 'status': 'Unknown' if state['isolated'] else 'True'}]}}
        workloads = [{'metadata': {'name': role, 'namespace': 'artifact-cleanup', 'labels': labels},
                      'status': {'podIP': '10.0.0.1', 'containerStatuses': [
                          {'containerID': 'containerd://' + role}]}}
                     for role, labels in [('control-plane', {'app': 'control-plane'}),
                                          ('upstream', {'app': 'upstream'})]]

        def command(args):
            if 'service' in args:
                namespace = args[args.index('-n') + 1]
                action = args.index('get' if 'get' in args else 'patch')
                return service(namespace, args[action:])
            if 'nodes' in args:
                return json.dumps({'items': [ready()]})
            if 'node' in args and 'get' in args:
                return json.dumps(ready())
            return json.dumps({'items': workloads})

        def service(namespace, args):
            args = tuple(args)
            key = (namespace, args[2])
            if args[:2] == ('get', 'service'):
                spec = {} if state['services'][key] is None else {
                    'publishNotReadyAddresses': state['services'][key]}
                return json.dumps({'metadata': {'name': args[2]}, 'spec': spec})
            if args[:2] == ('patch', 'service'):
                state['services'][key] = json.loads(args[-1])['spec']['publishNotReadyAddresses']
                state['service_policies'].append((*key, state['services'][key]))
                if (fail_service_protection and key == ('kube-system', 'kube-dns')
                        and state['services'][key] is True):
                    raise RuntimeError('DNS Service patch result unknown')
                return ''
            raise AssertionError('Unexpected Service call: ' + str(args))

        def kubectl(*args):
            if args[:2] in [('get', 'service'), ('patch', 'service')]:
                return service('artifact-cleanup', args)
            if args[:2] == ('get', 'planningpool'):
                return json.dumps({'spec': {'maxActive': 8}})
            if args[:2] == ('patch', 'planningpool'):
                state['pool_capacities'].append(json.loads(args[-1])['spec']['maxActive'])
                return ''
            if args[:2] == ('get', 'pods'):
                return json.dumps({'items': [] if state['restored'] else workloads})
            if args[:2] == ('get', 'pod'):
                state['pod_reads'] += 1
                current = json.loads(json.dumps(pod))
                if pod_statuses:
                    status = pod_statuses[min(state['pod_reads'] - 1, len(pod_statuses) - 1)]
                    current.pop('status')
                    if status is not None:
                        current['status'] = status
                if replace_pod_during_startup and state['pod_reads'] > 1:
                    current['metadata']['uid'] = 'replacement-uid'
                if state['canceled']:
                    current['metadata']['deletionTimestamp'] = '2026-11-19T00:00:00Z'
                return json.dumps(current)
            if args[:2] == ('get', 'planningrun'):
                status = {'phase': 'canceled' if state['canceled'] else 'running',
                          'execution': None, 'message': None}
                if not state['restored']:
                    status['execution'] = dict(execution, uid='wrong-uid' if wrong_initial_uid else 'pi-uid',
                                               stopped=stopped_while_isolated and state['isolated'])
                    if state['canceled']:
                        status['message'] = 'CleanupPending: waiting for confirmed stop'
                return json.dumps({'status': status})
            raise AssertionError('Unexpected kubectl call: ' + str(args))

        def api(path, _body=None):
            if path.endswith('/plans'):
                return {'runs': [{'id': before['id']}]}
            if path == '/api/v1/projects':
                return {'id': 'project'}
            if path == '/api/v1/projects/project/releases':
                self.assertEqual(_body['targets'], ['app'])
                return {'id': 'release', 'version': _body['version']}
            if path.endswith('/builds'):
                return {'id': 'build'}
            raise AssertionError('Unexpected API call: ' + path)

        def wait(callback, _seconds):
            for _ in range(16):
                value = callback()
                if value:
                    return value
            raise TimeoutError('fixture condition did not converge')

        def docker(_node, *args, data=None, check=True):
            output, code = b'', 0
            if args[:2] == ('systemctl', 'is-active'):
                output, code = (b'inactive', 3) if state['isolated'] else (b'active', 0)
            elif args[:2] == ('systemctl', 'stop'):
                state['isolated'] = True
            elif args[:2] == ('systemctl', 'start'):
                if fail_kubelet_restore:
                    raise RuntimeError('kubelet restart failed')
                state.update(isolated=False, restored=True)
            elif args[:2] == ('crictl', 'inspect'):
                output = json.dumps({'info': {'pid': 200 if args[2] == 'pi-container' else 100},
                    'status': {'id': args[2], 'state': 'CONTAINER_RUNNING',
                               'labels': {'io.kubernetes.pod.uid': 'pi-uid'}}}).encode()
            elif args[0] == 'kill':
                state['signals'].append(args[1])
                state['signal_targets'].append(args[2])
            elif args[0] == 'cat':
                output = b'200 (pi) T 1 1 1'
            elif args[0] == 'test':
                code = 1 if state['restored'] else 0
            elif args[0] == 'curl':
                url = next(json.loads(line[6:]) for line in data.decode().splitlines()
                           if line.startswith('url = '))
                if url.endswith('/cancel'):
                    state['canceled'] = True
                value = {'status': 'canceled' if state['canceled'] else 'running', 'lifecycle': life}
                if url.endswith('/healthz'):
                    value = {'status': 'ok'}
                elif url.endswith('/api/v1/builds/build'):
                    if state['isolated'] and fail_while_isolated:
                        raise RuntimeError('independent API read failed during isolation')
                    if state['isolated'] and any(value is not True for value in state['services'].values()):
                        raise RuntimeError('Database or DNS Service has no ready endpoints on Unknown node')
                    value = {'jobs': []}
                output = json.dumps(value).encode()
            else:
                raise AssertionError('Unexpected Docker call: ' + str(args))
            return subprocess.CompletedProcess(args, code, output, b'')

        helper = SimpleNamespace(terminate=lambda: state.update(helpers_stopped=True))
        acceptance = SimpleNamespace(context='kind-operator', ns='artifact-cleanup', arch='amd64',
            origin='https://control.invalid', tenant_id='tenant', jar=[], token='fixture-token',
            processes=[helper], resume=lambda: None, command=command, kubectl=kubectl, api=api,
            wait=wait, sql=lambda _database, query: '0' if 'count(*)' in query else json.dumps(before))
        acceptance.create_build = cleanup.Acceptance.create_build.__get__(acceptance)
        patches = [patch.object(cleanup, 'ROOT', Path(directory)),
                   patch.object(cleanup, 'Acceptance', return_value=acceptance),
                   patch.object(cleanup, 'docker', side_effect=docker),
                   patch.object(cleanup.subprocess, 'check_output', return_value='operator'),
                   patch.object(cleanup.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1)),
                   patch.object(cleanup.time, 'sleep')]
        with ExitStack() as stack:
            for context in patches:
                stack.enter_context(context)
            cleanup.run(SimpleNamespace(context='kind-operator', namespace='artifact-cleanup',
                                       node_container='operator-control-plane'))
        evidence = json.loads(next(Path(directory).rglob('evidence.json')).read_text())
        return state, evidence

    def test_direct_pod_ledger_survives_isolation_and_clears_after_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            state, evidence = self.exercise(directory)
        self.assertTrue(evidence['passed'])
        self.assertTrue(evidence['pending_identity_preserved'])
        self.assertTrue(evidence['status_cleared_after_cleanup'])
        self.assertEqual(len(evidence['isolated_samples']), 8)
        self.assertEqual(state['signals'], ['-STOP', '-CONT'])
        self.assertEqual(state['pool_capacities'], [1, 8])
        self.assertTrue(state['helpers_stopped'])

    def test_database_and_dns_survive_unknown_node_and_restore_absent_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            state, evidence = self.exercise(directory)
        self.assertTrue(evidence['logical_cancellation_no_device_jobs'])
        self.assertEqual(evidence['isolated_node_ready'], 'Unknown')
        self.assertEqual(state['service_policies'], [
            ('artifact-cleanup', 'artifactory-timescaledb', True),
            ('kube-system', 'kube-dns', True),
            ('kube-system', 'kube-dns', None),
            ('artifact-cleanup', 'artifactory-timescaledb', None)])
        self.assertTrue(all(value is None for value in state['services'].values()))

    def test_services_restore_their_own_policy_after_fault_read_failure(self):
        for original in [False, True]:
            with self.subTest(original=original), tempfile.TemporaryDirectory() as directory:
                with self.assertRaisesRegex(RuntimeError, 'read failed during isolation'):
                    self.exercise(directory, fail_while_isolated=True,
                                  initial_publish_unready=original,
                                  initial_dns_publish_unready=not original)
                self.assertEqual(self.latest_state['services'], {
                    ('artifact-cleanup', 'artifactory-timescaledb'): original,
                    ('kube-system', 'kube-dns'): not original})
                self.assertTrue(self.latest_state['restored'])
                self.assertTrue(self.latest_state['helpers_stopped'])
                self.assertEqual(self.latest_state['pool_capacities'], [1, 8])

    def test_curl_failure_reports_only_exit_and_http_status(self):
        result = subprocess.CompletedProcess([], 22, b'private-response-body',
            b'curl: (22) The requested URL returned error: 503 http://private:password@host')
        with patch.object(cleanup.subprocess, 'run', return_value=result):
            with self.assertRaises(RuntimeError) as raised:
                cleanup.docker('node', 'curl', '--config', '-', data=b'private-headers')
        message = str(raised.exception)
        self.assertIn('exit=22', message)
        self.assertIn('HTTP=503', message)
        for private in ['private', 'password', 'host']:
            self.assertNotIn(private, message)

    def test_services_restore_after_kubelet_recovery_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'kubelet restart failed'):
                self.exercise(directory, fail_kubelet_restore=True)
        self.assertTrue(all(value is None for value in self.latest_state['services'].values()))
        self.assertEqual(self.latest_state['signals'], ['-STOP', '-CONT'])
        self.assertTrue(self.latest_state['helpers_stopped'])
        self.assertEqual(self.latest_state['pool_capacities'], [1, 8])

    def test_partial_service_patch_is_restored_before_fault_injection(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'DNS Service patch result unknown'):
                self.exercise(directory, fail_service_protection=True)
        self.assertTrue(all(value is None for value in self.latest_state['services'].values()))
        self.assertNotIn('-STOP', self.latest_state['signals'])
        self.assertFalse(self.latest_state['isolated'])
        self.assertTrue(self.latest_state['helpers_stopped'])
        self.assertEqual(self.latest_state['pool_capacities'], [1, 8])

    def test_wrong_pod_uid_is_rejected_before_fault_injection(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(TimeoutError):
                self.exercise(directory, wrong_initial_uid=True)
        self.assertEqual(self.latest_state['signals'], [])
        self.assertTrue(self.latest_state['helpers_stopped'])
        self.assertEqual(self.latest_state['pool_capacities'], [1, 8])

    def test_waits_for_pi_container_startup_before_fault_injection(self):
        statuses = [None, {}, {'containerStatuses': []},
                    {'containerStatuses': [{'name': 'planning-worker', 'state': {
                        'waiting': {'reason': 'ContainerCreating'}}}]},
                    {'containerStatuses': [{'name': 'planning-worker', 'state': {'running': {}}}]},
                    {'containerStatuses': [{'name': 'planning-worker', 'containerID': 'containerd://old-container',
                                            'state': {'waiting': {'reason': 'ContainerCreating'}}}]},
                    {'containerStatuses': [
                        {'name': 'sidecar', 'containerID': 'containerd://sidecar',
                         'state': {'running': {}}},
                        {'name': 'planning-worker', 'containerID': 'containerd://pi-container',
                         'state': {'running': {}}}]}]
        with tempfile.TemporaryDirectory() as directory:
            state, evidence = self.exercise(directory, pod_statuses=statuses)
        self.assertTrue(evidence['passed'])
        self.assertEqual(state['signal_targets'], ['200', '200'])

    def test_pod_replacement_during_startup_is_rejected_before_fault_injection(self):
        statuses = [{'containerStatuses': [{'name': 'planning-worker', 'state': {
                        'waiting': {'reason': 'ContainerCreating'}}}]}]
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(AssertionError, 'Pod identity changed'):
                self.exercise(directory, pod_statuses=statuses, replace_pod_during_startup=True)
        self.assertEqual(self.latest_state['signals'], [])
        self.assertTrue(self.latest_state['helpers_stopped'])
        self.assertEqual(self.latest_state['pool_capacities'], [1, 8])

    def test_unstarted_container_times_out_without_fault_injection(self):
        statuses = [{'containerStatuses': [{'name': 'planning-worker', 'state': {
                        'waiting': {'reason': 'ImagePullBackOff'}}}]}]
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(TimeoutError):
                self.exercise(directory, pod_statuses=statuses)
        self.assertEqual(self.latest_state['signals'], [])
        self.assertTrue(self.latest_state['helpers_stopped'])
        self.assertEqual(self.latest_state['pool_capacities'], [1, 8])

    def test_stopped_ledger_cannot_prove_retained_unknown_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(AssertionError, 'discarded the Pod identity'):
                self.exercise(directory, stopped_while_isolated=True)


if __name__ == '__main__':
    unittest.main()
