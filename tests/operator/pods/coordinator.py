"""[INPUT]: 本机 Operator 的随机服务令牌与测试运行身份。
[OUTPUT]: 提供最小 Runtime HTTP 契约与线程安全回执。
[POS]: 仅模拟业务状态；真实 Pod 由 Kubernetes 与 Operator 执行。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import copy
import json
import secrets
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Coordinator:
    def __init__(self):
        self.token = secrets.token_hex(32)
        self.lock = threading.Lock()
        self.runs = {}
        self.events = []
        coordinator = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                if self.headers.get('Authorization') != 'Bearer ' + coordinator.token:
                    self.send_error(401)
                    return
                length = int(self.headers.get('Content-Length', '0'))
                if length < 0 or length > 8 * 1024 * 1024:
                    self.send_error(413)
                    return
                try:
                    data = json.loads(self.rfile.read(length))
                    result = coordinator.request(self.path, data)
                except (ValueError, KeyError):
                    self.send_error(400)
                    return
                encoded = json.dumps(result).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = 'http://127.0.0.1:' + str(self.server.server_address[1])

    def add(self, run_id):
        with self.lock:
            self.runs[run_id] = {'status': 'provisioning', 'lifecycle': {'generation': 1, 'pending': False, 'canceled': False}}

    def cancel(self, run_id):
        with self.lock:
            self.runs[run_id]['status'] = 'canceled'
            self.runs[run_id]['lifecycle']['canceled'] = True

    @staticmethod
    def execution(body, run):
        fields = {'run_id', 'generation', 'uid', 'name', 'namespace'}
        if set(body) != fields or any(not isinstance(body[key], str) or not body[key]
                                      for key in fields - {'generation'}):
            raise ValueError('Complete execution identity required')
        if type(body['generation']) is not int or body['generation'] != run['lifecycle']['generation']:
            raise ValueError('Execution generation fenced')
        return body

    def request(self, path, body):
        with self.lock:
            if path == '/v1/queue':
                return {'active': 0, 'queued': 0, 'runs': []}
            if path == '/v1/get':
                return copy.deepcopy(self.runs[body['run_id']])
            if path == '/v1/register':
                run = self.runs[body['run_id']]
                self.execution(body, run)
                if run['lifecycle']['canceled']:
                    raise ValueError('Execution canceled')
                if run.get('execution') not in (None, body):
                    raise ValueError('Execution identity changed')
                run['execution'] = copy.deepcopy(body)
                if not any(e['path'] == path and e['body'] == body for e in self.events):
                    self.events.append({'path': path, 'body': copy.deepcopy(body)})
                return {'ok': True}
            if path in ('/v1/execution-failed', '/v1/execution-lost'):
                identity = body.get('execution')
                run_id = body.get('run_id') or identity['run_id']
                run = self.runs[run_id]
                life = run['lifecycle']
                if identity:
                    self.execution(identity, run)
                    if identity['run_id'] != run_id:
                        raise ValueError('Execution scope mismatch')
                if run.get('execution') != identity:
                    raise ValueError('Failure identity differs from registration')
                if path == '/v1/execution-lost':
                    if type(body['generation']) is not int or body['generation'] != life['generation'] or body['reason'] not in ('pod_missing', 'resource_deleted'):
                        raise ValueError('Lost execution scope mismatch')
                    if identity is None and body['reason'] != 'resource_deleted':
                        raise ValueError('Missing execution identity')
                else:
                    if identity is None:
                        raise ValueError('Missing execution identity')
                    reason = body['reason']
                    if reason.get('kind') not in ('source_preparation', 'worker_exit', 'pod_failed'):
                        raise ValueError('Unknown failure kind')
                    if reason['kind'] != 'pod_failed' and type(reason.get('exit_code')) is not int:
                        raise ValueError('Missing exit code')
                    if reason['kind'] == 'source_preparation' and reason['exit_code'] == 0:
                        raise ValueError('Source preparation did not fail')
                if run['status'] in ('failed', 'canceled', 'succeeded', 'needs_input') or life['pending']:
                    return {'status': run['status'], 'pending': life['pending'], 'generation': life['generation']}
                if run['status'] not in ('provisioning', 'running') or life['canceled']:
                    raise ValueError('Run is not active')
                if path == '/v1/execution-lost':
                    run.pop('execution', None)
                    if body['reason'] == 'pod_missing' and life['generation'] < 3:
                        life['generation'] += 1
                        run['status'] = 'queued'
                    else:
                        run['status'] = 'failed'
                    self.events.append({'path': path, 'body': copy.deepcopy(body)})
                    return {'status': run['status'], 'pending': False, 'generation': life['generation']}
                run['status'] = 'failed'
                self.events.append({'path': path, 'body': copy.deepcopy(body)})
                return {'status': 'failed', 'pending': False}
            raise ValueError('Unknown Runtime endpoint')

    def snapshot(self):
        with self.lock:
            return copy.deepcopy(self.events)

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
