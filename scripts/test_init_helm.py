"""[INPUT]: Helm 模板与独立 Secret 初始化脚本。
[OUTPUT]: 验证统一网关、私有入口和不可覆盖的持久凭据。
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
                                 'imageBuilds.enabled=true,ingress.enabled=true'],
                                check=True, capture_output=True, text=True)
        rendered = result.stdout
        self.assertIn('- {port: 8443, targetPort: grpc, name: grpc}', rendered)
        gateway = rendered.split('# Source: artifactory/templates/gateway.yaml')[1:]
        self.assertEqual(sum('kind: Deployment' in document.split('---')[0] for document in gateway), 1)
        deployment = next(document.split('---')[0] for document in gateway if 'kind: Deployment' in document.split('---')[0])
        self.assertIn('metadata: {name: custom-gateway}', deployment)
        self.assertIn('replicas: 2', deployment)
        self.assertIn('minReadySeconds: 5', deployment)
        self.assertIn('maxUnavailable: 0', deployment)
        self.assertIn('maxSurge: 1', deployment)
        self.assertIn('ARTIFACTORY_GATEWAY_PROFILE, value: "unified"', deployment)
        self.assertIn('ARTIFACTORY_GATEWAY_INTERNAL_BIND, value: \'0.0.0.0:8445\'', deployment)
        self.assertIn('ARTIFACTORY_GATEWAY_INTERNAL_TLS_KEY, value: /run/internal-tls/server.key', deployment)
        self.assertIn('ARTIFACTORY_GATEWAY_INTERNAL_CONNECTIONS, value: "1024"', deployment)
        for old in ['gateway-edge', 'gateway-device', 'gateway-data']:
            self.assertNotIn(old, rendered)
        self.assertIn('port: 8445, targetPort: https', rendered)
        services = rendered.split('kind: Service\n')[1:]
        for name in ['custom-web', 'custom-grpc', 'custom-gateway-internal']:
            service = next(value.split('---')[0] for value in services
                           if f'name: {name}\n' in value or f'name: {name}}}' in value)
            self.assertIn('app.kubernetes.io/component: gateway\n', service)
        internal = next(value.split('---')[0] for value in services if 'name: custom-gateway-internal}' in value)
        self.assertIn('type: ClusterIP', internal)
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
    def test_gateway_nodeports_and_network_sources_preserve_internal_boundary(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        command = ['helm', 'template', 'boundary', str(chart), '--set',
                   'grpc.service.type=NodePort,grpc.service.nodePort=30843,grpc.service.telemetryNodePort=30844,gateway.internalTlsSecret=service-tls',
                   '--set-json', 'gateway.networkPolicy.publicFrom=[{"ipBlock":{"cidr":"10.169.1.0/24"}}]',
                   '--set-json', 'gateway.networkPolicy.deviceFrom=[{"ipBlock":{"cidr":"10.233.0.0/16"}}]']
        rendered = subprocess.run(command, check=True, capture_output=True, text=True).stdout
        grpc = next(document.split('---')[0] for document in rendered.split('kind: Service\n')[1:]
                    if 'name: boundary-grpc\n' in document)
        control, telemetry = grpc.split('    - port: ')[1:]
        self.assertTrue(control.startswith('8443\n'))
        self.assertIn('targetPort: grpc', control)
        self.assertIn('nodePort: 30843', control)
        self.assertTrue(telemetry.startswith('8444\n'))
        self.assertIn('targetPort: telemetry', telemetry)
        self.assertIn('nodePort: 30844', telemetry)
        self.assertIn('secretName: service-tls', rendered)
        policy = next(document for document in rendered.split('---') if 'kind: NetworkPolicy\n' in document)
        public, devices, internal = policy.split('    - ports: ')[1:]
        self.assertIn('port: 8080', public)
        self.assertIn('cidr: 10.169.1.0/24', public)
        self.assertIn('port: 8443', devices)
        self.assertIn('port: 8444', devices)
        self.assertIn('cidr: 10.233.0.0/16', devices)
        self.assertNotIn('port: 8445', public + devices)
        self.assertIn('port: 8445', internal)
        self.assertIn('from: [{podSelector: {}}]', internal)
        self.assertNotIn('namespaceSelector', internal)
        invalid = subprocess.run(command + ['--set', 'grpc.service.telemetryNodePort=30843'],
                                 capture_output=True, text=True)
        self.assertNotEqual(invalid.returncode, 0)
        self.assertIn('must differ', invalid.stderr)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_telemetry_backend_cannot_publish_a_second_device_entry(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        command = ['helm', 'template', 'boundary', str(chart)]
        rendered = subprocess.run(command, check=True, capture_output=True, text=True).stdout
        backend = next(document.split('---')[0] for document in rendered.split('kind: Service\n')[1:]
                       if 'name: boundary-telemetry-grpc\n' in document)
        self.assertIn('type: ClusterIP', backend)
        self.assertNotIn('nodePort:', backend)
        for legacy in ['telemetry.service.type=NodePort,telemetry.service.nodePort=30844',
                       'telemetry.service.type=LoadBalancer', 'telemetry.service.nodePort=30844']:
            with self.subTest(legacy=legacy):
                invalid = subprocess.run(command + ['--set', legacy], capture_output=True, text=True)
                self.assertNotEqual(invalid.returncode, 0)
                self.assertIn('grpc.service', invalid.stderr)

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
