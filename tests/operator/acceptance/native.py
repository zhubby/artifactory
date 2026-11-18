#!/usr/bin/env python3
"""[INPUT]: 隔离命名空间、精确节点出口策略和主机原生 Rust 工具链。
[OUTPUT]: 验证主机构建；恢复出口、设备与公告地址后写入成功证据。
[POS]: 直拉共同可达的 Git NodePort；出口只覆盖本次节点和端口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
from contextlib import ExitStack, contextmanager
from datetime import datetime, timedelta, timezone
import hashlib
import importlib.util
import ipaddress
import json
import os
from pathlib import Path
import platform
import re
import shutil
import ssl
import subprocess
import time
import uuid
from urllib.parse import urlsplit, urlunsplit
import urllib.parse
import urllib.request
import base64

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('operator_acceptance', Path(__file__).with_name('run.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def environment(a, deployment):
    value = json.loads(a.kubectl('get', 'deployment', deployment, '-o', 'json'))
    return {item['name']: item.get('value') for item in value['spec']['template']['spec']['containers'][0].get('env', [])}


def set_environment(a, deployment, values):
    before = json.loads(a.kubectl('get', 'deployment', deployment, '-o', 'json'))
    selector = ','.join(key + '=' + value for key, value in sorted(before['spec']['selector']['matchLabels'].items()))
    previous = json.loads(a.kubectl('get', 'pods', '-l', selector, '-o', 'json'))['items']
    previous_uids = {pod['metadata']['uid'] for pod in previous}
    a.kubectl('set', 'env', 'deployment/' + deployment, *[key + ('-' if value is None else '=' + value) for key, value in values.items()])
    a.kubectl('rollout', 'status', 'deployment/' + deployment, '--timeout=180s')
    after = json.loads(a.kubectl('get', 'deployment', deployment, '-o', 'json'))
    if before['spec']['template'] == after['spec']['template']:
        return

    # rollout 完成时，旧 Pod 仍能在排空期接受网关复用连接。
    def previous_pods_gone():
        pods = json.loads(a.kubectl('get', 'pods', '-l', selector, '-o', 'json'))['items']
        return previous_uids.isdisjoint(pod['metadata']['uid'] for pod in pods)

    a.wait(previous_pods_gone, 180)


def drain_existing_devices(a):
    with ExitStack() as restore:
        offset = 0
        while True:
            nodes = a.api(f'/api/v1/nodes?limit=500&offset={offset}')
            for node in nodes:
                if node['revoked'] or node['draining']:
                    continue
                path = '/api/v1/nodes/' + node['id']
                original = {key: node[key] for key in
                            ('draining', 'revoked', 'slots', 'labels', 'signing_profiles', 'pool_id')}
                # 先登记恢复动作，覆盖服务端已写入但响应中断的情况。
                restore.callback(a.api, path, original, method='PATCH')
                a.api(path, dict(original, draining=True), method='PATCH')
            if len(nodes) < 500:
                return restore.pop_all()
            offset += len(nodes)


@contextmanager
def native_git_egress(a, node_ip, port):
    address = ipaddress.ip_address(node_ip)
    if not isinstance(port, int) or not 1 <= port <= 65535:
        raise ValueError('Git NodePort must be a TCP port')
    name = 'native-git-egress-' + uuid.uuid4().hex[:8]
    policy = {
        'apiVersion': 'networking.k8s.io/v1', 'kind': 'NetworkPolicy',
        'metadata': {'name': name, 'namespace': a.ns},
        'spec': {
            'podSelector': {'matchExpressions': [{
                'key': 'app.kubernetes.io/name', 'operator': 'In',
                'values': ['artifactory-pi', 'artifactory-source-task'],
            }]},
            'policyTypes': ['Egress'],
            'egress': [{
                'to': [{'ipBlock': {'cidr': f'{address}/{address.max_prefixlen}'}}],
                'ports': [{'protocol': 'TCP', 'port': port}],
            }],
        },
    }
    try:
        # apply 已写入但响应中断时，仍删除临时策略。
        a.apply(policy)
        yield name
    finally:
        a.kubectl('delete', 'networkpolicy', name, '--ignore-not-found')


def native_git_ready(a, git_origin):
    ca = (a.run / 'pki/ca.pem').read_text()
    endpoint = git_origin + '/info/refs?service=git-upload-pack'
    http = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=ssl.create_default_context(cadata=ca)))

    def host_refs():
        with http.open(endpoint, timeout=3) as response:
            return response.status == 200 and b'# service=git-upload-pack' in response.read(256)

    a.wait(host_refs, 60)
    pool = json.loads(a.kubectl('get', 'agentpool', 'default', '-o', 'json'))['spec']
    name = 'native-git-ready-' + uuid.uuid4().hex[:8]
    script = '''
const https = require("node:https");
const endpoint = __ENDPOINT__;
const ca = __CA__;
const deadline = Date.now() + 45000;
let attempts = 0;
function probe() {
  attempts++;
  let done = false;
  function finish(connected) {
    if (done) return;
    done = true;
    if (!connected && Date.now() < deadline) return setTimeout(probe, 1000);
    console.log(JSON.stringify({connected, attempts}));
    process.exit(connected ? 0 : 1);
  }
  const request = https.get(endpoint, {ca}, response => {
    let body = "";
    response.on("data", chunk => body += chunk);
    response.on("end", () => finish(response.statusCode === 200 && body.includes("# service=git-upload-pack")));
    response.on("error", () => finish(false));
  });
  request.setTimeout(3000, () => request.destroy());
  request.on("error", () => finish(false));
}
probe();
'''.replace('__ENDPOINT__', json.dumps(endpoint)).replace('__CA__', json.dumps(ca))
    pod = {
        'apiVersion': 'v1', 'kind': 'Pod',
        'metadata': {'name': name, 'namespace': a.ns,
                     'labels': {'app.kubernetes.io/name': 'artifactory-source-task'}},
        'spec': {'restartPolicy': 'Never', 'automountServiceAccountToken': False,
                 'containers': [{'name': 'probe', 'image': pool['image'],
                                 'imagePullPolicy': pool.get('imagePullPolicy', 'IfNotPresent'),
                                 'command': ['node', '-e', script]}]},
    }
    try:
        a.apply(pod)
        a.wait(lambda: json.loads(a.kubectl('get', 'pod', name, '-o', 'json'))['status']['phase']
               in ['Succeeded', 'Failed'], 90)
        result = json.loads(a.kubectl('logs', name).strip())
        assert result['connected'], 'Restricted Pod must read Git refs before creating a build'
        return {'host_refs': True, 'restricted_pod_refs': True, 'restricted_attempts': result['attempts']}
    finally:
        a.kubectl('delete', 'pod', name, '--ignore-not-found')


def diagnostic_error(value):
    if not isinstance(value, str):
        return None
    value = re.sub(r"\b[a-zA-Z][a-zA-Z0-9+.-]*://[^\s<>\"']+", '[redacted URL]', value)
    return re.sub(r'(?i)\b(?:Bearer|Basic)\s+\S+', '[redacted authorization]', value)


def host_build_terminal(a, build_id, directory, evidence):
    value = a.api('/api/v1/builds/' + build_id)
    diagnostic = {key: value.get(key) for key in ('id', 'status')}
    diagnostic['error'] = diagnostic_error(value.get('error'))
    diagnostic['jobs'] = [
        {'target': job.get('target'), 'status': job.get('status'), 'attempts': [
            {**{key: attempt.get(key) for key in
                ('id', 'status', 'node_id', 'node_name', 'failure_kind')},
             'error': diagnostic_error(attempt.get('error'))}
            for attempt in job.get('attempts', [])]}
        for job in value.get('jobs', [])]
    (directory / 'build.json').write_text(json.dumps(diagnostic, indent=2))
    if value['status'] in ('failed', 'lost', 'canceled', 'needs_input'):
        evidence['failed_build'] = diagnostic
        (directory / 'evidence.json').write_text(json.dumps(evidence, indent=2))
        raise AssertionError('Host build failed: ' + json.dumps(diagnostic))
    return value if value['status'] == 'succeeded' else None


def run(args):
    a = module.Acceptance(args.context, args.namespace, resume=True)
    directory = ROOT / '.local/operator-load' / (time.strftime('%Y%m%d-%H%M%S') + '-native')
    directory.mkdir(parents=True)
    host_os = {'Darwin': 'darwin', 'Linux': 'linux', 'Windows': 'windows'}[platform.system()]
    arch = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'amd64', 'AMD64': 'amd64'}[platform.machine()]
    if host_os == 'windows':
        raise ValueError('This fixture uses sh; native Windows requires its own repository/toolchain acceptance')
    agent = directory / 'artifactory-emissary'
    config = directory / 'emissary.json'
    process = None
    build = None
    node = None
    restore_devices = ExitStack()
    restore_network = ExitStack()
    agent_pool = json.loads(a.kubectl('get', 'agentpool', 'default', '-o', 'json'))['spec']
    original_control = environment(a, 'control-plane')
    original_upstream = environment(a, 'upstream')
    evidence = {'context': args.context, 'namespace': args.namespace, 'platform': host_os + '/' + arch, 'host_process': True, 'system_service_installed': False, 'passed': False}
    try:
        a.resume()
        assert a.sql('planner', "SELECT count(*) FROM runtime_runs WHERE status IN ('running','provisioning')") == '0', 'Drain namespace before changing advertised routes'
        assert a.sql('business', "SELECT count(*) FROM attempts WHERE status IN ('leased','running')") == '0', 'Drain device jobs before changing routes'
        a.kubectl('patch', 'agentpool', 'default', '--type=merge', '-p', '{"spec":{"maxActive":2}}')
        set_environment(a, 'upstream', {'FIXTURE_OS': host_os, 'FIXTURE_ARCH': arch})
        set_environment(a, 'control-plane', {'ARTIFACTORY_PUBLIC_URL': a.base})
        for forward in a.processes:
            forward.terminate()
            forward.wait(timeout=10)
        a.processes.clear()
        a.resume()
        restore_devices.enter_context(drain_existing_devices(a))
        a.forward('service/control-plane', '38443:8443')
        a.forward('service/artifactory-telemetry','38444:8444')
        a.wait(lambda: a.service_request(39443,'/test/mode',{'build_seconds':25}) is not None)
        subprocess.run(['cargo', 'build', '--locked', '-p', 'artifactory-emissary', '--bin', 'artifactory-emissary'], cwd=ROOT, check=True)
        shutil.copy2(ROOT/'target/debug/artifactory-emissary', agent)
        name = 'operator-host-native-' + uuid.uuid4().hex[:8]
        token = a.api('/api/v1/enrollment-tokens', {'pool_id': '00000000-0000-0000-0000-000000000001'})['token']
        with (directory/'register.log').open('wb') as log:
            subprocess.run([str(agent), 'register', '--config', str(config), '--endpoint', 'localhost:38443', '--ca', str(a.run/'pki/ca.pem'), '--root', str(directory/'root'), '--name', name, '--allow-http-uploads'], env=dict(os.environ, ARTIFACTORY_ENROLLMENT_TOKEN=token), stdout=log, stderr=log, check=True)
        value = json.loads(config.read_text())
        value['git_ca_file'] = str(a.run/'pki/ca.pem')
        value['telemetry_endpoint']='https://localhost:38444'
        config.write_text(json.dumps(value))
        with (directory/'emissary.log').open('wb') as log:
            process = subprocess.Popen([str(agent), 'run', '--config', str(config)], stdout=log, stderr=log)
        node = a.wait(lambda: next((n for n in a.api('/api/v1/nodes') if n['name'] == name and n['online']), None))
        evidence['node_id'] = node['id']
        (directory / 'evidence.json').write_text(json.dumps(evidence, indent=2))
        service = a.service('upstream-native', [9443])
        service['spec'].update(type='NodePort',selector={'app':'upstream'})
        a.apply(service)
        port = json.loads(a.kubectl('get','service','upstream-native','-o','json'))['spec']['ports'][0]['nodePort']
        restore_network.enter_context(native_git_egress(a, a.node_ip, port))
        git_origin = f'https://{a.node_ip}:{port}/repo.git'
        evidence['git_readiness'] = native_git_ready(a, git_origin)
        project = a.api('/api/v1/projects', {'name': name, 'repository_url': git_origin,})['id']
        build = a.api('/api/v1/projects/' + project + '/builds', {'ref': 'v-fixture', 'goal': 'Inspect source and build dist/result.txt on the host-native device', 'target_requirements': [{'os': host_os, 'arch': arch}]})['id']
        evidence['build_id'] = build
        (directory / 'evidence.json').write_text(json.dumps(evidence, indent=2))
        detail = a.wait(lambda: host_build_terminal(a, build, directory, evidence), 600)
        manifest_id = detail['source_manifest_id']
        manifest = json.loads(a.sql('business',f"SELECT manifest FROM source_manifests WHERE id='{manifest_id}'"))
        assert manifest['repositories'][0]['upstream_url']==git_origin
        assert detail['artifacts'] and detail['artifacts'][0]['sha256'] == hashlib.sha256(b'operator-native-ok\n').hexdigest()
        a.verify_artifact_gateway(detail['artifacts'][0])
        assert a.sql('business', "SELECT count(*) FROM attempts a JOIN jobs j ON j.id=a.job_id WHERE j.build_id='" + build + "' AND a.node_id='" + node['id'] + "' AND a.status='succeeded'") == '1'
        history = a.api('/api/v1/builds/' + build + '/plans')
        assert len(history['plans']) == 1 and len(history['runs']) == 1 and history['runs'][0]['calls'] == 0
        latest=a.wait(lambda: (v if (v:=a.api('/api/v1/nodes/'+node['id']+'/telemetry'))['host'] and v['host']['fresh'] and v['inventory'] else None))
        inventory=latest['inventory']['inventory']
        assert inventory['os']==host_os and inventory['arch']==arch and inventory['physical_memory_bytes']>0 and inventory['tools']['git']['available']
        attempt=a.sql('business',f"SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id WHERE j.build_id='{build}' AND a.node_id='{node['id']}' AND a.status='succeeded'")
        params=urllib.parse.urlencode({'from':(datetime.now(timezone.utc)-timedelta(minutes=10)).isoformat(),'to':(datetime.now(timezone.utc)+timedelta(seconds=5)).isoformat(),'resolution':'raw'})
        processes=a.wait(lambda: (v if (v:=a.api(f'/api/v1/attempts/{attempt}/processes?'+params))['samples'] else None))
        assert any(p['step']==0 for p in processes['samples']) and all(p['started_at']>0 for p in processes['samples'])
        evidence['telemetry']={'inventory_os':inventory['os'],'inventory_arch':inventory['arch'],'host_fresh':True,'managed_process_samples':len(processes['samples'])}
        evidence.update({'build_id': build, 'node_id': node['id'], 'source_manifest_id': manifest_id, 'commit': detail['commit_sha'], 'emissary_binary_sha256': hashlib.sha256(agent.read_bytes()).hexdigest(), 'git_origin': git_origin, 'artifact_sha256': detail['artifacts'][0]['sha256'], 'git_lfs_downloaded_directly': True, 'accepted_plans': 1})
        evidence['images'] = a.images()
        if args.live_protocol:
            a.forward('service/postgres', '38543:5432')
            values = json.loads(a.kubectl('get','secret','control-plane','-o','json'))['data']
            database = urlsplit(base64.b64decode(values['DATABASE_URL']).decode())
            database = urlunsplit((database.scheme, database.netloc.rsplit('@',1)[0]+'@127.0.0.1:38543',database.path,database.query,''))
            live_environment = dict(os.environ, DATABASE_URL=database,
                ARTIFACTORY_BOOTSTRAP_USERNAME=base64.b64decode(values['ARTIFACTORY_BOOTSTRAP_USERNAME']).decode(),
                ARTIFACTORY_BOOTSTRAP_PASSWORD=base64.b64decode(values['ARTIFACTORY_BOOTSTRAP_PASSWORD']).decode(),
                ARTIFACTORY_LIVE_HTTP_URL=a.base, ARTIFACTORY_LIVE_EMISSARY_CONFIG=str(config),
                ARTIFACTORY_LIVE_PROJECT_ID=project, ARTIFACTORY_LIVE_TENANT_ID=a.tenant_id)
            if args.storage_node:
                selector=json.loads(a.kubectl('get','service','s3','-o','json'))['spec']['selector']
                pod=json.loads(a.kubectl('get','pods','-l',','.join(k+'='+v for k,v in selector.items()),'-o','json'))['items'][0]
                assert pod['spec']['nodeName']==args.storage_node, 'Storage node differs from disposable kind node'
                task=pod['status']['containerStatuses'][0]['containerID'].removeprefix('containerd://')
                live_environment.update(ARTIFACTORY_LIVE_S3_NODE=args.storage_node,ARTIFACTORY_LIVE_S3_TASK=task)
            with (directory/'live-protocol.log').open('wb') as log:
                subprocess.run(['cargo','test','--locked','-p','artifactory-emissary','--test','live_protocol','--','--ignored','--nocapture'],cwd=ROOT,env=live_environment,stdout=log,stderr=log,check=True,timeout=600)
            evidence['live_protocol_passed']=True
    finally:
        try:
            if build:
                try:
                    if a.api('/api/v1/builds/' + build)['status'] not in ['succeeded','failed','lost','canceled']:
                        a.api('/api/v1/builds/' + build + '/cancel', {})
                except (OSError, RuntimeError):
                    pass
            if process:
                process.terminate()
                process.wait(timeout=30)
            if node:
                a.api('/api/v1/nodes/' + node['id'], {'draining': True, 'revoked': True, 'slots': node['slots'], 'labels': node['labels'], 'signing_profiles': node['signing_profiles'], 'pool_id': node['pool_id']}, method='PATCH')
                assert a.sql('business', "SELECT revoked AND draining FROM nodes WHERE id='" + node['id'] + "'") == 't', 'Temporary device identity must be revoked'
                evidence['temporary_device_identity_revoked'] = True
            set_environment(a, 'upstream', {key: original_upstream.get(key) for key in ['FIXTURE_OS','FIXTURE_ARCH']})
            set_environment(a, 'control-plane', {'ARTIFACTORY_PUBLIC_URL': original_control.get('ARTIFACTORY_PUBLIC_URL')})
            a.origin = a.kubectl('exec', 'deploy/control-plane', '--', 'printenv', 'ARTIFACTORY_PUBLIC_URL').strip()
            a.kubectl('patch', 'agentpool', 'default', '--type=merge', '-p', json.dumps({'spec': agent_pool}))
            a.kubectl('delete','service','upstream-native','--ignore-not-found')
            evidence['temporary_routes_restored'] = True
        finally:
            try:
                restore_devices.close()
                evidence['existing_device_draining_restored'] = True
            finally:
                try:
                    restore_network.close()
                    evidence['temporary_git_egress_removed'] = True
                finally:
                    (directory/'evidence.json').write_text(json.dumps(evidence, indent=2))
                    a.cleanup()
    evidence['passed'] = True
    (directory/'evidence.json').write_text(json.dumps(evidence, indent=2))
    print('HOST NATIVE ACCEPTANCE PASSED', directory/'evidence.json', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--context', required=True)
    parser.add_argument('--namespace', required=True)
    parser.add_argument('--live-protocol', action='store_true', help='Also run Rust protocol tests against this isolated database and S3')
    parser.add_argument('--storage-node', help='Disposable kind node containing S3; enables storage pause/heartbeat tests')
    run(parser.parse_args())
