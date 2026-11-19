"""[INPUT]: 专用 Kubernetes 1.28 及以上 kind、最新 Operator 二进制与 fixture 镜像。
[OUTPUT]: 保存源码重试、规划失败、发布、取消、重启和失联容量证据。
[POS]: 真实普通 Pod 生命周期验收；业务 Git 与 BuildKit 使用独立镜像验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import time
import uuid

from runtime import Runtime


def require_kubernetes_minimum(version):
    match = re.fullmatch(r'v?(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.+-]+)?', version)
    if not match or tuple(map(int, match.group(1, 2))) < (1, 28):
        raise ValueError('This harness requires Kubernetes 1.28 or newer')


def kind_target(context, cluster, nodes, label, pods, namespace):
    if not cluster or context != 'kind-' + cluster or label != cluster:
        raise ValueError('Target must match the explicit dedicated kind context and Docker label')
    if len(nodes) != 1 or nodes[0]['metadata']['name'] != cluster + '-control-plane':
        raise ValueError('Lifecycle faults require a dedicated single-node kind')
    if any(p['metadata']['namespace'] not in (namespace, 'kube-system', 'local-path-storage') for p in pods):
        raise ValueError('Dedicated kind contains unrelated workloads')


def execution_id(code=0):
    value = uuid.uuid4().hex[:20] + format(code, '012x')
    return str(uuid.UUID(value))


def namespace_name(name):
    if not re.fullmatch(r'artifactory-pod-lifecycle-[a-z0-9]([-a-z0-9]*[a-z0-9])?', name) or len(name) > 63:
        raise ValueError('A new artifactory-pod-lifecycle- namespace is required')
    return name


class Harness:
    def __init__(self, args):
        self.args = args
        self.root = Path(__file__).resolve().parents[3]
        self.out = Path(args.output).resolve()
        self.out.mkdir(parents=True, exist_ok=True)
        self.ns = namespace_name(args.namespace or 'artifactory-pod-lifecycle-' + uuid.uuid4().hex[:10])
        self.namespace_uid = None
        self.namespace_nonce = uuid.uuid4().hex
        self.context = 'kind-' + args.cluster
        self.kubeconfig = str(Path(args.kubeconfig).resolve())
        self.node = args.cluster + '-control-plane'
        self.processes = {}
        self.logs = []
        self.runtime = Runtime()
        self.evidence = {'namespace': self.ns, 'context': self.context, 'scenarios': [], 'observed': {}}
        self.frozen_pid = None
        self.kubelet_stopped = False

    def command(self, args, data=None, check=True, timeout=45):
        result = subprocess.run(args, input=data, capture_output=True, text=True, timeout=timeout, cwd=self.root)
        if check and result.returncode:
            raise RuntimeError('Command failed: ' + ' '.join(args[:5]) + '\n' + result.stderr[-2500:])
        return result.stdout if check else result

    def kubectl(self, *args, data=None, check=True, timeout=45):
        return self.command([self.args.kubectl, '--kubeconfig', self.kubeconfig, '--context', self.context,
                             '-n', self.ns, *args], data=data, check=check, timeout=timeout)

    def get(self, kind, name=None):
        args = ['get', kind] + ([name] if name else []) + ['-o', 'json']
        return json.loads(self.kubectl(*args))

    def apply(self, obj):
        obj.setdefault('metadata', {}).setdefault('namespace', self.ns)
        return json.loads(self.kubectl('apply', '-f', '-', '-o', 'json', data=json.dumps(obj)))

    def wait(self, probe, message, seconds=180):
        started = time.monotonic()
        while time.monotonic() - started < seconds:
            for name, process in self.processes.items():
                if process.poll() is not None:
                    raise RuntimeError(name + ' exited; inspect the dedicated operator log')
            value = probe()
            if value:
                return value
            time.sleep(1)
        raise TimeoutError(message)

    def observe(self, kind, name):
        obj = self.get(kind, name)
        status = obj.get('status', {})
        entry = {'status': status, 'uid': obj['metadata']['uid']}
        values = self.evidence['observed'].setdefault(kind + '/' + name, [])
        if not values or values[-1] != entry:
            values.append(entry)
        return obj

    def state(self, kind, name, phases, cleaned=False):
        obj = self.observe(kind, name)
        status = obj.get('status', {})
        if status.get('phase') not in phases:
            return None
        if cleaned and (status.get('execution') or str(status.get('message', '')).startswith('CleanupPending:')):
            return None
        return obj

    def pods(self, selector):
        return json.loads(self.kubectl('get', 'pods', '-l', selector, '-o', 'json'))['items']

    def validate(self):
        label = self.command(['docker', 'inspect', '--format',
                              '{{index .Config.Labels "io.x-k8s.kind.cluster"}}', self.node]).strip()
        actual = self.command([self.args.kubectl, '--kubeconfig', self.kubeconfig,
                               'config', 'current-context']).strip()
        nodes = self.get('nodes')['items']
        pods = json.loads(self.kubectl('get', 'pods', '-A', '-o', 'json'))['items']
        kind_target(actual, self.args.cluster, nodes, label, pods, self.ns)
        version = json.loads(self.kubectl('version', '-o', 'json'))['serverVersion']['gitVersion']
        require_kubernetes_minimum(version)
        self.arch = nodes[0]['metadata']['labels']['kubernetes.io/arch']
        image_arch = self.command(['docker', 'image', 'inspect', self.args.image,
                                   '--format', '{{.Architecture}}']).strip()
        if self.arch != image_arch:
            raise ValueError('Fixture image architecture must match the dedicated node')
        self.evidence.update(kubernetes=version, node=self.node, architecture=self.arch, fixture=self.args.image,
                             fixture_image_id=self.command(['docker', 'image', 'inspect', self.args.image,
                                                            '--format', '{{.Id}}']).strip(),
                             scope='Real Pod lifecycle; fixture processes replace Git, planning and BuildKit business work')

    def setup(self):
        self.validate()
        existing = self.kubectl('get', 'namespace', self.ns, '--ignore-not-found=true', '-o', 'json')
        if existing.strip():
            raise ValueError('Lifecycle namespace already exists')
        namespace = json.loads(self.kubectl('create', '-f', '-', '-o', 'json', data=json.dumps({
            'apiVersion': 'v1', 'kind': 'Namespace', 'metadata': {'name': self.ns,
                'labels': {'tests.artifactory.dev/lifecycle': self.namespace_nonce}}})))
        self.namespace_uid = namespace['metadata']['uid']
        self.evidence['namespace_uid'] = self.namespace_uid
        self.command([self.args.kind, 'load', 'docker-image', self.args.image, '--name', self.args.cluster], timeout=120)
        self.kubectl('apply', '-f', str(self.root / 'operators/agent-operator/config/crd/resources.yaml'))
        self.kubectl('apply', '-f', str(self.root / 'operators/image-operator/config/crd/resources.yaml'))
        self.kubectl('wait', 'crd/agentruns.agents.artifactory.dev', 'crd/sourcetasks.agents.artifactory.dev',
                     'crd/imagebuilds.images.artifactory.dev', '--for=condition=Established', '--timeout=60s')
        for name in ('artifactory-pi', 'artifactory-source-preparer', 'artifactory-image-worker'):
            self.apply({'apiVersion': 'v1', 'kind': 'ServiceAccount', 'metadata': {'name': name}})
        agent = {'maxActive': 1, 'maxPerProject': 1, 'image': self.args.image, 'sourcePreparerImage': self.args.image,
                 'imagePullPolicy': 'IfNotPresent', 'runtimeUrl': self.runtime.url, 'controlUrl': self.runtime.url,
                 'ephemeralStorage': '32Mi', 'cpu': '100m', 'memory': '64Mi'}
        self.apply({'apiVersion': 'agents.artifactory.dev/v1alpha1', 'kind': 'AgentPool',
                    'metadata': {'name': 'lifecycle'}, 'spec': agent})
        budget = {'cpu': '100m', 'memory': '64Mi', 'ephemeralStorage': '32Mi'}
        image = {'workerImage': self.args.image, 'buildkitImage': self.args.image, 'sourcePreparerImage': self.args.image,
                 'controlUrl': self.runtime.url, 'imagePullPolicy': 'IfNotPresent', 'maxActive': 1, 'maxPerProject': 1,
                 'resources': {role: dict(budget) for role in ('source', 'runner', 'buildkit', 'publisher')}}
        self.apply({'apiVersion': 'images.artifactory.dev/v1alpha1', 'kind': 'BuilderPool',
                    'metadata': {'name': 'lifecycle'}, 'spec': image})
        self.start('agent')
        self.start('image')

    def cleanup_namespace(self):
        if self.namespace_uid is None:
            raise ValueError('Namespace was not created by this harness')
        namespace = self.get('namespace', self.ns)
        metadata = namespace['metadata']
        if metadata['uid'] != self.namespace_uid or metadata.get('labels', {}).get('tests.artifactory.dev/lifecycle') != self.namespace_nonce:
            raise ValueError('Namespace ownership changed')
        # 先收尾执行 CR；直接删 namespace 会与其中的选主 Lease 竞争。
        self.kubectl('delete', 'agentruns,sourcetasks,imagebuilds', '--all', '--wait=false')
        self.wait(lambda: not self.get('agentruns,sourcetasks,imagebuilds')['items'],
                  'Execution finalizers did not finish before namespace deletion')
        self.kubectl('delete', '--raw', '/api/v1/namespaces/' + self.ns, '-f', '-', data=json.dumps({
            'apiVersion': 'v1', 'kind': 'DeleteOptions', 'preconditions': {'uid': self.namespace_uid}}))
        self.wait(lambda: not self.kubectl('get', 'namespace', self.ns, '--ignore-not-found=true', '-o', 'name').strip(),
                  'Owned lifecycle namespace did not disappear')
        self.evidence['namespace_deleted'] = True

    def terminal_finalizers(self):
        resources = self.get('agentruns,sourcetasks,imagebuilds')['items']
        protected = {'agents.artifactory.dev/execution-cleanup', 'images.artifactory.dev/cleanup'}
        for resource in resources:
            if resource.get('status', {}).get('phase') in ('Succeeded', 'Failed', 'Canceled', 'failed', 'canceled'):
                if protected.intersection(resource['metadata'].get('finalizers', [])):
                    return False
        return True

    def start(self, operator):
        env = dict(os.environ, KUBECONFIG=self.kubeconfig, ARTIFACTORY_NAMESPACE=self.ns,
                   ARTIFACTORY_RUNTIME_URL=self.runtime.url, ARTIFACTORY_SERVICE_TOKEN=self.runtime.token,
                   ARTIFACTORY_INSECURE_DEV='true', POD_UID=self.ns + '-' + operator, RUST_LOG='info')
        log = (self.out / (operator + '-' + str(time.time_ns()) + '.log')).open('w')
        self.logs.append(log)
        binary = self.root / 'target/debug' / ('artifactory-' + operator + '-operator')
        with binary.open('rb') as source:
            digest = hashlib.file_digest(source, 'sha256').hexdigest()
        self.processes[operator] = subprocess.Popen([str(binary)],
                                                     env=env, stdout=log, stderr=subprocess.STDOUT, cwd=self.root)
        self.evidence.setdefault('operator_binaries', []).append({'operator': operator, 'sha256': digest,
                                                                  'pid': self.processes[operator].pid,
                                                                  'started_at_unix': time.time()})

    def stop(self, operator):
        process = self.processes.pop(operator)
        process.terminate()
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)

    def source(self):
        name = 'source-retry'
        self.apply({'apiVersion': 'agents.artifactory.dev/v1alpha1', 'kind': 'SourceTask', 'metadata': {'name': name},
                    'spec': {'taskId': execution_id(), 'tenantId': str(uuid.uuid4()), 'projectId': str(uuid.uuid4()),
                             'purpose': 'resolve', 'pool': 'lifecycle', 'timeoutSeconds': 240}})
        seen = {}

        def complete():
            obj = self.observe('sourcetask', name)
            execution = obj.get('status', {}).get('execution')
            if execution and execution.get('uid'):
                seen[execution['attempt']] = dict(execution)
            return obj if obj.get('status', {}).get('phase') == 'Succeeded' and not execution else None

        self.wait(complete, 'SourceTask did not succeed after explicit retry', 270)
        assert set(seen) == {1, 2}, seen
        assert seen[1]['uid'] != seen[2]['uid'] and seen[1]['deadlineMs'] == seen[2]['deadlineMs']
        assert not self.pods('artifactory.dev/source-task=' + name)
        self.evidence['scenarios'].append({'name': 'source-explicit-retry', 'passed': True, 'attempts': seen})

    def planning(self, code, source=False):
        run_id = execution_id(code)
        name = 'planning-' + str(code)
        self.runtime.add(run_id)
        spec = {'runId': run_id, 'projectId': str(uuid.uuid4()), 'generation': 1, 'pool': 'lifecycle'}
        if source:
            spec.update(sourceManifestId=str(uuid.uuid4()), sourceDigest='a' * 64)
        self.apply({'apiVersion': 'agents.artifactory.dev/v1alpha1', 'kind': 'AgentRun',
                    'metadata': {'name': name}, 'spec': spec})
        self.wait(lambda: self.state('agentrun', name, ['failed'], cleaned=True), 'Planning failure was not reported')
        events = [e for e in self.runtime.snapshot() if e['path'] == '/v1/execution-failed'
                  and e['body']['execution']['run_id'] == run_id]
        assert events, 'Planning Pod failure did not reach Runtime'
        identity = events[0]['body']['execution']
        registered = [e['body'] for e in self.runtime.snapshot() if e['path'] == '/v1/register' and e['body'] == identity]
        assert registered and identity['uid'] and identity['namespace'] == self.ns
        assert not self.pods('artifactory.dev/run=' + run_id)
        self.evidence['scenarios'].append({'name': 'planning-init-failure' if source else 'planning-main-failure',
                                            'passed': True, 'failure': events[0]['body']})

    def planning_restart(self):
        run_id = execution_id(2)
        name = 'planning-restart'
        self.runtime.add(run_id)
        self.apply({'apiVersion': 'agents.artifactory.dev/v1alpha1', 'kind': 'AgentRun',
                    'metadata': {'name': name}, 'spec': {'runId': run_id, 'projectId': str(uuid.uuid4()),
                                                       'generation': 1, 'pool': 'lifecycle'}})

        def running():
            obj = self.observe('agentrun', name)
            ledger = obj.get('status', {}).get('execution') or {}
            pods = self.pods('artifactory.dev/run=' + run_id)
            if len(pods) == 1 and pods[0].get('status', {}).get('phase') == 'Running' and ledger.get('uid'):
                assert pods[0]['metadata']['uid'] == ledger['uid']
                return pods[0]
            return None

        pod = self.wait(running, 'Planning fixture did not start')
        identity = self.pod_evidence(pod)
        self.stop('agent')
        self.start('agent')
        for _ in range(5):
            pods = self.pods('artifactory.dev/run=' + run_id)
            assert len(pods) == 1 and pods[0]['metadata']['uid'] == identity['uid']
            time.sleep(2)
        self.runtime.cancel(run_id)
        self.wait(lambda: self.state('agentrun', name, ['canceled'], cleaned=True),
                  'Planning cancellation did not clean up the recovered execution')
        assert not self.pods('artifactory.dev/run=' + run_id)
        self.evidence['scenarios'].append({'name': 'planning-operator-restart-and-cancel', 'passed': True,
                                            'preserved_pod': identity})

    def source_cancel(self):
        name = 'source-cancel'
        self.apply({'apiVersion': 'agents.artifactory.dev/v1alpha1', 'kind': 'SourceTask', 'metadata': {'name': name},
                    'spec': {'taskId': execution_id(4), 'tenantId': str(uuid.uuid4()), 'projectId': str(uuid.uuid4()),
                             'purpose': 'resolve', 'pool': 'lifecycle', 'timeoutSeconds': 700}})

        def running():
            obj = self.observe('sourcetask', name)
            ledger = obj.get('status', {}).get('execution') or {}
            pods = self.pods('artifactory.dev/source-task=' + name)
            if len(pods) == 1 and pods[0].get('status', {}).get('phase') == 'Running' and ledger.get('uid'):
                return self.pod_evidence(pods[0])
            return None

        identity = self.wait(running, 'Source fixture did not start')
        self.kubectl('patch', 'sourcetask', name, '--type=merge', '-p', json.dumps({'spec': {'cancelRequested': True}}))
        self.wait(lambda: self.state('sourcetask', name, ['Canceled'], cleaned=True), 'Source cancellation did not clean up')
        assert not self.pods('artifactory.dev/source-task=' + name)
        self.evidence['scenarios'].append({'name': 'source-cancel', 'passed': True, 'pod': identity})

    def planning_pull_cancel(self):
        run_id, name = execution_id(2), 'planning-pull-cancel'
        self.runtime.add(run_id)
        self.kubectl('patch', 'agentpool', 'lifecycle', '--type=merge', '-p',
                     json.dumps({'spec': {'image': '127.0.0.1:1/fixture:missing'}}))
        self.apply({'apiVersion': 'agents.artifactory.dev/v1alpha1', 'kind': 'AgentRun',
                    'metadata': {'name': name}, 'spec': {'runId': run_id, 'projectId': str(uuid.uuid4()),
                                                       'generation': 1, 'pool': 'lifecycle'}})
        try:
            def pull_failed():
                self.observe('agentrun', name)
                pods = self.pods('artifactory.dev/run=' + run_id)
                for pod in pods:
                    states = pod.get('status', {}).get('containerStatuses', [])
                    if pod.get('spec', {}).get('nodeName') and any(
                            c.get('state', {}).get('waiting', {}).get('reason') in ('ErrImagePull', 'ImagePullBackOff')
                            and not c.get('containerID') for c in states):
                        return self.pod_evidence(pod)
                return None
            identity = self.wait(pull_failed, 'Scheduled Pod did not reach the expected pull failure')
            self.runtime.cancel(run_id)
            self.wait(lambda: self.state('agentrun', name, ['canceled'], cleaned=True),
                      'Scheduled never-started Pod did not confirm stop after cancellation', 100)
            assert not self.pods('artifactory.dev/run=' + run_id)
            self.evidence['scenarios'].append({'name': 'planning-scheduled-pull-failure-cancel', 'passed': True,
                                                'pod_before_cancel': identity})
        finally:
            self.kubectl('patch', 'agentpool', 'lifecycle', '--type=merge', '-p',
                         json.dumps({'spec': {'image': self.args.image}}))

    def image(self, name, seconds=3):
        execution = execution_id()
        self.apply({'apiVersion': 'v1', 'kind': 'Secret', 'metadata': {'name': name},
                    'stringData': {'build.json': json.dumps({'sleepSeconds': seconds}),
                                   'publish.json': json.dumps({'sleepSeconds': 4})}})
        return self.apply({'apiVersion': 'images.artifactory.dev/v1alpha1', 'kind': 'ImageBuild',
                           'metadata': {'name': name}, 'spec': {
                               'executionId': execution, 'tenantId': str(uuid.uuid4()), 'projectId': str(uuid.uuid4()),
                               'pool': 'lifecycle', 'sourceManifestId': str(uuid.uuid4()), 'sourceDigest': 'a' * 64,
                               'taskSecret': name, 'platforms': ['linux/' + self.arch], 'allowEmulation': False,
                               'publishOnly': False, 'timeoutSeconds': 750}})

    def image_success(self):
        name = 'image-success'
        build = self.image(name)
        seen = {}

        def complete():
            obj = self.observe('imagebuild', name)
            for pod in self.pods('artifactory.dev/image-build=' + name):
                seen[pod['metadata']['name']] = self.pod_evidence(pod)
            return obj if obj.get('status', {}).get('phase') == 'Succeeded' and not seen_pending(obj) else None

        self.wait(complete, 'ImageBuild did not publish after its build Pod succeeded')
        assert any(p['name'].endswith('-publish') for p in seen.values()), seen
        assert any(set(p['containers']) == {'worker', 'buildkit'} for p in seen.values())
        assert all(p['owner'][0]['uid'] == build['metadata']['uid'] for p in seen.values())
        assert not self.pods('artifactory.dev/image-build=' + name)
        self.evidence['scenarios'].append({'name': 'image-publish-after-success', 'passed': True, 'pods': list(seen.values())})

    @staticmethod
    def pod_evidence(pod):
        status = pod.get('status', {})
        return {'name': pod['metadata']['name'], 'uid': pod['metadata']['uid'], 'phase': status.get('phase'),
                'node': pod.get('spec', {}).get('nodeName'), 'owner': pod['metadata'].get('ownerReferences', []),
                'containers': [c['name'] for c in pod['spec']['containers']],
                'init': [c['name'] for c in pod['spec'].get('initContainers', [])],
                'states': {c['name']: c.get('state') for c in status.get('containerStatuses', [])}}

    def running_image(self, name):
        def running():
            obj = self.observe('imagebuild', name)
            pods = self.pods('artifactory.dev/image-build=' + name)
            for pod in pods:
                containers = pod.get('status', {}).get('containerStatuses', [])
                if len(containers) == 2 and all(c.get('state', {}).get('running') for c in containers):
                    ledger = obj.get('status', {}).get('podOwnership', {}).get('pods', [])
                    if any(r['uid'] == pod['metadata']['uid'] for r in ledger):
                        return pod
            return None
        return self.wait(running, 'Image fixture did not start both regular main containers')

    def cancel(self, name):
        self.kubectl('patch', 'imagebuild', name, '--type=merge', '-p', json.dumps({'spec': {'cancelRequested': True}}))

    def image_cancel_restart(self):
        name = 'image-restart-cancel'
        self.image(name, 600)
        pod = self.running_image(name)
        before = self.pod_evidence(pod)
        self.stop('image')
        time.sleep(2)
        self.start('image')
        for _ in range(5):
            self.observe('imagebuild', name)
            pods = self.pods('artifactory.dev/image-build=' + name)
            assert len(pods) == 1 and pods[0]['metadata']['uid'] == before['uid']
            time.sleep(2)
        self.cancel(name)
        self.wait(lambda: self.state('imagebuild', name, ['Canceled']) and
                  not self.pods('artifactory.dev/image-build=' + name), 'Canceled image Pod did not stop and clean up')
        self.evidence['scenarios'].append({'name': 'image-operator-restart-and-cancel', 'passed': True, 'preserved_pod': before})

    def docker(self, *args, check=True):
        return self.command(['docker', 'exec', self.node, *args], check=check)

    def restore_node(self):
        failure = None
        if self.frozen_pid is not None:
            try:
                self.docker('kill', '-CONT', str(self.frozen_pid))
                self.frozen_pid = None
            except Exception as error:
                failure = error
        if self.kubelet_stopped:
            self.docker('systemctl', 'start', 'kubelet')
            self.kubectl('wait', 'node/' + self.node, '--for=condition=Ready', '--timeout=120s', timeout=130)
            self.kubelet_stopped = False
        if failure:
            raise failure

    def node_failure(self):
        self.validate()
        name, probe = 'image-node-lost', 'image-capacity-probe'
        self.image(name, 600)
        pod = self.running_image(name)
        uid = pod['metadata']['uid']
        worker = next(c for c in pod['status']['containerStatuses'] if c['name'] == 'worker')
        container = worker['containerID'].split('://', 1)[1]
        inspected = json.loads(self.docker('crictl', 'inspect', container))
        cri = inspected['status']
        if cri['id'] != container or cri.get('labels', {}).get('io.kubernetes.pod.uid') != uid or cri['state'] != 'CONTAINER_RUNNING':
            raise ValueError('CRI identity differs from the selected live fixture Pod')
        pid = inspected['info']['pid']
        if not isinstance(pid, int) or pid <= 1:
            raise ValueError('Invalid fixture container PID')
        self.frozen_pid = pid
        self.docker('kill', '-STOP', str(pid))
        self.kubelet_stopped = True
        self.docker('systemctl', 'stop', 'kubelet')
        health = self.kubectl('get', '--raw', '/api/v1/nodes/' + self.node + '/proxy/healthz',
                              '--request-timeout=5s', check=False, timeout=10)
        assert health.returncode != 0, 'Kubelet remained reachable'
        self.cancel(name)
        self.image(probe, 600)
        samples = []
        try:
            self.wait(lambda: self.state('imagebuild', name, ['Canceled']), 'Canceled lost-node build was not reconciled')
            for _ in range(7):
                current = self.observe('imagebuild', name)
                physical = self.get('pod', pod['metadata']['name'])
                ledger = current['status']['podOwnership']['pods']
                assert any(r['uid'] == uid and not r['stopped'] for r in ledger)
                assert physical['metadata']['uid'] == uid and physical['metadata'].get('deletionTimestamp')
                assert not self.pods('artifactory.dev/image-build=' + probe), 'Lost-node Pod released capacity'
                candidate = self.observe('imagebuild', probe)
                state = self.docker('cat', '/proc/' + str(pid) + '/stat').split(') ', 1)[1].split()[0]
                assert state == 'T', 'Fixture runner is not physically frozen'
                samples.append({'uid': uid, 'ledger_retained': True, 'deleting': True, 'runner_state': state, 'probe_pods': 0,
                                'probe_phase': candidate.get('status', {}).get('phase'),
                                'probe_observed_generation': candidate.get('status', {}).get('observedGeneration')})
                time.sleep(5)
            assert any(s['probe_observed_generation'] == 1 for s in samples), 'Capacity probe was never reconciled'
        finally:
            try:
                self.kubectl('delete', 'imagebuild', probe, '--ignore-not-found=true', '--wait=false')
            finally:
                self.restore_node()
        self.wait(lambda: self.state('imagebuild', name, ['Canceled']) and
                  not self.pods('artifactory.dev/image-build=' + name), 'Recovered node did not confirm cleanup', 150)
        self.evidence['scenarios'].append({'name': 'node-unreachable-capacity-and-recovery', 'passed': True,
                                            'kubelet_unreachable': True, 'samples': samples, 'kubelet_restored': True})

    def finish(self):
        failures = []
        try:
            self.restore_node()
        except Exception as error:
            failures.append('node restoration: ' + str(error))
        try:
            self.evidence['runtime_events'] = self.runtime.snapshot()
        except Exception as error:
            failures.append('Runtime snapshot: ' + str(error))
        for operator in list(self.processes):
            try:
                self.stop(operator)
            except Exception as error:
                failures.append(operator + ' stop: ' + str(error))
        try:
            self.runtime.close()
        except Exception as error:
            failures.append('Runtime stop: ' + str(error))
        for log in self.logs:
            try:
                log.close()
            except Exception as error:
                failures.append('log close: ' + str(error))
        if failures:
            self.evidence['cleanup_failures'] = failures
            self.evidence['recovery_identity'] = {'node': self.node, 'frozen_pid': self.frozen_pid,
                                                  'kubelet_stopped': self.kubelet_stopped}
        # 恢复失败仍保存现场；每个 helper 的退出互不阻塞。
        (self.out / 'evidence.json').write_text(json.dumps(self.evidence, indent=2) + '\n')
        if failures:
            raise RuntimeError('Lifecycle harness cleanup failed; inspect evidence.json')


def seen_pending(obj):
    return str(obj.get('status', {}).get('message', '')).startswith('CleanupPending:')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cluster', required=True)
    parser.add_argument('--kubeconfig', required=True)
    parser.add_argument('--kubectl', default='kubectl')
    parser.add_argument('--kind', default='kind')
    parser.add_argument('--namespace')
    parser.add_argument('--image', default='artifactory-pod-lifecycle:k128')
    parser.add_argument('--output', default='.deploy/direct-pods-k128')
    parser.add_argument('--scenarios', nargs='+', choices=['source', 'planning', 'planning-init', 'planning-restart',
                                                         'source-cancel', 'image', 'image-restart', 'node', 'pull-cancel'])
    parser.add_argument('--assert-terminal-finalizers', action='store_true')
    args = parser.parse_args()
    harness = Harness(args)
    try:
        harness.setup()
        scenarios = {'source': harness.source, 'planning': lambda: harness.planning(1),
                     'planning-init': lambda: harness.planning(3, True), 'planning-restart': harness.planning_restart,
                     'source-cancel': harness.source_cancel, 'image': harness.image_success,
                     'image-restart': harness.image_cancel_restart, 'node': harness.node_failure,
                     'pull-cancel': harness.planning_pull_cancel}
        for key in args.scenarios or scenarios:
            scenario = scenarios[key]
            scenario()
            if args.assert_terminal_finalizers:
                harness.wait(harness.terminal_finalizers, 'Clean terminal CR retained its execution finalizer', 45)
                time.sleep(2)
                assert harness.terminal_finalizers(), 'Terminal CR reinstalled its execution finalizer'
                harness.evidence['terminal_finalizers_released'] = True
            print('PASS ' + harness.evidence['scenarios'][-1]['name'], flush=True)
        assert not harness.get('jobs')['items'], 'Direct Pod execution unexpectedly created a Job'
        harness.evidence['no_jobs'] = True
        harness.cleanup_namespace()
    except Exception as error:
        harness.evidence['failure'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        harness.finish()


if __name__ == '__main__':
    main()
