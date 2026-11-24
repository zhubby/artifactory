"""[INPUT]: Helm 模板与独立 Secret 初始化脚本。
[OUTPUT]: 验证账号、镜像池、持久凭据及 Registry 与终端的入站隔离。
[POS]: 部署契约回归；不连接 Kubernetes。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import base64
import json
import os
import shutil
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("init-helm.py")


class HelmBootstrapTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_notification_smtp_secret_and_private_allowlist_are_explicit(self):
        chart = str(SCRIPT.parents[1] / 'charts/artifactory')
        base = ['helm', 'template', 'notifications', chart, '--kube-version', '1.28.3']
        default = subprocess.run(base, check=True, capture_output=True, text=True).stdout
        self.assertIn('ARTIFACTORY_WEBHOOK_PRIVATE_ALLOWLIST, value: ""', default)
        configured = subprocess.run(base + [
            '--set', 'notifications.smtpSecretName=platform-smtp',
            '--set', 'notifications.webhookPrivateAllowlist=10.42.0.0/24',
        ], check=True, capture_output=True, text=True).stdout
        self.assertIn('name: "platform-smtp"', configured)
        self.assertIn('ARTIFACTORY_WEBHOOK_PRIVATE_ALLOWLIST, value: "10.42.0.0/24"', configured)
        self.assertNotIn('ARTIFACTORY_SMTP_PASSWORD:', configured)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_image_default_pool_and_readonly_pool_lookup_are_independent_from_native_pools(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        rendered = subprocess.run(['helm', 'template', 'images', str(chart), '--set',
                                   'imageBuilds.enabled=true,imageBuilds.pool=ci-images'],
                                  check=True, capture_output=True, text=True).stdout
        self.assertIn('name: ARTIFACTORY_IMAGE_POOL, value: "ci-images"', rendered)
        rules = [document for document in rendered.split('---') if 'kind: Role\n' in document]
        self.assertTrue(any(re.search(r'resources: \[builderpools\]\s+verbs: \[get\]', role)
                            for role in rules))

    def services(self, rendered):
        services = {}
        for document in rendered.split('\n---\n'):
            if re.search(r'^kind: Service$', document, re.MULTILINE):
                name = re.search(r'^metadata:(?: \{ *|\n +)name: ([^\s,}]+)', document, re.MULTILINE)
                self.assertIsNotNone(name, document)
                services[name.group(1)] = document
        return services

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_registry_ingress_cannot_inherit_the_trial_namespace_allow_rule(self):
        root = SCRIPT.parents[1]
        rendered = subprocess.run([
            'helm', 'template', 'registry-test', str(root / 'charts/artifactory-registry'),
            '--kube-version', '1.28.3',
        ], check=True, capture_output=True, text=True).stdout
        policies = [doc for doc in rendered.split('\n---\n') if 'kind: NetworkPolicy\n' in doc]
        self.assertEqual(len(policies), 1)
        policy = policies[0]
        destination, allowed = policy.split('  ingress:\n', 1)
        self.assertIn('app.kubernetes.io/name: artifactory-registry', destination)
        self.assertIn('app.kubernetes.io/instance: registry-test', destination)
        self.assertIn('app.kubernetes.io/component: gateway', allowed)
        self.assertIn('app.kubernetes.io/name: artifactory', allowed)
        self.assertIn('{protocol: TCP, port: 5000}', allowed)
        self.assertNotIn('podSelector: {}', allowed)
        trial = (root / 'deploy/kubernetes-trial.yaml').read_text()
        trial_policies = [doc for doc in trial.split('\n---\n') if 'kind: NetworkPolicy\n' in doc]
        self.assertEqual(len(trial_policies), 2)
        broad = next(doc for doc in trial_policies if '  podSelector: {}\n' in doc)
        self.assertIn('policyTypes: [Egress]', broad)
        self.assertNotIn('  ingress:', broad)
        internal = next(doc for doc in trial_policies if '  ingress:' in doc)
        self.assertIn('key: app.kubernetes.io/name\n        operator: NotIn\n        values: [artifactory-registry]', internal)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_terminal_relay_cannot_inherit_namespace_wide_ingress(self):
        root = SCRIPT.parents[1]
        rendered = subprocess.run([
            'helm', 'template', 'terminal-test', str(root / 'charts/artifactory'),
            '--kube-version', '1.28.3', '--set', 'controlPlane.replicas=3',
        ], check=True, capture_output=True, text=True).stdout
        policy = next(doc for doc in rendered.split('\n---\n')
                      if 'name: artifactory-terminal-relay\n' in doc)
        existing_ports = policy.split('    - ports: [{port: 8446, protocol: TCP}]', 1)[0]
        self.assertIn('from:\n        - podSelector: {}', existing_ports)
        source = policy.split('    - ports: [{port: 8446, protocol: TCP}]', 1)[1]
        self.assertIn('app.kubernetes.io/component: control-plane', source)
        self.assertIn('app.kubernetes.io/instance: terminal-test', source)
        self.assertNotIn('podSelector: {}', source)
        trial = (root / 'deploy/kubernetes-trial.yaml').read_text()
        internal = next(doc for doc in trial.split('\n---\n')
                        if 'name: artifactory-trial-internal\n' in doc)
        self.assertIn('key: app.kubernetes.io/component\n        operator: NotIn\n        values: [control-plane]', internal)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_planning_pool_and_coordinator_settings_reach_control(self):
        chart = str(SCRIPT.parents[1] / 'charts/artifactory')
        base = ['helm', 'template', 'planning-test', chart, '--kube-version', '1.28.3']
        default = subprocess.run(base, check=True, capture_output=True, text=True).stdout
        self.assertIn('ARTIFACTORY_PLANNING_POOL: "default"', default)
        configured = subprocess.run(base + [
            '--set', 'planning.pool=custom-planning',
            '--set', 'planning.coordinatorUrl=https://planning.example/internal/planning',
            '--set', 'planning.serviceSecretName=planning-identity',
        ], check=True, capture_output=True, text=True).stdout
        self.assertIn('ARTIFACTORY_PLANNING_POOL: "custom-planning"', configured)
        self.assertIn('ARTIFACTORY_PLANNING_URL: "https://planning.example/internal/planning"', configured)
        self.assertIn('name: planning-identity', configured)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_pool_proxy_is_optional_and_adds_only_explicit_proxy_ports(self):
        charts = SCRIPT.parents[1] / 'charts'
        proxy = {
            'httpProxy': 'http://egress.example:18443',
            'httpsProxy': 'https://egress.example:19443',
            'noProxy': 'localhost,127.0.0.1,.svc,artifactory-gateway-internal,artifactory-registry,10.96.0.1',
        }
        for chart, pool in [('artifactory-planning-operator', 'planningPool'),
                            ('artifactory-image-operator', 'builderPool')]:
            command = ['helm', 'template', 'proxy-test', str(charts / chart), '--kube-version', '1.28.3']
            default = subprocess.run(command, check=True, capture_output=True, text=True).stdout
            self.assertNotRegex(default, r'^  proxy:', 'unconfigured pools do not emit an empty proxy')
            with tempfile.TemporaryDirectory() as directory:
                values = Path(directory) / 'values.json'
                values.write_text(json.dumps({pool: {'proxy': proxy}}))
                rendered = subprocess.run(command + ['-f', str(values)], check=True,
                                          capture_output=True, text=True).stdout
            self.assertIn('  proxy:', rendered)
            for key, value in proxy.items():
                self.assertIn(f'{key}: {value}', rendered)
            if pool == 'planningPool':
                policy = next(document for document in rendered.split('---')
                              if 'kind: NetworkPolicy' in document)
                self.assertIn('{port: 18443, protocol: TCP}', policy)
                self.assertIn('{port: 19443, protocol: TCP}', policy)
                self.assertNotIn('egress: [{}]', policy)

    def test_rollout_notes_do_not_contradict_replicated_control_or_variable_pvc_counts(self):
        notes = (SCRIPT.parents[1] / 'charts/artifactory/templates/NOTES.txt').read_text()
        self.assertNotIn('do not scale its Deployment above one replica', notes)
        self.assertNotIn('all three PVCs', notes)
        self.assertIn('.Values.controlPlane.replicas', notes)
        self.assertIn('task emptyDir', notes)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_resource_names_and_internal_references_follow_the_prefix(self):
        charts = SCRIPT.parents[1] / 'charts'

        def resource_names(rendered, kind):
            names = []
            for document in rendered.split('\n---\n'):
                if re.search(rf'^kind: {kind}$', document, re.MULTILINE):
                    name = re.search(r'^metadata:(?: \{ *|\n +)name: ([^\s,}]+)', document, re.MULTILINE)
                    self.assertIsNotNone(name, document)
                    names.append(name.group(1))
            return names

        for prefix in ('artifactory', 'custom'):
            outputs = {}
            for chart in ('artifactory', 'artifactory-planning-operator', 'artifactory-image-operator', 'artifactory-registry'):
                command = ['helm', 'template', 'unrelated-release', str(charts / chart), '--kube-version', '1.28.3']
                if prefix != 'artifactory':
                    command += ['--set', 'fullnameOverride=' + prefix]
                outputs[chart] = subprocess.run(command, check=True, capture_output=True, text=True).stdout
            main = outputs['artifactory']
            for name in ('timescaledb', 'seaweedfs'):
                for kind in ('Deployment', 'Service', 'PersistentVolumeClaim'):
                    self.assertIn(f'{prefix}-{name}', resource_names(main, kind))
            self.assertIn('http://' + prefix + '-seaweedfs:8333', main)
            self.assertIn('http://' + prefix + '-registry:5000', main)
            agent = outputs['artifactory-planning-operator']
            self.assertEqual(resource_names(agent, 'Deployment'), [f'{prefix}-planning-operator'])
            self.assertEqual(resource_names(agent, 'Service'), [])
            self.assertIn('https://' + prefix + '-gateway-internal:8445/internal/planning', agent)
            image = outputs['artifactory-image-operator']
            self.assertEqual(resource_names(image, 'Deployment'), [f'{prefix}-image-operator'])
            self.assertIn('https://' + prefix + '-gateway-internal:8445', image)
            registry = outputs['artifactory-registry']
            self.assertEqual(resource_names(registry, 'Deployment'), [f'{prefix}-registry'])
            self.assertEqual(resource_names(registry, 'Service'), [f'{prefix}-registry'])
            self.assertIn('secretName: ' + prefix + '-registry-signing-public', registry)
            self.assertIn('name: ' + prefix + '-registry-storage', registry)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_stateful_resources_and_selectors_follow_each_release(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        for release in ('first', 'second'):
            rendered = subprocess.run(
                ['helm', 'template', release, str(chart), '--kube-version', '1.28.3'],
                check=True, capture_output=True, text=True).stdout
            documents = rendered.split('\n---\n')
            for component in ('timescaledb', 'seaweedfs'):
                name = 'artifactory-' + component
                resources = {}
                for kind in ('PersistentVolumeClaim', 'Service', 'Deployment'):
                    matches = [doc for doc in documents if f'kind: {kind}\n' in doc
                               and f'  name: {name}\n' in doc]
                    self.assertEqual(len(matches), 1, (release, component, kind))
                    resources[kind] = matches[0]
                service_selector = resources['Service'].split('\n  selector:\n', 1)[1]
                deployment = resources['Deployment']
                match_labels = deployment.split('\n    matchLabels:\n', 1)[1].split('\n  template:', 1)[0]
                pod_labels = deployment.split('\n    metadata:\n      labels:\n', 1)[1].split('\n    spec:', 1)[0]
                labels = {line.strip() for line in service_selector.splitlines() if line.strip()}
                self.assertIn(f'app.kubernetes.io/instance: {release}', labels)
                self.assertIn(f'app.kubernetes.io/component: {component}', labels)
                self.assertEqual(labels, {line.strip() for line in match_labels.splitlines()})
                self.assertTrue(labels <= {line.strip() for line in pod_labels.splitlines()})
                self.assertIn(f'claimName: {name}', deployment)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_planning_operator_and_telemetry_selectors_are_release_scoped(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory-planning-operator'
        for release in ('first', 'second'):
            rendered = subprocess.run(
                ['helm', 'template', release, str(chart), '--set', 'telemetry.enabled=true'],
                check=True, capture_output=True, text=True).stdout
            documents = rendered.split('\n---\n')
            for name in ('planning-operator', 'telemetry'):
                deployment = next(doc for doc in documents if 'kind: Deployment\n' in doc
                                  and f'  name: artifactory-{name}\n' in doc)
                labels = f'app.kubernetes.io/name: artifactory-{name}, app.kubernetes.io/instance: {release}'
                self.assertIn(f'matchLabels: {{{labels}}}', deployment)
                self.assertIn(f'labels: {{{labels}}}', deployment)
                if name == 'telemetry':
                    service = next(doc for doc in documents if 'kind: Service\n' in doc
                                   and '  name: artifactory-telemetry\n' in doc)
                    self.assertIn(f'selector: {{{labels}}}', service)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_long_prefix_is_truncated_consistently_across_charts(self):
        charts = SCRIPT.parents[1] / 'charts'
        chart_settings = (
            ('artifactory', 'timescaledb', []),
            ('artifactory-planning-operator', 'planning-operator', ['--set', 'telemetry.enabled=true']),
            ('artifactory-image-operator', 'image-operator', []),
            ('artifactory-registry', 'registry', []),
        )
        for prefix, truncated in (('a' * 52, 'a' * 46), ('b' * 44 + '--tail', 'b' * 44)):
            for chart, component, settings in chart_settings:
                with self.subTest(prefix=prefix, chart=chart):
                    rendered = subprocess.run(
                        ['helm', 'template', 'other-release', str(charts / chart),
                         '--kube-version', '1.28.3', '--set', 'fullnameOverride=' + prefix, *settings],
                        check=True, capture_output=True, text=True).stdout
                    self.assertIn('name: ' + truncated + '-' + component, rendered)
                    services = set()
                    for document in rendered.split('\n---\n'):
                        if not re.search(r'^kind: Service$', document, re.MULTILINE):
                            continue
                        name = re.search(r'^metadata:(?: \{ *|\n +)name: ([^\s,}]+)',
                                         document, re.MULTILINE)
                        self.assertIsNotNone(name, document)
                        name = name.group(1)
                        self.assertRegex(name, r'^[a-z][a-z0-9-]{0,61}[a-z0-9]$')
                        services.add(name)
                    if chart == 'artifactory':
                        self.assertIn(truncated + '-gateway-internal', services)
                        self.assertIn('http://' + truncated + '-seaweedfs:8333', rendered)
                    if chart in ('artifactory-planning-operator', 'artifactory-image-operator'):
                        self.assertIn('https://' + truncated + '-gateway-internal:8445', rendered)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_trial_profile_is_replicated_private_and_uses_default_priority(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        result = subprocess.run(['helm', 'template', 'artifactory', str(chart),
                                 '--namespace', 'artifactory', '--kube-version', '1.28.3',
                                 '-f', str(chart / 'values-trial.yaml')],
                                check=True, capture_output=True, text=True)
        self.assertIn('replicas: 3', result.stdout)
        self.assertNotIn('local-source', result.stdout)
        self.assertIn('kind: PodDisruptionBudget', result.stdout)
        self.assertNotIn('type: NodePort', result.stdout)
        self.assertNotIn('type: LoadBalancer', result.stdout)
        self.assertNotIn('kind: Ingress\n', result.stdout)
        self.assertIn('resources: [tokenreviews]', result.stdout)
        self.assertNotIn('preemptionPolicy:', result.stdout)
        self.assertNotIn('priorityClassName:', result.stdout)
        self.assertNotIn('kind: PriorityClass', result.stdout)
        version = (SCRIPT.parents[1] / 'VERSION').read_text().strip()
        self.assertIn(f'ghcr.io/zhubby/artifactory-control-plane:{version}', result.stdout)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_replicated_control_uses_temporary_storage_and_renders_ha_resources(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        result = subprocess.run(['helm', 'template', 'ha-test', str(chart), '-f', str(chart / 'values-ha.yaml')],
                                check=True, capture_output=True, text=True)
        self.assertIn('replicas: 3', result.stdout)
        self.assertNotIn('serve-source', result.stdout)
        self.assertIn('emptyDir: {sizeLimit: 10Gi}', result.stdout)
        self.assertIn('kind: PodDisruptionBudget', result.stdout)
        self.assertNotIn('local-source', result.stdout)
        self.assertIn('ARTIFACTORY_SERVICE_CA, value: /run/ca/ca.crt', result.stdout)
        self.assertNotIn('ARTIFACTORY_GATEWAY_SOURCE', result.stdout)
        self.assertIn('ARTIFACTORY_DB_MAX_CONNECTIONS: "40"', result.stdout)
        self.assertIn('path: /readyz', result.stdout)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_gateway_ports_and_independent_image_services(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        result = subprocess.run(['helm', 'template', 'custom', str(chart), '--set',
                                 'fullnameOverride=custom,imageBuilds.enabled=true,ingress.enabled=true'],
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
        services = self.services(rendered)
        for name in ['custom-gateway', 'custom-gateway-internal']:
            self.assertIn(name, set(services))
            service = services[name]
            self.assertIn('    app.kubernetes.io/component: gateway', service.splitlines())
        internal = services['custom-gateway-internal']
        self.assertIn('type: ClusterIP', internal)
        self.assertNotIn('custom-web', set(services))
        self.assertNotIn('custom-grpc', set(services))
        self.assertIn('ARTIFACTORY_GATEWAY_TELEMETRY, value: "custom-telemetry:8444"', deployment)
        self.assertIn('secretKeyRef: {name: custom-gateway, key: token}', rendered)
        self.assertIn('secretName: custom-registry-signing', rendered)
        self.assertIn('ARTIFACTORY_S3_ENDPOINT: "http://custom-seaweedfs:8333"', rendered)
        self.assertIn('ARTIFACTORY_S3_ALLOW_HTTP: "true"', rendered)
        self.assertNotIn('type: NodePort', rendered)
        ingress = rendered.split('# Source: artifactory/templates/ingress.yaml')[1].split('---')[0]
        self.assertNotIn('custom-seaweedfs', ingress)
        self.assertIn('name: custom-gateway\n', ingress)
        notes = (chart / 'templates/NOTES.txt').read_text()
        self.assertIn('-gateway:8443', notes)
        self.assertIn('-gateway:8444', notes)
        self.assertNotIn('-grpc:8443', notes)
        image_chart = SCRIPT.parents[1] / 'charts/artifactory-image-operator'
        image = subprocess.run(['helm', 'template', 'images', str(image_chart)],
                               check=True, capture_output=True, text=True).stdout
        self.assertIn('fieldPath: metadata.namespace', image)
        self.assertNotIn('metadata.uidspace', image)
        self.assertNotIn('resources: [secrets]', image)
        agent = subprocess.run(['helm', 'template', 'native', str(SCRIPT.parents[1] / 'charts/artifactory-planning-operator')],
                               check=True, capture_output=True, text=True).stdout
        self.assertIn('name: artifactory-planning-operator', agent)
        self.assertIn('app.kubernetes.io/name: artifactory-planning-operator', agent)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_gateway_nodeports_and_network_sources_preserve_internal_boundary(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        command = ['helm', 'template', 'boundary', str(chart), '--set',
                   'fullnameOverride=boundary,gateway.service.type=NodePort,gateway.service.nodePorts.http=30820,gateway.service.nodePorts.grpc=30843,gateway.service.nodePorts.telemetry=30844,gateway.internalTlsSecret=service-tls',
                   '--set-json', 'gateway.networkPolicy.publicFrom=[{"ipBlock":{"cidr":"10.169.1.0/24"}}]',
                   '--set-json', 'gateway.networkPolicy.deviceFrom=[{"ipBlock":{"cidr":"10.233.0.0/16"}}]']
        rendered = subprocess.run(command, check=True, capture_output=True, text=True).stdout
        services = self.services(rendered)
        self.assertIn('boundary-gateway', set(services))
        gateway = services['boundary-gateway']
        self.assertIn('type: NodePort', gateway)
        http, control, telemetry = gateway.split('    - port: ')[1:]
        self.assertTrue(http.startswith('80\n'))
        self.assertIn('targetPort: http', http)
        self.assertIn('nodePort: 30820', http)
        self.assertTrue(control.startswith('8443\n'))
        self.assertIn('targetPort: grpc', control)
        self.assertIn('nodePort: 30843', control)
        self.assertTrue(telemetry.startswith('8444\n'))
        self.assertIn('targetPort: telemetry', telemetry)
        self.assertIn('nodePort: 30844', telemetry)
        self.assertNotIn('8445', gateway)
        self.assertIn('type: ClusterIP', services['boundary-gateway-internal'])
        self.assertNotIn('nodePort:', services['boundary-gateway-internal'])
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
        self.assertIn('namespaceSelector:', internal)
        self.assertIn('values: ["default"]', internal)
        for duplicate in ['gateway.service.nodePorts.telemetry=30843',
                          'gateway.service.nodePorts.http=30843',
                          'gateway.service.nodePorts.http=30844']:
            with self.subTest(duplicate=duplicate):
                invalid = subprocess.run(command + ['--set', duplicate], capture_output=True, text=True)
                self.assertNotEqual(invalid.returncode, 0)
                self.assertIn('must differ', invalid.stderr)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_gateway_nodeports_can_be_automatically_allocated(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        for assigned in (False, True):
            with self.subTest(http_explicit=assigned):
                settings = 'gateway.service.type=NodePort'
                if assigned:
                    settings += ',gateway.service.nodePorts.http=30820'
                rendered = subprocess.run(['helm', 'template', 'automatic', str(chart), '--set', settings],
                                          check=True, capture_output=True, text=True).stdout
                services = self.services(rendered)
                self.assertIn('artifactory-gateway', set(services))
                gateway = services['artifactory-gateway']
                self.assertIn('type: NodePort', gateway)
                self.assertEqual(gateway.count('nodePort:'), int(assigned))
                if assigned:
                    self.assertIn('nodePort: 30820', gateway)
                self.assertEqual(len(gateway.split('    - port: ')[1:]), 3)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_clusterip_gateway_rejects_nodeports(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        for port in ('http', 'grpc', 'telemetry'):
            with self.subTest(port=port):
                result = subprocess.run(['helm', 'template', 'private', str(chart), '--set',
                                         f'gateway.service.type=ClusterIP,gateway.service.nodePorts.{port}=30820'],
                                        capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('gateway.service.nodePorts require NodePort or LoadBalancer', result.stderr)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_loadbalancer_gateway_keeps_three_explicit_nodeports(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        rendered = subprocess.run(['helm', 'template', 'loadbalancer', str(chart), '--set',
                                   'gateway.service.type=LoadBalancer,gateway.service.nodePorts.http=30820,gateway.service.nodePorts.grpc=30843,gateway.service.nodePorts.telemetry=30844'],
                                  check=True, capture_output=True, text=True).stdout
        gateway = self.services(rendered)['artifactory-gateway']
        self.assertIn('type: LoadBalancer', gateway)
        self.assertEqual(gateway.count('nodePort:'), 3)
        for port, nodeport in zip(gateway.split('    - port: ')[1:], (30820, 30843, 30844), strict=True):
            self.assertIn('nodePort: ' + str(nodeport), port)
        self.assertNotIn('8445', gateway)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_legacy_public_service_settings_require_explicit_migration(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        command = ['helm', 'template', 'legacy', str(chart)]
        for legacy in ['console.service.type=NodePort,console.service.nodePort=30820',
                       'console.service.type=ClusterIP', 'grpc.service.type=ClusterIP',
                       'grpc.service.type=NodePort,grpc.service.nodePort=30843',
                       'grpc.service.telemetryNodePort=30844',
                       'gateway.service.type=NodePort,console.service.nodePort=30820']:
            with self.subTest(legacy=legacy):
                invalid = subprocess.run(command + ['--set', legacy], capture_output=True, text=True)
                self.assertNotEqual(invalid.returncode, 0)
                self.assertIn('gateway.service', invalid.stderr)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_telemetry_authorization_database_can_use_existing_readonly_identity(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        for key in ('database-url', 'telemetry-business-database-url'):
            with self.subTest(secret_key=key):
                command = ['helm', 'template', 'authorization', str(chart)]
                if key != 'database-url':
                    command += ['--set', 'telemetry.businessDatabaseSecretKey=' + key]
                rendered = subprocess.run(command, check=True, capture_output=True, text=True).stdout
                telemetry = next(document for document in rendered.split('\n---\n')
                                 if 'kind: Deployment\n' in document
                                 and '  name: artifactory-telemetry\n' in document)
                authorization = telemetry.split('- name: ARTIFACTORY_BUSINESS_DATABASE_URL\n', 1)[1].split('\n            -', 1)[0]
                self.assertIn('key: ' + key + '}', authorization)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_source_and_image_namespaces_have_identity_access_and_internal_gateway_route(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        rendered = subprocess.run(['helm', 'template', 'boundary', str(chart), '--namespace', 'control',
                                   '--set', 'planning.namespace=planning,imageBuilds.namespace=images'],
                                  check=True, capture_output=True, text=True).stdout
        policy = next(document for document in rendered.split('---') if 'kind: NetworkPolicy\n' in document)
        self.assertIn('values: ["control","planning","images"]', policy)
        bindings = [document for document in rendered.split('---') if 'kind: RoleBinding\n' in document]
        self.assertTrue(any('namespace: planning\n' in document for document in bindings))
        self.assertTrue(any('namespace: images\n' in document for document in bindings))
        self.assertTrue(all('namespace: control}' in document for document in bindings))

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_telemetry_backend_cannot_publish_a_second_device_entry(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        command = ['helm', 'template', 'boundary', str(chart), '--set', 'fullnameOverride=boundary']
        rendered = subprocess.run(command, check=True, capture_output=True, text=True).stdout
        services = self.services(rendered)
        backend = services['boundary-telemetry']
        self.assertIn('type: ClusterIP', backend)
        self.assertNotIn('nodePort:', backend)
        self.assertIn('name: query, port: 8093, targetPort: query', backend)
        self.assertIn('name: ingest, port: 8444, targetPort: ingest', backend)
        self.assertNotIn('boundary-telemetry-grpc', services)
        for legacy in ['telemetry.service.type=NodePort,telemetry.service.nodePort=30844',
                       'telemetry.service.type=LoadBalancer', 'telemetry.service.nodePort=30844']:
            with self.subTest(legacy=legacy):
                invalid = subprocess.run(command + ['--set', legacy], capture_output=True, text=True)
                self.assertNotEqual(invalid.returncode, 0)
                self.assertIn('gateway.service', invalid.stderr)

    @unittest.skipUnless(shutil.which('helm'), 'requires helm')
    def test_service_set_tracks_optional_telemetry_and_external_storage(self):
        chart = SCRIPT.parents[1] / 'charts/artifactory'
        command = ['helm', 'template', 'services', str(chart)]
        base = {'artifactory-control-plane', 'artifactory-web-internal',
                'artifactory-gateway', 'artifactory-gateway-internal'}
        for enabled in (True, False):
            for storage in (True, False):
                with self.subTest(telemetry=enabled, embedded_storage=storage):
                    settings = ['--set', 'telemetry.enabled=' + str(enabled).lower()]
                    if not storage:
                        settings += ['--set', 'postgres.enabled=false,s3.enabled=false,s3.endpoint=https://objects.example.com']
                    rendered = subprocess.run(command + settings, check=True,
                                              capture_output=True, text=True).stdout
                    expected = base | ({'artifactory-telemetry'} if enabled else set())
                    if storage:
                        expected |= {'artifactory-timescaledb', 'artifactory-seaweedfs'}
                    self.assertEqual(set(self.services(rendered)), expected)

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
        self.assertNotIn('name: artifactory-timescaledb\n', result.stdout)
        self.assertNotIn('name: artifactory-seaweedfs\n', result.stdout)
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
            prefix = 'a' * 52
            truncated = 'a' * 46
            command = [sys.executable, str(SCRIPT), "--output", str(output),
                       "--namespace", "test-builds", "--release", "different-release", "--resource-prefix", prefix,
                       "--grpc-host", "127.0.0.1", "--grpc-host", "agents.example.com"]
            subprocess.run(command, check=True, capture_output=True)
            before = {str(path): path.read_bytes() for path in output.rglob("*") if path.is_file()}
            secret = json.loads((output / "credentials.json").read_text())
            data = secret["stringData"]
            self.assertEqual(secret["metadata"]["namespace"], "test-builds")
            self.assertEqual(data["bootstrap-username"], "admin")
            self.assertEqual(data["bootstrap-password"], "123456")
            self.assertEqual(len(base64.b64decode(data["master-key"])), 32)
            self.assertIn(data["database-password"] + f"@{truncated}-timescaledb:5432/", data["database-url"])
            self.assertIn(f'@{truncated}-timescaledb:5432/', data['telemetry-database-url'])
            self.assertEqual(json.loads((output / "access.json").read_text())["password"],
                             data["bootstrap-password"])
            identity = json.loads(data["s3.json"])["identities"][0]["credentials"][0]
            self.assertEqual(identity["secretKey"], data["s3-secret-key"])
            gateway = json.loads((output / 'gateway.json').read_text())
            self.assertEqual(gateway['metadata']['name'], f'{truncated}-gateway')
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
                            "-verify_hostname", f"{truncated}-gateway-internal", str(output / "pki/server.pem")],
                           check=True, capture_output=True)
            subprocess.run(["openssl", "verify", "-CAfile", str(output / "pki/ca.pem"),
                            "-verify_hostname", f"{truncated}-telemetry", str(output / "pki/server.pem")],
                           check=True, capture_output=True)
            for hostname in (f'{truncated}-gateway', f'{truncated}-gateway.test-builds.svc',
                             f'{truncated}-grpc', f'{truncated}-grpc.test-builds.svc'):
                with self.subTest(hostname=hostname):
                    verified = subprocess.run(['openssl', 'verify', '-CAfile', str(output / 'pki/ca.pem'),
                                               '-verify_hostname', hostname, str(output / 'pki/server.pem')],
                                              capture_output=True)
                    self.assertEqual(verified.returncode, 0, 'certificate SAN missing: ' + hostname)
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
