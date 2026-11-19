#!/usr/bin/env python3
"""[INPUT]: 直接 Pod 台账、模拟 Kubernetes API 和节点进程。
[OUTPUT]: 验证失联撤权、身份保留、物理停机和测试资源恢复。
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
    def exercise(self, directory, *, wrong_initial_uid=False, stopped_while_isolated=False):
        state = {'canceled': False, 'isolated': False, 'restored': False,
                 'signals': [], 'pool_capacities': [], 'helpers_stopped': False}
        self.latest_state = state
        life = {'generation': 1, 'calls': 1, 'deadline_ms': 123456}
        execution = {'name': 'pi-pod', 'uid': 'pi-uid'}
        before = {'id': 'planning-run', 'life': life, 'execution': execution}
        pod = {'metadata': {'name': 'pi-pod', 'uid': 'pi-uid'}, 'status': {
            'containerStatuses': [{'containerID': 'containerd://pi-container'}]}}
        ready = lambda: {'metadata': {'name': 'operator-control-plane'}, 'status': {
            'conditions': [{'type': 'Ready', 'status': 'Unknown' if state['isolated'] else 'True'}]}}
        workloads = [{'metadata': {'name': role, 'namespace': 'artifact-cleanup', 'labels': labels},
                      'status': {'podIP': '10.0.0.1', 'containerStatuses': [
                          {'containerID': 'containerd://' + role}]}}
                     for role, labels in [('control-plane', {'app': 'control-plane'}),
                                          ('upstream', {'app': 'upstream'})]]

        def command(args):
            if 'nodes' in args:
                return json.dumps({'items': [ready()]})
            if 'node' in args and 'get' in args:
                return json.dumps(ready())
            return json.dumps({'items': workloads})

        def kubectl(*args):
            if args[:2] == ('get', 'agentpool'):
                return json.dumps({'spec': {'maxActive': 8}})
            if args[:2] == ('patch', 'agentpool'):
                state['pool_capacities'].append(json.loads(args[-1])['spec']['maxActive'])
                return ''
            if args[:2] == ('get', 'pods'):
                return json.dumps({'items': [] if state['restored'] else workloads})
            if args[:2] == ('get', 'pod'):
                current = json.loads(json.dumps(pod))
                if state['canceled']:
                    current['metadata']['deletionTimestamp'] = '2026-11-19T00:00:00Z'
                return json.dumps(current)
            if args[:2] == ('get', 'agentrun'):
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
            if path.endswith('/builds'):
                return {'id': 'build'}
            raise AssertionError('Unexpected API call: ' + path)

        def wait(callback, _seconds):
            value = callback()
            if not value:
                raise TimeoutError('fixture condition did not converge')
            return value

        def docker(_node, *args, data=None, check=True):
            output, code = b'', 0
            if args[:2] == ('systemctl', 'is-active'):
                output, code = (b'inactive', 3) if state['isolated'] else (b'active', 0)
            elif args[:2] == ('systemctl', 'stop'):
                state['isolated'] = True
            elif args[:2] == ('systemctl', 'start'):
                state.update(isolated=False, restored=True)
            elif args[:2] == ('crictl', 'inspect'):
                output = json.dumps({'info': {'pid': 200 if args[2] == 'pi-container' else 100},
                    'status': {'id': args[2], 'state': 'CONTAINER_RUNNING',
                               'labels': {'io.kubernetes.pod.uid': 'pi-uid'}}}).encode()
            elif args[0] == 'kill':
                state['signals'].append(args[1])
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

    def test_wrong_pod_uid_is_rejected_before_fault_injection(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(TimeoutError):
                self.exercise(directory, wrong_initial_uid=True)
        self.assertEqual(self.latest_state['signals'], [])
        self.assertTrue(self.latest_state['helpers_stopped'])
        self.assertEqual(self.latest_state['pool_capacities'], [1, 8])

    def test_stopped_ledger_cannot_prove_retained_unknown_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(AssertionError, 'discarded the Pod identity'):
                self.exercise(directory, stopped_while_isolated=True)


if __name__ == '__main__':
    unittest.main()
