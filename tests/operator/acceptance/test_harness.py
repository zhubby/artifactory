#!/usr/bin/env python3
"""[INPUT]: 验收清单渲染器、本机 HTTP 服务和工作区 Dockerfile。
[OUTPUT]: 验证网关最小凭据、构建上下文和公告地址切换。
[POS]: 只执行本机检查，不访问 Docker 或 Kubernetes。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import shlex
import threading
import unittest
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location('operator_acceptance', Path(__file__).with_name('run.py'))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
NATIVE_SPEC = importlib.util.spec_from_file_location('native_acceptance', Path(__file__).with_name('native.py'))
NATIVE = importlib.util.module_from_spec(NATIVE_SPEC)
NATIVE_SPEC.loader.exec_module(NATIVE)


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
