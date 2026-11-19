#!/usr/bin/env python3
"""[INPUT]: Pi 策略、组件 Service 地址和新 Pod 网络身份。
[OUTPUT]: 验证首次 TCP 策略，独立记录 DNS 与组件就绪状态。
[POS]: TCP 使用 Kubernetes 地址快照；完整业务验收仍使用 DNS。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import importlib.util
import json
from pathlib import Path
import time

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('acceptance', Path(__file__).with_name('run.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def targets(a):
    values = {}
    for key, service, port in [('runtime', 'gateway', 8445),
                               ('business', 'control-plane', 8080),
                               ('gateway', 'gateway', 8080),
                               ('provider', 'upstream', 9443),
                               ('database', 'artifactory-timescaledb', 5432),
                               ('objects', 'artifactory-seaweedfs', 8333)]:
        address = json.loads(a.kubectl('get', 'service', service, '-o', 'json'))['spec']['clusterIP']
        values[key] = {'host': service, 'address': address, 'port': port}
    values['provider_ip'] = dict(values['provider'], host=values['provider']['address'])
    return values


def probe_script(addresses):
    return '''
const net = require("node:net");
const dns = require("node:dns");
const targets = __TARGETS__;
function connect([key, target]) {
  return new Promise(resolve => {
    const started = performance.now();
    const socket = net.createConnection({host: target.address, port: target.port});
    let done = false;
    const finish = (connected, code) => {
      if (done) return;
      done = true;
      socket.destroy();
      resolve([key, {...target, connected, code, elapsed_ms: Math.round(performance.now() - started)}]);
    };
    socket.setTimeout(3000, () => finish(false, "TCP_TIMEOUT"));
    socket.on("connect", () => finish(true, null));
    socket.on("error", error => finish(false, error.code || "TCP_ERROR"));
  });
}
function lookup([key, target]) {
  return new Promise(resolve => {
    const started = performance.now();
    let done = false;
    const finish = value => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      resolve([key, {host: target.host, elapsed_ms: Math.round(performance.now() - started), ...value}]);
    };
    const timer = setTimeout(() => finish({code: "DNS_TIMEOUT"}), 3000);
    dns.lookup(target.host, (error, address, family) => finish(error
      ? {code: error.code || "DNS_ERROR"} : {code: null, address, family}));
  });
}
// 首次 TCP 全部结束后才观测 DNS，避免解析流量预热冷 Pod。
Promise.all(Object.entries(targets).map(connect)).then(async tcp => {
  const lookups = await Promise.all(Object.entries(targets)
    .filter(([, target]) => !net.isIP(target.host)).map(lookup));
  console.log(JSON.stringify({connections: Object.fromEntries(tcp),
    dns: Object.fromEntries(lookups),
    resolv_conf: require("node:fs").readFileSync("/etc/resolv.conf", "utf8"),
  }));
});
'''.replace('__TARGETS__', json.dumps(addresses))


def probe(a, restricted):
    name = 'pi-network-' + ('restricted' if restricted else 'control')
    pool = json.loads(a.kubectl('get', 'planningpool', 'default', '-o', 'json'))['spec']
    pod = {'apiVersion': 'v1', 'kind': 'Pod', 'metadata': {'name': name, 'labels': {'app.kubernetes.io/name': 'artifactory-planning-worker' if restricted else 'network-control', 'artifactory.dev/network-probe': str(time.time_ns())}}, 'spec': {'restartPolicy': 'Never', 'automountServiceAccountToken': False, 'containers': [{'name': 'probe', 'image': pool['image'], 'imagePullPolicy': pool.get('imagePullPolicy', 'IfNotPresent'), 'command': ['node', '-e', probe_script(targets(a))]}]}}
    a.kubectl('delete', 'pod', name, '--ignore-not-found')
    a.apply(pod)
    a.wait(lambda: json.loads(a.kubectl('get', 'pod', name, '-o', 'json'))['status']['phase'] in ['Succeeded', 'Failed'])
    values = json.loads(a.kubectl('logs', name).strip())
    observed = json.loads(a.kubectl('get', 'pod', name, '-o', 'json'))
    values['pod'] = {'name': name, 'uid': observed['metadata']['uid'],
                     'address': observed['status'].get('podIP'),
                     'phase': observed['status']['phase']}
    a.kubectl('delete', 'pod', name)
    return values


def network_components(a):
    pods = json.loads(a.command(['kubectl', '--context', a.context, '-n', 'kube-system',
                                'get', 'pods', '-o', 'json']))['items']
    return [{'name': pod['metadata']['name'], 'uid': pod['metadata']['uid'],
             'phase': pod['status']['phase'],
             'containers': [{'name': c['name'], 'ready': c['ready'], 'restarts': c['restartCount']}
                            for c in pod['status'].get('containerStatuses', [])]}
            for pod in pods if pod['metadata'].get('labels', {}).get('k8s-app') in ['cilium', 'kube-dns']]


def assert_isolation(unrestricted, restricted):
    assert all(unrestricted.values()), 'Positive control must reach all addresses before asserting isolation'
    assert restricted and all(value == {key: key in ['runtime', 'provider', 'provider_ip'] for key in unrestricted} for value in restricted), 'Pi reaches Runtime and configured Git port; direct Control, database and object paths stay blocked'


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--context', required=True)
    parser.add_argument('--namespace', required=True)
    parser.add_argument('--skip-e2e', action='store_true')
    parser.add_argument('--repeat', type=int, default=3, help='Independent new-Pod probes with fresh network identities')
    parser.add_argument('--controller', default='cluster CNI', help='Installed CNI name/version for the evidence record')
    args = parser.parse_args()
    a = module.Acceptance(args.context, args.namespace, resume=True)
    a.evidence = json.loads((a.run / 'evidence.json').read_text())
    directory = ROOT / '.local/operator-load' / (time.strftime('%Y%m%d-%H%M%S') + '-network')
    directory.mkdir(parents=True)
    try:
        a.resume()
        components = network_components(a)
        observations = {'unrestricted': probe(a, False), 'restricted': [probe(a, True) for _ in range(args.repeat)]}
        connected = lambda result: {key: value['connected'] for key, value in result['connections'].items()}
        unrestricted = connected(observations['unrestricted'])
        restricted = [connected(value) for value in observations['restricted']]
        print('NETWORK PROBES', {'unrestricted': unrestricted, 'restricted': restricted}, flush=True)
        evidence = {'context': args.context, 'namespace': args.namespace, 'controller': args.controller, 'images': a.images(), 'unrestricted': unrestricted, 'restricted': restricted, 'observations': observations, 'network_components': components, 'scope': 'first TCP connection to Kubernetes addresses; DNS observed independently', 'cold_pod_probe': True, 'e2e': False, 'passed': False}
        (directory / 'evidence.json').write_text(json.dumps(evidence, indent=2))
        try:
            assert_isolation(unrestricted, restricted)
        except AssertionError:
            evidence['network_components_after_failure'] = network_components(a)
            (directory / 'evidence.json').write_text(json.dumps(evidence, indent=2))
            raise
        evidence['passed'] = True
        (directory / 'evidence.json').write_text(json.dumps(evidence, indent=2))
        print('NETWORK ENFORCEMENT PASSED', evidence, flush=True)
        if not args.skip_e2e:
            a.verify()
            evidence['e2e'] = True
            (directory / 'evidence.json').write_text(json.dumps(evidence, indent=2))
        print('NETWORK ACCEPTANCE PASSED', directory / 'evidence.json', flush=True)
    finally:
        a.cleanup()
