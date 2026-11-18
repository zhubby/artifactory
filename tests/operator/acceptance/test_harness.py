#!/usr/bin/env python3
"""[INPUT]: 验收清单渲染器、本机 HTTP 服务和工作区 Dockerfile。
[OUTPUT]: 验证会话分页与回执收敛、设备隔离和独立镜像。
[POS]: 只执行本机检查，不访问 Docker 或 Kubernetes。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
from contextlib import redirect_stdout
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
from pathlib import Path
import shlex
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location('operator_acceptance', Path(__file__).with_name('run.py'))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
NATIVE_SPEC = importlib.util.spec_from_file_location('native_acceptance', Path(__file__).with_name('native.py'))
NATIVE = importlib.util.module_from_spec(NATIVE_SPEC)
NATIVE_SPEC.loader.exec_module(NATIVE)


class DeviceFixture:
    def __init__(self):
        self.nodes = {name: {'id': name, 'os': 'linux', 'arch': 'amd64',
            'draining': draining, 'revoked': revoked, 'slots': 2,
            'labels': {'fixture': name}, 'signing_profiles': [], 'pool_id': 'fixture-pool'}
            for name, draining, revoked in [('old-device', False, False),
                                          ('already-drained', True, False),
                                          ('revoked-device', False, True)]}
        self.updates = []
        self.fail_drain = None

    def api(self, path, body=None, method=None):
        if path.startswith('/api/v1/nodes?'):
            return json.loads(json.dumps(list(self.nodes.values())))
        node_id = path.removeprefix('/api/v1/nodes/')
        if method != 'PATCH' or node_id not in self.nodes:
            raise AssertionError('unexpected fixture request: ' + path)
        self.updates.append((node_id, dict(body)))
        self.nodes[node_id].update(body)
        if node_id == self.fail_drain and body['draining']:
            raise RuntimeError('fixture response lost after draining')
        return self.nodes[node_id]


class NativeDeviceIsolationTests(unittest.TestCase):
    def test_same_platform_host_is_the_only_available_device_and_original_flags_return(self):
        fixture = DeviceFixture()
        original = json.loads(json.dumps(fixture.nodes))
        with NATIVE.drain_existing_devices(fixture):
            fixture.nodes['host-device'] = dict(original['old-device'], id='host-device')
            available = [node['id'] for node in fixture.nodes.values()
                         if not node['revoked'] and not node['draining']
                         and (node['os'], node['arch']) == ('linux', 'amd64')]
            self.assertEqual(available, ['host-device'])
        for node_id, node in original.items():
            self.assertEqual(fixture.nodes[node_id], node)
        self.assertEqual([node_id for node_id, _ in fixture.updates], ['old-device', 'old-device'])

    def test_partial_drain_failure_restores_every_modified_device(self):
        fixture = DeviceFixture()
        fixture.nodes['second-device'] = dict(fixture.nodes['old-device'], id='second-device')
        original = json.loads(json.dumps(fixture.nodes))
        fixture.fail_drain = 'second-device'
        with self.assertRaisesRegex(RuntimeError, 'response lost'):
            NATIVE.drain_existing_devices(fixture)
        self.assertEqual(fixture.nodes, original)

    def test_failed_host_build_records_only_safe_diagnostic_fields(self):
        detail = {'id': 'failed-build', 'status': 'failed',
                  'error': 'fetch https://user:password@source.invalid/file?ticket=raw-ticket Bearer raw-auth',
                  'source_credential': 'must-not-appear',
                  'jobs': [{'target': 'app', 'status': 'failed', 'target_config': {'secret': 'must-not-appear'},
                            'attempts': [{'id': 'failed-attempt', 'status': 'failed',
                                          'node_id': 'old-device', 'node_name': 'cluster device',
                                          'failure_kind': 'source', 'error': 'checkout failed',
                                          'lease_token': 'must-not-appear'}]}]}
        evidence = {'build_id': 'failed-build', 'node_id': 'host-device', 'passed': False}
        fixture = SimpleNamespace(api=lambda _path: detail)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            with self.assertRaisesRegex(AssertionError, 'checkout failed') as failure:
                NATIVE.host_build_terminal(fixture, 'failed-build', directory, evidence)
            projection = json.loads((directory / 'build.json').read_text())
            recorded = (directory / 'evidence.json').read_text()
            combined = json.dumps(projection) + recorded + str(failure.exception)
            for secret in ('must-not-appear', 'password', 'raw-ticket', 'raw-auth', 'source.invalid',
                           'lease_token', 'target_config', 'source_credential'):
                self.assertNotIn(secret, combined)
            self.assertEqual(projection['jobs'][0]['attempts'][0]['node_id'], 'old-device')
            self.assertEqual(json.loads(recorded)['node_id'], 'host-device')
            self.assertEqual(json.loads(recorded)['failed_build'], projection)





class HarnessTests(unittest.TestCase):
    def test_changed_origin_login_waits_for_old_control_pod_exit(self):
        state = {'public_url': 'http://gateway:8080', 'desired_url': 'http://gateway:8080',
                 'generation': 1, 'transition_polls': 0, 'old_pod_removed': False}

        class Login(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                self.rfile.read(int(self.headers.get('Content-Length', '0')))
                allowed = self.headers.get('Origin') == state['public_url']
                self.send_response(200 if allowed else 403)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(b'{"authenticated":true}' if allowed else b'{}')

        server = ThreadingHTTPServer(('127.0.0.1', 0), Login)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        acceptance.base = 'http://127.0.0.1:' + str(server.server_port)
        acceptance.origin = acceptance.base
        acceptance.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def kubectl(*args):
            if args[:2] == ('get', 'deployment'):
                template = {'spec': {'containers': [{'env': [
                    {'name': 'ARTIFACTORY_PUBLIC_URL', 'value': state['desired_url']},
                ]}]}}
                return json.dumps({'metadata': {'generation': state['generation']},
                                   'spec': {'selector': {'matchLabels': {'app': 'control-plane'}}, 'template': template}})
            if args[:2] == ('get', 'pods'):
                self.assertEqual(args, ('get', 'pods', '-l', 'app=control-plane', '-o', 'json'))
                old = {'metadata': {'name': 'old-control', 'uid': 'old-uid'}}
                new = {'metadata': {'name': 'new-control', 'uid': 'new-uid'}}
                if state['generation'] == 1:
                    return json.dumps({'items': [old]})
                if state['old_pod_removed']:
                    return json.dumps({'items': [new]})
                state['transition_polls'] += 1
                if state['transition_polls'] == 1:
                    # rollout 已完成，但旧 Pod 尚未出现删除标记。
                    return json.dumps({'items': [old, new]})
                state['old_pod_removed'] = True
                state['public_url'] = state['desired_url']
                return json.dumps({'items': [new]})
            if args[:2] == ('set', 'env'):
                desired = args[-1].split('=', 1)[1]
                if state['desired_url'] != desired:
                    state['generation'] += 1
                    state['desired_url'] = desired
            return ''

        acceptance.kubectl = kubectl
        try:
            with self.assertRaises(urllib.error.HTTPError) as rejected:
                acceptance.api('/api/v1/auth/login', {})
            self.assertEqual(rejected.exception.code, 403)
            rejected.exception.close()
            NATIVE.set_environment(acceptance, 'control-plane', {'ARTIFACTORY_PUBLIC_URL': acceptance.base})
            self.assertEqual(acceptance.api('/api/v1/auth/login', {}), {'authenticated': True})
            self.assertTrue(state['old_pod_removed'])
            self.assertEqual(state['transition_polls'], 2)

            def no_rollout_wait(*_args, **_kwargs):
                self.fail('Unchanged environment must not wait for live Pod replacement')

            acceptance.wait = no_rollout_wait
            NATIVE.set_environment(acceptance, 'control-plane', {'ARTIFACTORY_PUBLIC_URL': acceptance.base})
            self.assertEqual(acceptance.api('/api/v1/auth/login', {}), {'authenticated': True})
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    def test_gateway_has_resolver_token_without_storage_or_service_credentials(self):
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        acceptance.image_tag = 'fixture'
        acceptance.pull_policy = 'IfNotPresent'
        secret, deployment, service = acceptance.gateway('isolated-token-' + 'x' * 32)
        env = secret['stringData']
        self.assertEqual(env['ARTIFACTORY_GATEWAY_CONTROL'], 'http://control-plane:8080')
        self.assertEqual(env['ARTIFACTORY_S3_ENDPOINT'], 'http://s3:8333')
        self.assertFalse(any(key in env for key in ['ARTIFACTORY_S3_ACCESS_KEY', 'ARTIFACTORY_S3_SECRET_KEY', 'ARTIFACTORY_MASTER_KEY', 'ARTIFACTORY_SERVICE_TOKEN']))
        pod = deployment['spec']['template']['spec']
        self.assertFalse(pod['automountServiceAccountToken'])
        self.assertEqual(pod['containers'][0]['image'], acceptance.image('gateway'))
        self.assertEqual(pod['containers'][0]['envFrom'][0]['secretRef']['name'], secret['metadata']['name'])
        self.assertEqual(service['spec']['selector'], deployment['spec']['selector']['matchLabels'])

    def test_conversation_reads_all_pages_in_sequence(self):
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        paths = []
        def api(path):
            paths.append(path)
            return {'messages':[{'sequence':len(paths)}],'next_cursor':1 if len(paths)==1 else None}
        acceptance.api = api
        self.assertEqual(acceptance.conversation('build','run',2),[{'sequence':1},{'sequence':2}])
        self.assertTrue(paths[0].endswith('after=0&limit=50'))
        self.assertTrue(paths[1].endswith('after=1&limit=50'))

    def test_conversation_waits_for_runtime_acknowledgement(self):
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        acceptance.evidence = {}
        polls = []
        with tempfile.TemporaryDirectory() as temporary:
            acceptance.run = Path(temporary)

            def api(path):
                if path.endswith('/conversations'):
                    polls.append(path)
                    return {'conversations': [{'run_id': 'run', 'generation': 1,
                        'status': 'running' if len(polls) == 1 else 'succeeded',
                        'source_digest': 'fixed'}]}
                return {'messages': [{'sequence': 1, 'kind': 'proposal_result'}], 'next_cursor': None}

            def wait(callback, seconds):
                self.assertEqual(seconds, 60)
                self.assertIsNone(callback(), 'business completion does not acknowledge the Runtime outbox')
                return callback()

            acceptance.api = api
            acceptance.wait = wait
            sessions, messages = acceptance.wait_conversation('build', 'run', 1, 'fixed')
            self.assertEqual(sessions[0]['status'], 'succeeded')
            self.assertEqual(messages[0]['kind'], 'proposal_result')
            self.assertEqual(len(polls), 2)

    def test_conversation_wait_retains_strict_checks_and_safe_timeout_evidence(self):
        for changes in ({'source_digest': 'wrong'}, {'messages': []}, {'extra_session': True}):
            with self.subTest(changes=changes), tempfile.TemporaryDirectory() as temporary:
                acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
                acceptance.run = Path(temporary)
                acceptance.evidence = {}
                session = {'run_id': 'run', 'generation': 1, 'status': 'succeeded',
                    'source_digest': changes.get('source_digest', 'fixed'), 'secret': 'raw-credential'}
                messages = changes.get('messages', [{'sequence': 1, 'kind': 'message',
                    'payload': {'secret': 'raw-credential'}}])
                acceptance.api = lambda path: ({'conversations': [session] *
                    (2 if changes.get('extra_session') else 1)} if path.endswith('/conversations')
                    else {'messages': messages, 'next_cursor': None})

                def timeout(callback, _seconds):
                    self.assertIsNone(callback())
                    raise TimeoutError('acknowledgement did not converge')

                acceptance.wait = timeout
                with self.assertRaises(TimeoutError):
                    acceptance.wait_conversation('build', 'run', 1, 'fixed')
                evidence = json.loads((acceptance.run / 'evidence.json').read_text())
                self.assertEqual(evidence['conversation_snapshot']['message_count'], len(messages))
                self.assertNotIn('raw-credential', json.dumps(evidence))

    def test_control_source_authority_has_no_secret_read_permission(self):
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        acceptance.ns = 'fixture'
        account, role, binding, cluster, cluster_binding = acceptance.control_identity()
        self.assertEqual(account['metadata']['name'],'artifactory-control')
        self.assertTrue(any('sourcetasks' in rule['resources'] for rule in role['rules']))
        self.assertFalse(any('secrets' in rule['resources'] for rule in role['rules']))
        self.assertEqual(cluster['rules'][0]['resources'],['tokenreviews'])
        self.assertEqual(binding['subjects'],cluster_binding['subjects'])

    def test_service_dockerfiles_copy_only_existing_workspace_paths(self):
        files = ['Dockerfile', 'crates/emissary/Dockerfile', 'crates/gateway/Dockerfile', 'crates/planner-runtime/Dockerfile', 'crates/source-preparer/Dockerfile', 'operators/agent-operator/Dockerfile']
        for filename in files:
            for line in (ROOT / filename).read_text().splitlines():
                fields = shlex.split(line)
                if not fields or fields[0] != 'COPY' or any(field.startswith('--from') for field in fields):
                    continue
                sources = [field for field in fields[1:-1] if not field.startswith('--')]
                for source in sources:
                    self.assertTrue((ROOT / source).exists(), f'{filename}: missing COPY source {source}')

    def test_each_service_image_starts_its_own_binary(self):
        components = {'operators/agent-operator': 'artifactory-agent-operator', 'crates/planner-runtime': 'artifactory-planner-runtime', 'crates/source-preparer': 'artifactory-source-preparer'}
        for directory, binary in components.items():
            dockerfile = (ROOT / directory / 'Dockerfile').read_text()
            self.assertIn(f'ENTRYPOINT ["/usr/local/bin/{binary}"]', dockerfile)
            self.assertNotIn('entrypoint.sh', dockerfile)


if __name__ == '__main__':
    unittest.main()
