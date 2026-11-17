"""[INPUT]: Helm 模板与独立 Secret 初始化脚本。
[OUTPUT]: 验证私有入口、服务端口和不可覆盖的持久凭据。
[POS]: 部署契约回归；不连接 Kubernetes。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import base64
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("init-helm.py")


class HelmBootstrapTests(unittest.TestCase):
    def test_rollout_notes_do_not_contradict_replicated_control_or_variable_pvc_counts(self):
        notes = (SCRIPT.parents[1] / 'charts/artifactory/templates/NOTES.txt').read_text()
        self.assertNotIn('do not scale its Deployment above one replica', notes)
        self.assertNotIn('all three PVCs', notes)
        self.assertIn('.Values.controlPlane.replicas', notes)
        self.assertIn('.Values.localSource.enabled', notes)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_trial_profile_is_replicated_private_and_uses_default_priority(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        result = subprocess.run(['helm', 'template', 'artifactory', str(chart),
                                 '--namespace', 'artifactory', '--kube-version', '1.28.3',
                                 '-f', str(chart / 'values-trial.yaml')],
                                check=True, capture_output=True, text=True)
        self.assertIn('replicas: 3', result.stdout)
        self.assertIn('name: artifactory-local-source', result.stdout)
        self.assertIn('kind: PodDisruptionBudget', result.stdout)
        self.assertNotIn('type: NodePort', result.stdout)
        self.assertNotIn('type: LoadBalancer', result.stdout)
        self.assertNotIn('kind: Ingress\n', result.stdout)
        self.assertNotIn('kind: ClusterRole', result.stdout)
        self.assertNotIn('preemptionPolicy:', result.stdout)
        self.assertNotIn('priorityClassName:', result.stdout)
        self.assertNotIn('kind: PriorityClass', result.stdout)
        version = (SCRIPT.parents[1] / 'VERSION').read_text().strip()
        self.assertIn(f'ghcr.io/zhubby/artifactory-control-plane:{version}', result.stdout)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_replicated_control_requires_source_isolation_and_renders_ha_resources(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        invalid = subprocess.run(['helm', 'template', 'ha-test', str(chart), '--set', 'controlPlane.replicas=3'],
                                 capture_output=True, text=True)
        self.assertNotEqual(invalid.returncode, 0)
        self.assertIn('localSource.enabled', invalid.stderr)
        result = subprocess.run(['helm', 'template', 'ha-test', str(chart), '-f', str(chart / 'values-ha.yaml')],
                                check=True, capture_output=True, text=True)
        self.assertIn('replicas: 3', result.stdout)
        self.assertIn('command: [artifactory-control-plane, serve-source]', result.stdout)
        self.assertIn('emptyDir: {sizeLimit: 10Gi}', result.stdout)
        self.assertIn('kind: PodDisruptionBudget', result.stdout)
        self.assertIn('https://ha-test-local-source:8080', result.stdout)
        self.assertIn('ARTIFACTORY_SERVICE_CA, value: /run/ca/ca.crt', result.stdout)
        self.assertIn('ARTIFACTORY_GATEWAY_SOURCE, value: "https://ha-test-local-source:8080"', result.stdout)
        self.assertIn('ARTIFACTORY_DB_MAX_CONNECTIONS: "40"', result.stdout)
        self.assertIn('path: /readyz', result.stdout)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_gateway_ports_and_independent_image_services(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        result = subprocess.run(['helm', 'template', 'custom', str(chart), '--set',
                                 'imageBuilds.enabled=true,gateway.dedicatedData=true,ingress.enabled=true'],
                                check=True, capture_output=True, text=True)
        rendered = result.stdout
        self.assertIn('- {port: 8443, targetPort: grpc, name: grpc}', rendered)
        self.assertIn('app.kubernetes.io/component: gateway-device', rendered)
        self.assertIn('ARTIFACTORY_GATEWAY_PROFILE, value: "control"', rendered)
        self.assertIn('ARTIFACTORY_GATEWAY_PROFILE, value: "data"', rendered)
        self.assertIn('ARTIFACTORY_GATEWAY_PROFILE, value: "internal"', rendered)
        self.assertIn('ARTIFACTORY_GATEWAY_TLS_KEY, value: /run/ca/server.key', rendered)
        self.assertIn('port: 8445, targetPort: https', rendered)
        self.assertIn('secretKeyRef: {name: custom-gateway, key: token}', rendered)
        self.assertIn('secretName: custom-registry-signing', rendered)
        self.assertIn('ARTIFACTORY_S3_ENDPOINT: "http://custom-s3:8333"', rendered)
        self.assertIn('ARTIFACTORY_S3_ALLOW_HTTP: "true"', rendered)
        self.assertNotIn('type: NodePort', rendered)
        ingress = rendered.split('# Source: artifactory/templates/ingress.yaml')[1].split('---')[0]
        self.assertNotIn('custom-s3', ingress)
        image_chart = SCRIPT.parents[1] / 'charts/artifactory-image-operator'
        image = subprocess.run(['helm', 'template', 'images', str(image_chart)],
                               check=True, capture_output=True, text=True).stdout
        self.assertIn('fieldPath: metadata.namespace', image)
        self.assertNotIn('metadata.uidspace', image)
        self.assertNotIn('resources: [secrets]', image)
        agent = subprocess.run(['helm', 'template', 'native', str(SCRIPT.parents[1] / 'charts/artifactory-agent-operator')],
                               check=True, capture_output=True, text=True).stdout
        self.assertIn('name: native-agent-operator', agent)
        self.assertIn('app.kubernetes.io/name: artifactory-agent-operator', agent)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_private_external_http_requires_explicit_permission(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        command = ['helm', 'template', 'external', str(chart), '--set',
                   's3.enabled=false,s3.endpoint=http://private-objects:8333']
        denied = subprocess.run(command, capture_output=True, text=True)
        self.assertNotEqual(denied.returncode, 0)
        allowed = subprocess.run(command + ['--set', 's3.allowHttp=true'],
                                 check=True, capture_output=True, text=True)
        self.assertIn('ARTIFACTORY_S3_ALLOW_HTTP: "true"', allowed.stdout)
        self.assertIn('ARTIFACTORY_PUBLIC_URL: "https://build.example.com"', allowed.stdout)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_external_stateful_services_are_not_created(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        result = subprocess.run(['helm', 'template', 'external', str(chart), '--set',
                                 'postgres.enabled=false,s3.enabled=false,s3.endpoint=https://objects.example.com'],
                                check=True, capture_output=True, text=True)
        self.assertNotIn('name: external-postgres\n', result.stdout)
        self.assertNotIn('name: external-s3\n', result.stdout)
        self.assertIn('ARTIFACTORY_S3_ENDPOINT: "https://objects.example.com"', result.stdout)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_registration_defaults_on_and_can_be_disabled(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        for enabled in ['true', 'false']:
            with self.subTest(enabled=enabled):
                command = ['helm', 'template', 'registration-test', str(chart)]
                if enabled == 'false':
                    command.extend(['--set', 'auth.registrationEnabled=false'])
                result = subprocess.run(command, check=True, capture_output=True, text=True)
                self.assertTrue(
                    f'ARTIFACTORY_REGISTRATION_ENABLED: "{enabled}"' in result.stdout,
                    'rendered registration setting does not match the requested value',
                )

    def test_persistent_secrets_and_certificate_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "private"
            command = [sys.executable, str(SCRIPT), "--output", str(output),
                       "--namespace", "test-builds", "--release", "test-kit",
                       "--grpc-host", "127.0.0.1", "--grpc-host", "agents.example.com"]
            subprocess.run(command, check=True, capture_output=True)
            before = {str(path): path.read_bytes() for path in output.rglob("*") if path.is_file()}
            secret = json.loads((output / "credentials.json").read_text())
            data = secret["stringData"]
            self.assertEqual(secret["metadata"]["namespace"], "test-builds")
            self.assertEqual(len(base64.b64decode(data["master-key"])), 32)
            self.assertIn(data["database-password"] + "@test-kit-postgres:5432/", data["database-url"])
            self.assertEqual(json.loads((output / "access.json").read_text())["password"],
                             data["bootstrap-password"])
            identity = json.loads(data["s3.json"])["identities"][0]["credentials"][0]
            self.assertEqual(identity["secretKey"], data["s3-secret-key"])
            gateway = json.loads((output / 'gateway.json').read_text())
            self.assertEqual(gateway['metadata']['name'], 'test-kit-gateway')
            self.assertGreaterEqual(len(gateway['stringData']['token']), 32)
            signing = json.loads((output / 'registry-signing.json').read_text())['stringData']
            public = json.loads((output / 'registry-signing-public.json').read_text())['stringData']
            self.assertEqual(public, {'tls.crt': signing['tls.crt']})
            self.assertNotEqual(signing['tls.key'], (output / 'pki/ca.key').read_text())
            subprocess.run(["openssl", "verify", "-CAfile", str(output / "pki/ca.pem"),
                            "-verify_hostname", "agents.example.com", str(output / "pki/server.pem")],
                           check=True, capture_output=True)
            subprocess.run(["openssl", "verify", "-CAfile", str(output / "pki/ca.pem"),
                            "-verify_ip", "127.0.0.1", str(output / "pki/server.pem")],
                           check=True, capture_output=True)
            subprocess.run(["openssl", "verify", "-CAfile", str(output / "pki/ca.pem"),
                            "-verify_hostname", "test-kit-local-source", str(output / "pki/server.pem")],
                           check=True, capture_output=True)
            subprocess.run(["openssl", "verify", "-CAfile", str(output / "pki/ca.pem"),
                            "-verify_hostname", "test-kit-gateway-internal", str(output / "pki/server.pem")],
                           check=True, capture_output=True)
            if os.name != "nt":
                for path in output.rglob("*"):
                    self.assertEqual(path.stat().st_mode & 0o077, 0, str(path))
            repeated = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(repeated.returncode, 0)
            self.assertIn("refusing to rotate", repeated.stderr)
            self.assertEqual(before, {str(path): path.read_bytes()
                                      for path in output.rglob("*") if path.is_file()})

    def test_rejects_invalid_hosts_before_creating_secrets(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "private"
            result = subprocess.run([sys.executable, str(SCRIPT), "--output", str(output),
                                     "--grpc-host", "bad.example.com,IP:1.2.3.4"], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
