#!/usr/bin/env python3
"""[INPUT]: 验收清单渲染器和工作区 Dockerfile。
[OUTPUT]: 验证网关最小凭据与迁移后的构建上下文。
[POS]: 只执行本机检查，不访问 Docker 或 Kubernetes。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
from pathlib import Path
import shlex
import unittest

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location('operator_acceptance', Path(__file__).with_name('run.py'))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class HarnessTests(unittest.TestCase):
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
