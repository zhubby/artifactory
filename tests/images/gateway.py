#!/usr/bin/env python3
"""[INPUT]: 本机构建的 Pingora 网关和 Python HTTP/TLS 夹具。
[OUTPUT]: 验证监听就绪、交付文件名、Manifest 发布围栏和写请求禁止重放。
[POS]: 使用真实 TCP/TLS 和流式对象；不连接已有集群。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import signal
import socket
import ssl
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

DATA = bytes(range(256)) * 32768
TOKEN = 'isolated-gateway-test-token-' + 'a' * 32
UPSTREAM_IDLE_TTL = 2
GATEWAY_IDLE_WAIT = 32
MANIFEST = b'{"schemaVersion":2,"fixture":"frozen-release"}'
MANIFEST_DIGEST = 'sha256:' + hashlib.sha256(MANIFEST).hexdigest()
DELIVERY_FILENAME = 'app-server-1.2.3-linux-amd64.tar.gz'

class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *args):
        pass
    def reply(self, status, body=b'', **headers):
        self.send_response(status)
        for key, value in headers.items():
            self.send_header(key.replace('_', '-'), value)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if self.command != 'HEAD':
            self.wfile.write(body)
    def do_POST(self):
        if self.path == '/v2/repo/blobs/uploads/':
            self.server.layer = b''
            return self.reply(202, Location='/v2/repo/blobs/uploads/fixture')
        if self.path == '/internal/registry-manifest-authorize':
            assert self.headers.get('Authorization') == 'Bearer ' + TOKEN
            request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            self.server.guard_calls.append(request)
            if request['authorization'] == 'Bearer expired-release-publisher':
                return self.reply(401)
            if request['authorization'] != 'Bearer release-publisher':
                return self.reply(403)
            if request['digest'] != MANIFEST_DIGEST or request['reference'] not in ['1.2.3', MANIFEST_DIGEST]:
                return self.reply(409)
            return self.reply(200, b'{"authorized":true}', Content_Type='application/json')
        if self.path != '/internal/artifact-resolve' or self.headers.get('Authorization') != 'Bearer ' + TOKEN:
            return self.reply(403)
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        expected = {'get': ['GET', 'HEAD'], 'named': ['GET', 'HEAD'], 'badname': ['GET'], 'put': ['PUT'], 'escape': ['GET']}
        if request.get('method') not in expected.get(request.get('ticket'), []):
            return self.reply(403)
        origin = self.server.origin if request['ticket'] != 'escape' else 'http://127.0.0.1:1'
        resolved = {'url': origin + '/private/object?signature=retained'}
        if request['ticket'] in ['named', 'badname']:
            resolved['filename'] = DELIVERY_FILENAME if request['ticket'] == 'named' else 'bad\r\nX:injected'
        self.reply(200, json.dumps(resolved).encode(), Content_Type='application/json')
    def do_GET(self):
        if self.path == '/v2/' and not self.headers.get('Authorization') and getattr(self.server, 'challenge_enabled', False):
            return self.reply(401, WWW_Authenticate='Bearer realm="https://auth.example.invalid/token",service="registry"')
        if self.path == '/api/hold':
            self.server.hold_started.set()
            assert self.server.hold_release.wait(timeout=10)
            return self.reply(200)
        if self.path.startswith('/v2/'):
            assert not self.headers.get('X-Client-Cert')
            assert not self.headers.get('X-Artifactory-Identity')
            return self.reply(200, b'registry')
        assert self.path == '/private/object?signature=retained', self.path
        assert self.headers['Host'] == urlsplit(self.server.origin).netloc
        assert not self.headers.get('Authorization')
        assert not self.headers.get('Cookie')
        content = DATA
        if self.headers.get('Range'):
            begin, end = map(int, self.headers['Range'].removeprefix('bytes=').split('-'))
            content = DATA[begin:end + 1]
            return self.reply(206, content, Content_Range=f'bytes {begin}-{end}/{len(DATA)}', ETag='"fixture"')
        self.reply(200, content, Accept_Ranges='bytes', ETag='"fixture"')
    do_HEAD = do_GET
    def do_PATCH(self):
        assert self.path == '/v2/repo/blobs/uploads/fixture'
        assert self.headers.get('Authorization') == 'Bearer registry-token'
        self.server.layer += self.rfile.read(int(self.headers['Content-Length']))
        self.reply(202, Location=self.path, Range=f'0-{len(self.server.layer)-1}')
    def do_PUT(self):
        if '/images/app/manifests/' in self.path:
            body = self.rfile.read(int(self.headers['Content-Length']))
            self.server.manifest_writes.append((self.path, body))
            return self.reply(201, Docker_Content_Digest='sha256:' + hashlib.sha256(body).hexdigest())
        if self.path.startswith('/v2/repo/blobs/uploads/fixture?digest='):
            assert self.server.layer == DATA
            return self.reply(201, Location='/v2/repo/blobs/sha256:fixture')
        assert self.path == '/private/object?signature=retained'
        size = int(self.headers['Content-Length'])
        self.server.upload = self.rfile.read(size)
        self.reply(200)


class IdleHandler(Handler):
    def setup(self):
        super().setup()
        with self.server.lock:
            self.server.connections += 1
            self.connection_id = self.server.connections
        self.last_response = time.monotonic()

    def do_POST(self):
        assert self.path == '/api/idle-connection'
        body = self.rfile.read(int(self.headers['Content-Length']))
        expired = time.monotonic() - self.last_response > UPSTREAM_IDLE_TTL
        with self.server.lock:
            self.server.requests.append((self.connection_id, body))
            if expired:
                self.server.stale_requests += 1
            else:
                self.server.executions[body] = self.server.executions.get(body, 0) + 1
        # 模拟中间层静默丢失连接状态，直到后续请求到达才断开。
        if expired or body == b'consume-then-disconnect':
            self.close_connection = True
            self.connection.shutdown(socket.SHUT_RDWR)
            return
        self.reply(200, str(self.connection_id).encode())
        self.last_response = time.monotonic()


def idle_connection_regression(binary, base, tls, processes):
    upstream = ThreadingHTTPServer(('127.0.0.1', 0), IdleHandler)
    upstream.socket = tls.wrap_socket(upstream.socket, server_side=True)
    upstream.lock = threading.Lock()
    upstream.connections, upstream.stale_requests = 0, 0
    upstream.requests, upstream.executions = [], {}
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    try:
        http_port = port()
        origin = f'https://127.0.0.1:{upstream.server_port}'
        env = dict(base, ARTIFACTORY_GATEWAY_PROFILE='all',
                   ARTIFACTORY_GATEWAY_HTTP_BIND=f'127.0.0.1:{http_port}',
                   ARTIFACTORY_GATEWAY_CONTROL=origin)
        process = subprocess.Popen([binary], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        processes.append(process)
        wait(http_port, process)
        first = request(http_port, 'POST', '/api/idle-connection', b'first')
        immediate = request(http_port, 'POST', '/api/idle-connection', b'immediate')
        assert first[0] == immediate[0] == 200, (first, immediate)
        assert first[2] == immediate[2], 'immediate requests must reuse the same upstream TCP connection'
        print('PASS: immediate POST requests reuse upstream TCP; waiting for the production idle deadline', flush=True)
        time.sleep(GATEWAY_IDLE_WAIT)
        after_idle = request(http_port, 'POST', '/api/idle-connection', b'after-idle')
        assert after_idle[0] == 200, f'first POST after idle returned {after_idle[0]}'
        assert after_idle[2] != first[2], 'idle connection must be replaced before the next POST'
        dropped = request(http_port, 'POST', '/api/idle-connection', b'consume-then-disconnect')
        assert dropped[0] == 502, f'consumed POST with a lost response returned {dropped[0]}'
        with upstream.lock:
            assert upstream.stale_requests == 0, 'gateway sent a request over an expired upstream connection'
            assert upstream.executions == {b'first': 1, b'immediate': 1, b'after-idle': 1,
                                           b'consume-then-disconnect': 1}, upstream.executions
            assert [body for _, body in upstream.requests] == [
                b'first', b'immediate', b'after-idle', b'consume-then-disconnect'], upstream.requests
        print('PASS: idle HTTP connections are replaced and consumed POST requests are never replayed', flush=True)
    finally:
        upstream.shutdown()
        upstream.server_close()


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]

def request(port_number, method, path, body=None, headers=None, context=None, timeout=10):
    client = http.client.HTTPSConnection('127.0.0.1', port_number, timeout=timeout, context=context) if context else http.client.HTTPConnection('127.0.0.1', port_number, timeout=timeout)
    try:
        client.request(method, path, body=body, headers=headers or {})
        response = client.getresponse()
        return response.status, {key.lower(): value for key, value in response.getheaders()}, response.read()
    finally:
        client.close()

def wait(port_number, process):
    for _ in range(100):
        if process.poll() is not None:
            raise RuntimeError('gateway exited before listening')
        try:
            with socket.create_connection(('127.0.0.1', port_number), timeout=.1):
                return
        except OSError:
            time.sleep(.1)
    raise RuntimeError('gateway did not listen')


def wait_ready(port_number, process, context=None):
    for _ in range(100):
        assert process.poll() is None, 'gateway exited before becoming ready'
        try:
            status = request(port_number, 'GET', '/healthz', context=context, timeout=.2)[0]
            assert status in [200, 503], f'unexpected readiness status {status}'
            if status == 200:
                return
        except (OSError, http.client.HTTPException):
            pass
        time.sleep(.05)
    raise AssertionError('gateway did not become ready')


def startup_failure_regression(binary, env, root):
    invalid_pem = root / 'invalid.pem'
    invalid_pem.write_text('invalid PEM fixture\n')
    context = ssl.create_default_context(cafile=str(root / 'ca.pem'))
    cases = [
        ('internal certificate', {'ARTIFACTORY_GATEWAY_INTERNAL_TLS_CERT': str(invalid_pem)}),
        ('internal private key', {'ARTIFACTORY_GATEWAY_INTERNAL_TLS_KEY': str(invalid_pem)}),
        ('device bind', {'ARTIFACTORY_GATEWAY_TCP_BIND': f'192.0.2.123:{port()}'}),
        ('telemetry bind', {'ARTIFACTORY_GATEWAY_TELEMETRY_BIND': f'192.0.2.123:{port()}'}),
    ]
    for name, changes in cases:
        http_port, internal_port = port(), port()
        invalid = dict(env, ARTIFACTORY_GATEWAY_HTTP_BIND=f'127.0.0.1:{http_port}',
                       ARTIFACTORY_GATEWAY_INTERNAL_BIND=f'127.0.0.1:{internal_port}',
                       ARTIFACTORY_GATEWAY_TCP_BIND=f'127.0.0.1:{port()}',
                       ARTIFACTORY_GATEWAY_TELEMETRY_BIND=f'127.0.0.1:{port()}')
        invalid.update(changes)
        with tempfile.TemporaryFile() as log:
            process = subprocess.Popen([binary], env=invalid, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 10
                while process.poll() is None and time.monotonic() < deadline:
                    for number, tls in [(http_port, None), (internal_port, context)]:
                        try:
                            status = request(number, 'GET', '/healthz', context=tls, timeout=.1)[0]
                            assert status == 503, f'{name}: failed listener advertised readiness {status}'
                        except (OSError, http.client.HTTPException):
                            pass
                    time.sleep(.02)
                assert process.poll() is not None, f'{name}: gateway survived a required listener failure'
                assert process.returncode != 0, f'{name}: gateway exited successfully after listener failure'
            finally:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
        print(f'PASS: {name} failure never reports ready and exits nonzero', flush=True)


def startup_readiness_regression(binary, env, root):
    context = ssl.create_default_context(cafile=str(root / 'ca.pem'))
    blocker = socket.socket()
    blocker.bind(('127.0.0.1', 0))
    blocker.listen()
    telemetry_port = blocker.getsockname()[1]
    http_port, internal_port = port(), port()
    delayed = dict(env, ARTIFACTORY_GATEWAY_HTTP_BIND=f'127.0.0.1:{http_port}',
                   ARTIFACTORY_GATEWAY_INTERNAL_BIND=f'127.0.0.1:{internal_port}',
                   ARTIFACTORY_GATEWAY_TCP_BIND=f'127.0.0.1:{port()}',
                   ARTIFACTORY_GATEWAY_TELEMETRY_BIND=f'127.0.0.1:{telemetry_port}')
    with tempfile.TemporaryFile() as log:
        process = subprocess.Popen([binary], env=delayed, stdout=log, stderr=log)
        try:
            observed = set()
            deadline = time.monotonic() + 10
            while len(observed) != 2 and time.monotonic() < deadline:
                assert process.poll() is None, 'gateway exited while the telemetry bind was retrying'
                for number, tls in [(http_port, None), (internal_port, context)]:
                    try:
                        status = request(number, 'GET', '/healthz', context=tls, timeout=.1)[0]
                        assert status == 503, f'gateway advertised readiness {status} before owning every listener'
                        observed.add(number)
                    except (OSError, http.client.HTTPException):
                        pass
                time.sleep(.02)
            assert len(observed) == 2, 'public and internal health endpoints did not report pending readiness'
            blocker.close()
            wait_ready(http_port, process)
            wait_ready(internal_port, process, context)
            process.send_signal(signal.SIGINT)
            process.communicate(timeout=5)
            assert process.returncode == 0, f'normal shutdown exited with {process.returncode}'
        finally:
            blocker.close()
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
    print('PASS: both health endpoints stay unready until the last listener starts; normal shutdown exits zero', flush=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/artifactory-gateway'))
    parser.add_argument('--startup-only', action='store_true', help='run listener startup failure regressions only')
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.origin = f'http://127.0.0.1:{server.server_port}'
    threading.Thread(target=server.serve_forever, daemon=True).start()
    processes = []
    with tempfile.TemporaryDirectory(prefix='artifactory-gateway-') as directory:
        root = Path(directory)
        def openssl(*options):
            subprocess.run(['openssl', *options], cwd=root, check=True, capture_output=True)
        openssl('req','-x509','-newkey','rsa:2048','-nodes','-keyout','ca.key','-out','ca.pem','-days','1','-subj','/CN=test CA','-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign','-addext','subjectKeyIdentifier=hash')
        for identity in ['server', 'client']:
            openssl('req','-newkey','rsa:2048','-nodes','-keyout',identity+'.key','-out',identity+'.csr','-subj','/CN='+identity)
            (root/'ext').write_text('subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\nextendedKeyUsage='+('serverAuth' if identity=='server' else 'clientAuth')+'\n')
            openssl('x509','-req','-in',identity+'.csr','-CA','ca.pem','-CAkey','ca.key','-CAcreateserial','-out',identity+'.pem','-days','1','-extfile','ext')
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(root/'server.pem', root/'server.key')
        tls.load_verify_locations(root/'ca.pem')
        tls.verify_mode = ssl.CERT_REQUIRED
        upstreams = []
        def tls_server(upstream, identity):
            while True:
                try:
                    conn, _ = upstream.accept()
                    with tls.wrap_socket(conn, server_side=True) as stream:
                        stream.sendall(identity + b'-client-certificate-verified:' + stream.recv(64))
                except ssl.SSLError:
                    continue
                except OSError:
                    return
        for identity in [b'control', b'telemetry']:
            upstream = socket.socket(); upstream.bind(('127.0.0.1', 0)); upstream.listen()
            upstreams.append(upstream)
            threading.Thread(target=tls_server, args=(upstream, identity), daemon=True).start()
        service_tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        service_tls.load_cert_chain(root/'server.pem', root/'server.key')
        server.socket = service_tls.wrap_socket(server.socket, server_side=True)
        server.origin = f'https://127.0.0.1:{server.server_port}'
        # 第二个根模拟系统信任，验证服务 CA 不替换公网信任。
        openssl('req','-x509','-newkey','rsa:2048','-nodes','-keyout','system.key','-out','system.pem','-days','1','-subj','/CN=system test CA','-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign','-addext','subjectKeyIdentifier=hash')
        openssl('req','-newkey','rsa:2048','-nodes','-keyout','registry.key','-out','registry.csr','-subj','/CN=registry')
        (root/'ext').write_text((root/'ext').read_text().replace('clientAuth','serverAuth'))
        openssl('x509','-req','-in','registry.csr','-CA','system.pem','-CAkey','system.key','-CAcreateserial','-out','registry.pem','-days','1','-extfile','ext')
        registry = ThreadingHTTPServer(('127.0.0.1',0),Handler)
        registry_tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        registry_tls.load_cert_chain(root/'registry.pem',root/'registry.key')
        registry.socket = registry_tls.wrap_socket(registry.socket,server_side=True)
        registry.origin = f'https://127.0.0.1:{registry.server_port}'
        threading.Thread(target=registry.serve_forever,daemon=True).start()
        base = dict(os.environ, SSL_CERT_FILE=str(root/'system.pem'), ARTIFACTORY_GATEWAY_TOKEN=TOKEN, ARTIFACTORY_SERVICE_CA=str(root/'ca.pem'), ARTIFACTORY_S3_ENDPOINT=server.origin, ARTIFACTORY_GATEWAY_CONTROL=server.origin, ARTIFACTORY_GATEWAY_WEB=server.origin, ARTIFACTORY_GATEWAY_REGISTRY=registry.origin, ARTIFACTORY_GATEWAY_DEVICE=f'127.0.0.1:{upstreams[0].getsockname()[1]}', ARTIFACTORY_GATEWAY_TELEMETRY=f'127.0.0.1:{upstreams[1].getsockname()[1]}')
        try:
            http_port, tls_port, internal_port, telemetry_port = port(), port(), port(), port()
            env = dict(base, ARTIFACTORY_GATEWAY_PROFILE='unified', ARTIFACTORY_GATEWAY_HTTP_BIND=f'127.0.0.1:{http_port}', ARTIFACTORY_GATEWAY_TCP_BIND=f'127.0.0.1:{tls_port}', ARTIFACTORY_GATEWAY_INTERNAL_BIND=f'127.0.0.1:{internal_port}', ARTIFACTORY_GATEWAY_TELEMETRY_BIND=f'127.0.0.1:{telemetry_port}', ARTIFACTORY_GATEWAY_INTERNAL_TLS_CERT=str(root/'server.pem'), ARTIFACTORY_GATEWAY_INTERNAL_TLS_KEY=str(root/'server.key'), ARTIFACTORY_GATEWAY_HTTP_CONNECTIONS='1', ARTIFACTORY_GATEWAY_INTERNAL_CONNECTIONS='1')
            for missing in ['ARTIFACTORY_GATEWAY_INTERNAL_TLS_KEY', 'ARTIFACTORY_GATEWAY_INTERNAL_TLS_CERT']:
                invalid = dict(env)
                invalid.pop(missing)
                result = subprocess.run([binary], env=invalid, capture_output=True, timeout=5)
                assert result.returncode != 0 and b'requires both certificate and key' in result.stderr
            invalid = dict(env)
            invalid.pop('ARTIFACTORY_GATEWAY_INTERNAL_TLS_KEY')
            invalid.pop('ARTIFACTORY_GATEWAY_INTERNAL_TLS_CERT')
            result = subprocess.run([binary], env=invalid, capture_output=True, timeout=5)
            assert result.returncode != 0 and b'unified gateway requires internal TLS' in result.stderr
            invalid = dict(env, ARTIFACTORY_GATEWAY_INTERNAL_BIND=env['ARTIFACTORY_GATEWAY_HTTP_BIND'])
            result = subprocess.run([binary], env=invalid, capture_output=True, timeout=5)
            assert result.returncode != 0 and b'require distinct ports' in result.stderr
            startup_failure_regression(binary, env, root)
            startup_readiness_regression(binary, env, root)
            if args.startup_only:
                return
            process = subprocess.Popen([binary], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            processes.append(process)
            for number in [http_port, tls_port, internal_port, telemetry_port]:
                wait(number, process)
            internal_client = ssl.create_default_context(cafile=str(root/'ca.pem'))
            wait_ready(http_port, process)
            wait_ready(internal_port, process, internal_client)
            status, headers, body = request(http_port,'GET','/artifacts/object?ticket=get',headers={'Range':'bytes=123-4096','Authorization':'Bearer must-not-forward','Cookie':'secret'})
            assert status == 206 and body == DATA[123:4097] and headers['content-range'] == f'bytes 123-4096/{len(DATA)}'
            status, headers, body = request(http_port,'HEAD','/artifacts/object?ticket=get')
            assert status == 200 and not body and int(headers['content-length']) == len(DATA)
            dispositions = []
            for method, extra in [('GET', {}), ('HEAD', {}), ('GET', {'Range': 'bytes=1-9'})]:
                status, headers, _ = request(http_port, method, '/artifacts/object?ticket=named', headers=extra)
                assert status == (206 if extra else 200)
                dispositions.append(headers['content-disposition'])
            assert len(set(dispositions)) == 1 and f'filename="{DELIVERY_FILENAME}"' in dispositions[0]
            assert request(http_port, 'GET', '/artifacts/object?ticket=badname')[0] == 403
            assert request(http_port,'PUT','/artifacts/object?ticket=put',DATA)[0] == 200 and server.upload == DATA
            assert request(http_port,'PUT','/artifacts/object?ticket=get',b'invalid')[0] == 403
            assert request(http_port,'GET','/artifacts/object?ticket=escape')[0] == 403
            assert request(http_port,'GET','/internal/artifact-resolve')[0] == 403
            assert request(http_port,'GET','/v2/',headers={'X-Client-Cert':'forged','X-Artifactory-Identity':'forged'})[2] == b'registry'
            status, headers, _ = request(http_port,'POST','/v2/repo/blobs/uploads/')
            assert status == 202 and headers['location'] == '/v2/repo/blobs/uploads/fixture'
            for chunk in [DATA[:4096], DATA[4096:]]:
                assert request(http_port,'PATCH',headers['location'],chunk,{'Authorization':'Bearer registry-token'})[0] == 202
            assert request(http_port,'PUT',headers['location']+'?digest=sha256:fixture',b'')[0] == 201
            registry.manifest_writes, server.guard_calls = [], []
            registry.challenge_enabled = True
            formal = '/v2/tenant/project/images/app/manifests/'
            publisher = {'Authorization': 'Bearer release-publisher', 'Content-Type': 'application/vnd.oci.image.manifest.v1+json'}
            assert request(http_port, 'PUT', formal + '1.2.3', MANIFEST, publisher)[0] == 201
            assert request(http_port, 'PUT', formal + MANIFEST_DIGEST, MANIFEST, publisher)[0] == 201
            assert len(registry.manifest_writes) == 2
            assert server.guard_calls[-1]['digest'] == MANIFEST_DIGEST
            for reference, body, auth, expected_status in [
                ('latest', MANIFEST, publisher, 409),
                ('1.2.3', MANIFEST + b' ', publisher, 409),
                ('1.2.3', MANIFEST, {'Authorization': 'Bearer ordinary-user'}, 403),
                ('1.2.3', MANIFEST, {}, 401),
            ]:
                assert request(http_port, 'PUT', formal + reference, body, auth)[0] == expected_status
            for authorization in ['Basic cHVibGlzaGVyOmZpeHR1cmU=', 'Bearer expired-release-publisher']:
                status, headers, _ = request(http_port, 'PUT', formal + '1.2.3', MANIFEST, {'Authorization': authorization})
                assert status == 401 and headers['www-authenticate'].startswith('Bearer realm=')
            assert request(http_port, 'PUT', formal + '1.2.3', b'', dict(publisher, **{'Content-Length': str(4*1024*1024+1)}))[0] == 413
            assert request(http_port, 'PUT', formal.replace('/images/', '/%69mages/') + '1.2.3', MANIFEST, publisher)[0] == 400
            assert len(registry.manifest_writes) == 2, 'rejected Manifest reached Distribution'
            registry.challenge_enabled = False
            assert request(internal_port,'GET','/api/v1/images',context=internal_client)[0] == 404
            assert request(internal_port,'POST','/internal/artifact-resolve',json.dumps({'ticket':'get','method':'GET'}),{'Authorization':'Bearer '+TOKEN,'Content-Type':'application/json'},context=internal_client)[0] == 200
            client = ssl.create_default_context(cafile=str(root/'ca.pem'))
            client.load_cert_chain(root/'client.pem', root/'client.key')
            for number, identity in [(tls_port, b'control'), (telemetry_port, b'telemetry')]:
                with socket.create_connection(('127.0.0.1',number),timeout=5) as conn:
                    with client.wrap_socket(conn,server_hostname='localhost') as stream:
                        stream.sendall(b'ping'); assert stream.recv(128) == identity + b'-client-certificate-verified:ping'
            anonymous = ssl.create_default_context(cafile=str(root/'ca.pem'))
            for number in [tls_port, telemetry_port]:
                try:
                    with socket.create_connection(('127.0.0.1',number),timeout=5) as conn:
                        with anonymous.wrap_socket(conn,server_hostname='localhost') as stream:
                            stream.sendall(b'bad'); assert not stream.recv(128)
                except ssl.SSLError:
                    pass
            server.hold_started = threading.Event()
            server.hold_release = threading.Event()
            held = []
            pending = threading.Thread(target=lambda: held.append(request(http_port, 'GET', '/api/hold')))
            pending.start()
            try:
                assert server.hold_started.wait(timeout=5)
                assert request(http_port, 'GET', '/v2/')[0] == 503
                assert request(internal_port,'POST','/internal/artifact-resolve',json.dumps({'ticket':'get','method':'GET'}),{'Authorization':'Bearer '+TOKEN,'Content-Type':'application/json'},context=internal_client)[0] == 200
            finally:
                server.hold_release.set()
                pending.join(timeout=10)
            assert held and held[0][0] == 200
            idle_connection_regression(binary, base, service_tls, processes)
            print('PASS: unified listeners, isolated budgets, filename-bound Range/HEAD, bounded frozen Manifest guard, streaming layers, private TLS API and real mTLS passthrough')
        finally:
            for process in processes:
                process.terminate()
                try: process.communicate(timeout=5)
                except subprocess.TimeoutExpired: process.kill(); process.communicate()
            for upstream in upstreams:
                upstream.close()
            server.shutdown(); server.server_close(); registry.shutdown(); registry.server_close()
if __name__ == '__main__':
    main()
