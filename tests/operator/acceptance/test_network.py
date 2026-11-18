#!/usr/bin/env python3
"""[INPUT]: 冷 Pod 探针、本机 TCP 服务与受控解析失败。
[OUTPUT]: 验证首次 TCP 与 DNS 故障分别记录且不重试。
[POS]: 不访问 Kubernetes 的网络验收夹具回归。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
import json
from pathlib import Path
import socket
import socketserver
import subprocess
import threading
import unittest

SPEC = importlib.util.spec_from_file_location('network_acceptance', Path(__file__).with_name('network.py'))
NETWORK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NETWORK)


class NetworkProbeTests(unittest.TestCase):
    def test_cold_tcp_connects_once_when_hostname_resolution_fails(self):
        class Connection(socketserver.BaseRequestHandler):
            def handle(self):
                pass

        server = socketserver.ThreadingTCPServer(('127.0.0.1', 0), Connection)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        port = server.server_address[1]

        class Acceptance:
            ns = 'network-fixture'

            def kubectl(self, *args):
                if args[:2] == ('get', 'service'):
                    return json.dumps({'spec': {'clusterIP': '127.0.0.1', 'ports': [{'port': port}]}})
                if args[:2] == ('get', 'agentpool'):
                    return json.dumps({'spec': {'image': 'fixture'}})
                if args[:2] == ('get', 'pod'):
                    return json.dumps({'metadata': {'name': args[2], 'uid': 'cold-probe'},
                                       'status': {'phase': 'Succeeded', 'podIP': '127.0.0.1'}})
                if args[0] == 'logs':
                    script = self.pod['spec']['containers'][0]['command'][-1]
                    # 保留真实 TCP；只让本次探针的 getaddrinfo 返回暂态失败。
                    prelude = '''
(() => {
let tcpCompleted = 0;
let dnsBeforeTcp = false;
const dns = require("node:dns");
dns.lookup = (host, options, callback) => {
  if (tcpCompleted < 7) dnsBeforeTcp = true;
  if (typeof options === "function") callback = options;
  process.nextTick(() => callback(Object.assign(new Error("temporary lookup failure"), {code:"EAI_AGAIN"})));
};
const net = require("node:net");
const original = net.createConnection;
let attempts = 0;
net.createConnection = options => {
  attempts++;
  const socket = original({...options, port: ''' + str(port) + '''});
  socket.once("connect", () => tcpCompleted++);
  socket.once("error", () => tcpCompleted++);
  return socket;
};
const log = console.log;
console.log = value => log(JSON.stringify({...JSON.parse(value), test_attempts:attempts, test_dns_before_tcp:dnsBeforeTcp}));
})();
'''
                    return subprocess.check_output(['node', '-e', prelude + script], text=True, timeout=10)
                return ''

            def apply(self, pod):
                self.pod = pod

            def wait(self, callback):
                return callback()

        try:
            result = NETWORK.probe(Acceptance(), True)
            connections = result.get('connections', {})
            self.assertTrue(connections.get('runtime', {}).get('connected'),
                            'Cold TCP must not inherit the hostname lookup failure')
            self.assertEqual(set(connections), {'runtime', 'business', 'gateway',
                                               'provider', 'provider_ip', 'database', 'objects'})
            self.assertTrue(all(value['connected'] for value in connections.values()))
            self.assertEqual(result['dns']['runtime']['code'], 'EAI_AGAIN')
            self.assertIn('elapsed_ms', result['dns']['runtime'])
            self.assertEqual(result['pod']['uid'], 'cold-probe')
            self.assertEqual(result['test_attempts'], 7)
            self.assertFalse(result['test_dns_before_tcp'])
            reachable = {key: value['connected'] for key, value in connections.items()}
            with self.assertRaisesRegex(AssertionError, 'direct Control'):
                NETWORK.assert_isolation(reachable, [reachable])
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    def test_refused_allowed_address_still_fails_without_retry(self):
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            port = reservation.getsockname()[1]
        script = NETWORK.probe_script({'runtime': {'host': '127.0.0.1',
                                                   'address': '127.0.0.1', 'port': port}})
        result = json.loads(subprocess.check_output(['node', '-e', script], text=True, timeout=10))
        connection = result['connections']['runtime']
        self.assertFalse(connection['connected'])
        self.assertEqual(connection['code'], 'ECONNREFUSED')
        self.assertEqual(result['dns'], {})
        with self.assertRaisesRegex(AssertionError, 'Positive control'):
            NETWORK.assert_isolation({'runtime': False}, [{'runtime': False}])


if __name__ == '__main__':
    unittest.main()
