#!/usr/bin/env python3
"""[INPUT]: 验收清单渲染器、本机 HTTP 服务和工作区 Dockerfile。
[OUTPUT]: 验证 Release 构建入口、会话、池恢复和设备隔离。
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
    def test_build_creation_uses_unique_releases_with_exact_target_contracts(self):
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        acceptance.arch = 'amd64'
        calls = []
        def api(path, body):
            calls.append((path, body))
            if path.endswith('/releases'):
                return {'id': 'release-' + str(len(calls)), 'version': body['version']}
            return {'id': 'build-' + str(len(calls))}
        acceptance.api = api
        first = acceptance.create_build('project', 'Build native artifact')
        image_contract = [{'kind': 'image', 'key': 'image', 'platforms': ['linux/amd64', 'linux/arm64']}]
        second = acceptance.create_build('project', 'Build image', ref='a' * 40,
            target_requirements=image_contract, parameters={'fixture': True})
        self.assertEqual(calls[0][0], '/api/v1/projects/project/releases')
        self.assertEqual(calls[0][1]['target_requirements'], [
            {'kind': 'native', 'key': 'app', 'os': 'linux', 'arch': 'amd64'}])
        self.assertEqual(calls[0][1]['targets'], ['app'])
        self.assertEqual(calls[0][1]['ref'], 'v-fixture')
        self.assertRegex(first['release_version'], r'^0\.0\.0-acceptance\.r[0-9a-f]{32}$')
        self.assertNotEqual(first['release_version'], second['release_version'])
        self.assertEqual(calls[1], ('/api/v1/releases/release-1/builds', {'parameters': {}}))
        self.assertEqual(calls[2][1]['target_requirements'], image_contract)
        self.assertEqual(calls[2][1]['targets'], ['image'])
        self.assertEqual(calls[2][1]['ref'], 'a' * 40)
        self.assertEqual(calls[3], ('/api/v1/releases/release-3/builds', {'parameters': {'fixture': True}}))
        self.assertEqual(second['release_id'], 'release-3')

    def test_release_failure_does_not_submit_an_unowned_build(self):
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        acceptance.arch = 'amd64'
        calls = []
        def api(path, body):
            calls.append(path)
            raise RuntimeError('release contract rejected')
        acceptance.api = api
        with self.assertRaisesRegex(RuntimeError, 'release contract rejected'):
            acceptance.create_build('project', 'Build native artifact')
        self.assertEqual(calls, ['/api/v1/projects/project/releases'])

    def test_pool_pause_restores_original_capacity_after_success_and_failure(self):
        for failure in (None, 'assertion', 'lost-patch-response'):
            with self.subTest(failure=failure):
                acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
                state = {'capacity': 7, 'generation': 1, 'phase': 'Ready'}
                writes = []

                def kubectl(*args):
                    if args[0] == 'get':
                        return json.dumps({'metadata': {'generation': state['generation']},
                            'spec': {'maxActive': state['capacity']},
                            'status': {'phase': state['phase'], 'observedGeneration': state['generation']}})
                    capacity = json.loads(args[-1])['spec']['maxActive']
                    state.update(capacity=capacity, generation=state['generation'] + 1,
                                 phase='Paused' if capacity == 0 else 'Ready')
                    writes.append(capacity)
                    if failure == 'lost-patch-response' and capacity == 0:
                        raise RuntimeError('response lost after committed pause')
                    return ''

                def wait(callback, seconds):
                    self.assertEqual(seconds, 30)
                    self.assertTrue(callback())

                def check(_name, value):
                    self.assertTrue(value)
                    if failure == 'assertion':raise AssertionError('fixture assertion')

                acceptance.kubectl, acceptance.wait, acceptance.check = kubectl, wait, check
                if failure:
                    with self.assertRaises((AssertionError, RuntimeError)):
                        acceptance.verify_pool_pause()
                else:acceptance.verify_pool_pause()
                self.assertEqual(writes, [0, 7])
                self.assertEqual((state['capacity'], state['phase']), (7, 'Ready'))

    def test_cleanup_collects_every_operator_replica_with_prefixes(self):
        acceptance = MODULE.Acceptance.__new__(MODULE.Acceptance)
        calls = []
        acceptance.processes = []
        def kubectl(*args):
            calls.append(args)
            return '[pod/leader/operator] leader\n[pod/follower/operator] follower\n'
        acceptance.kubectl = kubectl
        with tempfile.TemporaryDirectory() as temporary:
            acceptance.run = Path(temporary)
            acceptance.cleanup()
            operator = next(call for call in calls if '-l' in call)
            self.assertIn('app.kubernetes.io/name=artifactory-planning-operator', operator)
            self.assertIn('--prefix=true', operator)
            self.assertNotIn('deploy/artifactory-planning-operator', operator)
            self.assertIn('follower', (acceptance.run / 'artifactory-planning-operator.log').read_text())

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
        acceptance.registry = 'localhost:5001/artifactory'
        acceptance.pull_policy = 'IfNotPresent'
        secret, deployment, service = acceptance.gateway('isolated-token-' + 'x' * 32)
        env = secret['stringData']
        self.assertEqual(env['ARTIFACTORY_GATEWAY_CONTROL'], 'http://control-plane:8080')
        self.assertEqual(env['ARTIFACTORY_S3_ENDPOINT'], 'http://artifactory-seaweedfs:8333')
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
        self.assertTrue(all('list' in rule['verbs'] for rule in role['rules']))
        self.assertFalse(any('secrets' in rule['resources'] for rule in role['rules']))
        self.assertEqual(cluster['rules'][0]['resources'],['tokenreviews'])
        self.assertEqual(binding['subjects'],cluster_binding['subjects'])

    def test_service_dockerfiles_copy_only_existing_workspace_paths(self):
        files = ['Dockerfile', 'crates/emissary/Dockerfile', 'crates/gateway/Dockerfile', 'crates/source-preparer/Dockerfile', 'operators/planning-operator/Dockerfile']
        for filename in files:
            for line in (ROOT / filename).read_text().splitlines():
                fields = shlex.split(line)
                if not fields or fields[0] != 'COPY' or any(field.startswith('--from') for field in fields):
                    continue
                sources = [field for field in fields[1:-1] if not field.startswith('--')]
                for source in sources:
                    self.assertTrue((ROOT / source).exists(), f'{filename}: missing COPY source {source}')

    def test_each_service_image_starts_its_own_binary(self):
        components = {'operators/planning-operator': 'artifactory-planning-operator', 'crates/source-preparer': 'artifactory-source-preparer'}
        for directory, binary in components.items():
            dockerfile = (ROOT / directory / 'Dockerfile').read_text()
            self.assertIn(f'ENTRYPOINT ["/usr/local/bin/{binary}"]', dockerfile)
            self.assertNotIn('entrypoint.sh', dockerfile)


if __name__ == '__main__':
    unittest.main()
