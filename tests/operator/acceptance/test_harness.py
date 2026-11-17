#!/usr/bin/env python3
"""[INPUT]: 验收清单渲染器、本机 HTTP 服务和工作区 Dockerfile。
[OUTPUT]: 验证同源切换、设备恢复及清理完成后的成功证据。
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


class CompletedNativeFixture(DeviceFixture):
    def __init__(self, root):
        super().__init__()
        for node in self.nodes.values():
            node.update(name=node['id'], online=True)
        self.root, self.run = root, root / 'saved-run'
        (root / '.local/operator-load').mkdir(parents=True)
        binary = root / 'target/debug/artifactory-emissary'
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'fixture binary')
        self.processes, self.base = [], 'http://127.0.0.1:38081'
        self.public_url = self.origin = 'http://gateway:8080'
        self.failure_stage = None
        self.on_control_restored = lambda: None
        self.artifact = {'id': 'artifact', 'sha256': hashlib.sha256(b'operator-native-ok\n').hexdigest()}

    def resume(self):
        self.origin = self.public_url

    def kubectl(self, *args):
        if args[:2] == ('get', 'deployment'):
            return json.dumps({'spec': {'template': {'spec': {'containers': [{'env': [
                {'name': 'ARTIFACTORY_PUBLIC_URL', 'value': self.public_url}]}]}}}})
        if args[:2] == ('get', 'sourcepool'):
            return json.dumps({'spec': {}, 'metadata': {'generation': 1}, 'status': {'observedGeneration': 1}})
        if args[:2] == ('get', 'agentpool'):
            return json.dumps({'spec': {}})
        if args[:2] == ('get', 'statefulset'):
            return 'https://127.0.0.1:38090'
        if args == ('exec', 'deploy/control-plane', '--', 'printenv', 'ARTIFACTORY_PUBLIC_URL'):
            return self.public_url + '\n'
        return ''

    def set_environment(self, _acceptance, deployment, values):
        if deployment == 'control-plane':
            self.public_url = values['ARTIFACTORY_PUBLIC_URL']
            if self.public_url == 'http://gateway:8080':
                self.on_control_restored()

    def api(self, path, body=None, method=None):
        if path == '/api/v1/nodes':
            return json.loads(json.dumps(list(self.nodes.values())))
        if path.startswith('/api/v1/nodes?') or method == 'PATCH':
            if path == '/api/v1/nodes/old-device' and not body['draining'] and self.failure_stage == 'device_restore':
                raise RuntimeError('fixture device restore failure')
            return super().api(path, body, method)
        if path.endswith('/telemetry'):
            return {'host': {'fresh': True}, 'inventory': {'inventory': {
                'os': 'linux', 'arch': 'amd64', 'physical_memory_bytes': 1, 'tools': {'git': {'available': True}}}}}
        if '/processes?' in path:
            return {'samples': [{'step': 0, 'started_at': 1}]}
        if path.endswith('/plans'):
            return {'plans': [{}], 'runs': [{'calls': 0}]}
        if path.startswith('/api/v1/builds/'):
            return {'id': 'build', 'status': 'succeeded', 'source_snapshot_id': 'snapshot',
                    'commit_sha': 'a' * 40, 'artifacts': [self.artifact], 'jobs': []}
        if body is not None:
            return {'id': 'build' if path.endswith('/builds') else 'project', 'token': 'fixture-token'}
        raise AssertionError('unexpected fixture request: ' + path)

    def sql(self, _database, query):
        if 'json_build_object' in query:
            return json.dumps({'internal': 'https://source.svc:8090', 'public': 'https://127.0.0.1:38090'})
        if 'revoked AND draining' in query:
            return 't'
        if query.startswith('SELECT a.id'):
            return 'attempt'
        if "a.status='succeeded'" in query or 'coalesce(sum(lfs_download_bytes)' in query:
            return '1'
        return '0'

    def command(self, args, **_kwargs):
        if len(args) > 1 and args[1] == 'register':
            Path(args[args.index('--config') + 1]).write_text('{}')
            self.nodes['host-device'] = dict(self.nodes['old-device'], id='host-device',
                name=args[args.index('--name') + 1], draining=False)

    def cleanup(self):
        self.evidence_at_cleanup = self.evidence()
        if self.failure_stage == 'diagnostics':
            raise RuntimeError('fixture diagnostics failure')

    def evidence(self):
        return json.loads(next((self.root / '.local/operator-load').glob('*/evidence.json')).read_text())

    def invoke(self):
        self.forward = lambda *_: None
        self.wait = lambda callback, *_: callback()
        self.service_request = lambda *_: 'ok'
        self.verify_artifact_gateway = lambda *_: None
        self.images = lambda: []
        process = SimpleNamespace(terminate=lambda: None, wait=lambda **_: None)
        with patch.object(NATIVE, 'ROOT', self.root), \
                patch.object(NATIVE.module, 'Acceptance', return_value=self), \
                patch.object(NATIVE, 'set_environment', side_effect=self.set_environment), \
                patch.object(NATIVE.platform, 'system', return_value='Linux'), \
                patch.object(NATIVE.platform, 'machine', return_value='x86_64'), \
                patch.object(NATIVE.subprocess, 'run', side_effect=self.command), \
                patch.object(NATIVE.subprocess, 'Popen', return_value=process):
            NATIVE.run(SimpleNamespace(context='fixture', namespace='fixture', live_protocol=False))


class NativeDeviceIsolationTests(unittest.TestCase):
    def test_success_is_recorded_only_after_device_and_route_cleanup(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = CompletedNativeFixture(Path(temporary))
            output = io.StringIO()
            with redirect_stdout(output):
                fixture.invoke()
            self.assertFalse(fixture.evidence_at_cleanup['passed'])
            self.assertTrue(fixture.evidence()['passed'])
            self.assertTrue(fixture.evidence()['existing_device_draining_restored'])
            self.assertFalse(fixture.nodes['old-device']['draining'])
            self.assertTrue(fixture.nodes['host-device']['revoked'])
            self.assertIn('HOST NATIVE ACCEPTANCE PASSED', output.getvalue())

    def test_successful_build_with_failed_cleanup_never_reports_success(self):
        for stage in ('device_restore', 'diagnostics'):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as temporary:
                fixture = CompletedNativeFixture(Path(temporary))
                fixture.failure_stage = stage
                output = io.StringIO()
                with redirect_stdout(output), self.assertRaisesRegex(RuntimeError, 'fixture .* failure'):
                    fixture.invoke()
                self.assertFalse(fixture.evidence()['passed'])
                self.assertNotIn('HOST NATIVE ACCEPTANCE PASSED', output.getvalue())

    def test_real_http_restore_rejects_old_origin_and_accepts_refreshed_origin(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = CompletedNativeFixture(Path(temporary))
            service_api = fixture.api
            patch_results = []

            class API(BaseHTTPRequestHandler):
                def log_message(self, *_args):
                    pass

                def do_GET(self):
                    self.respond(service_api(self.path))

                def do_PATCH(self):
                    body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    valid = self.headers.get('Origin') == fixture.public_url
                    patch_results.append((self.headers.get('Origin'), 200 if valid else 403))
                    self.respond(service_api(self.path, body, 'PATCH') if valid else {}, 200 if valid else 403)

                def do_POST(self):
                    body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    self.respond(service_api(self.path, body))

                def respond(self, value, status=200):
                    self.send_response(status)
                    self.send_header('Content-Type', 'application/json')
                    self.end_headers()
                    self.wfile.write(json.dumps(value).encode())

            with ThreadingHTTPServer(('127.0.0.1', 0), API) as server:
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                fixture.base = f'http://127.0.0.1:{server.server_port}'
                fixture.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                fixture.api = MODULE.Acceptance.api.__get__(fixture)

                def reject_stale_origin():
                    with self.assertRaises(urllib.error.HTTPError) as failure:
                        fixture.api('/api/v1/nodes/old-device', {}, method='PATCH')
                    self.assertEqual(failure.exception.code, 403)
                    failure.exception.close()

                fixture.on_control_restored = reject_stale_origin
                try:
                    with redirect_stdout(io.StringIO()):
                        fixture.invoke()
                    self.assertFalse(fixture.nodes['old-device']['draining'])
                    self.assertEqual(patch_results[-2:], [(fixture.base, 403), ('http://gateway:8080', 200)])
                finally:
                    server.shutdown()
                    thread.join(timeout=5)

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

    def test_run_restores_original_devices_after_host_compilation_failure(self):
        self.assert_run_failure_cleanup(route_failure=False)

    def test_route_cleanup_error_still_restores_devices_and_preserves_evidence(self):
        self.assert_run_failure_cleanup(route_failure=True)

    def assert_run_failure_cleanup(self, route_failure):
        fixture = DeviceFixture()
        original = json.loads(json.dumps(fixture.nodes))
        fixture.processes = []
        fixture.base = 'http://127.0.0.1:38081'
        fixture.resume = lambda: None
        fixture.sql = lambda *_: '0'
        fixture.forward = lambda *_: None
        fixture.wait = lambda callback, *_: callback()
        fixture.service_request = lambda *_: 'ok'
        cleaned = []
        fixture.cleanup = lambda: cleaned.append(True)

        def kubectl(*args):
            if args[:2] == ('get', 'deployment'):
                return json.dumps({'spec': {'template': {'spec': {'containers': [{'env': []}]}}}})
            if args[:2] == ('get', 'sourcepool'):
                return json.dumps({'spec': {'publicUrlTemplate': 'https://source.invalid'},
                                   'metadata': {'generation': 1}, 'status': {'observedGeneration': 1}})
            if args[:2] == ('get', 'agentpool'):
                return json.dumps({'spec': {'maxActive': 2}})
            if args[:2] == ('get', 'statefulset'):
                return 'https://127.0.0.1:38090'
            return ''

        fixture.kubectl = kubectl
        def cargo_failure(*_args, **_kwargs):
            self.assertTrue(fixture.nodes['old-device']['draining'])
            raise RuntimeError('fixture cargo failure')

        route_updates = [None, None, RuntimeError('fixture route cleanup failure')] if route_failure else None
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / '.local/operator-load').mkdir(parents=True)
            with patch.object(NATIVE, 'ROOT', root), \
                    patch.object(NATIVE.module, 'Acceptance', return_value=fixture), \
                    patch.object(NATIVE, 'set_environment', side_effect=route_updates), \
                    patch.object(NATIVE.subprocess, 'run', side_effect=cargo_failure):
                error = 'fixture route cleanup failure' if route_failure else 'fixture cargo failure'
                with self.assertRaisesRegex(RuntimeError, error):
                    NATIVE.run(SimpleNamespace(context='fixture', namespace='fixture', live_protocol=False))
            evidence = json.loads(next((root / '.local/operator-load').glob('*/evidence.json')).read_text())
            self.assertTrue(evidence['existing_device_draining_restored'])
            self.assertFalse(evidence['passed'])
        self.assertEqual(fixture.nodes, original)
        self.assertEqual(cleaned, [True])

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

    def test_service_dockerfiles_copy_only_existing_workspace_paths(self):
        files = ['Dockerfile', 'crates/emissary/Dockerfile', 'crates/gateway/Dockerfile', 'crates/planner-runtime/Dockerfile', 'crates/source-service/Dockerfile', 'operators/agent-operator/Dockerfile']
        for filename in files:
            for line in (ROOT / filename).read_text().splitlines():
                fields = shlex.split(line)
                if not fields or fields[0] != 'COPY' or any(field.startswith('--from') for field in fields):
                    continue
                sources = [field for field in fields[1:-1] if not field.startswith('--')]
                for source in sources:
                    self.assertTrue((ROOT / source).exists(), f'{filename}: missing COPY source {source}')

    def test_each_service_image_starts_its_own_binary(self):
        components = {'operators/agent-operator': 'artifactory-agent-operator', 'crates/planner-runtime': 'artifactory-planner-runtime', 'crates/source-service': 'artifactory-source-service'}
        for directory, binary in components.items():
            dockerfile = (ROOT / directory / 'Dockerfile').read_text()
            self.assertIn(f'ENTRYPOINT ["/usr/local/bin/{binary}"]', dockerfile)
            self.assertNotIn('entrypoint.sh', dockerfile)


if __name__ == '__main__':
    unittest.main()
