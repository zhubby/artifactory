#!/usr/bin/env python3
"""[INPUT]: 本地 Pingora 二进制和隔离 WebSocket 上游。
[OUTPUT]: 验证真实升级、字节、空闲存活及终端与 HTTP 独立配额。
[POS]: 网关传输回归；设备 PTY 和业务授权由其他验收覆盖。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import base64
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import threading
import time

PATH = '/api/v1/terminal-sessions/00000000-0000-0000-0000-000000000001/stream'


def receive(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.read(size - len(data))
        if not chunk:
            raise EOFError('WebSocket closed')
        data.extend(chunk)
    return bytes(data)


def read_frame(stream):
    first, second = receive(stream, 2)
    size = second & 127
    if size == 126:
        size = struct.unpack('!H', receive(stream, 2))[0]
    elif size == 127:
        size = struct.unpack('!Q', receive(stream, 8))[0]
    assert size <= 32 * 1024, 'oversized fixture frame'
    mask = receive(stream, 4) if second & 128 else b''
    data = receive(stream, size)
    if mask:
        data = bytes(value ^ mask[index % 4] for index, value in enumerate(data))
    return first & 15, data


def write_frame(stream, kind, data, masked=False):
    flag = 128 if masked else 0
    header = bytes([128 | kind, flag | len(data)]) if len(data) < 126 else bytes([128 | kind, flag | 126]) + struct.pack('!H', len(data))
    if masked:
        mask = os.urandom(4)
        header += mask
        data = bytes(value ^ mask[index % 4] for index, value in enumerate(data))
    stream.write(header + data)
    stream.flush()


class Upstream(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path == '/api/hold':
            self.server.held.set()
            assert self.server.release.wait(20)
            self.send_response(200)
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        assert self.path == PATH
        key = self.headers['Sec-WebSocket-Key']
        accept = base64.b64encode(hashlib.sha1((key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
        self.send_response(101)
        self.send_header('Upgrade', 'websocket')
        self.send_header('Connection', 'Upgrade')
        self.send_header('Sec-WebSocket-Accept', accept)
        self.end_headers()
        try:
            while True:
                kind, data = read_frame(self.rfile)
                write_frame(self.wfile, 10 if kind == 9 else kind, data)
                if kind == 8:
                    return
        except (EOFError, OSError):
            pass
        finally:
            self.close_connection = True


def free_port():
    with socket.socket() as stream:
        stream.bind(('127.0.0.1', 0))
        return stream.getsockname()[1]


def request(port, path):
    conn = http.client.HTTPConnection('127.0.0.1', port, timeout=10)
    try:
        conn.request('GET', path)
        response = conn.getresponse()
        response.read()
        return response.status
    finally:
        conn.close()


def connect(port):
    conn = socket.create_connection(('127.0.0.1', port), timeout=5)
    stream = conn.makefile('rwb', buffering=0)
    key = base64.b64encode(os.urandom(16)).decode()
    stream.write((f'GET {PATH} HTTP/1.1\r\nHost: localhost:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\n\r\n').encode())
    status = stream.readline().split()[1]
    headers = {}
    while (line := stream.readline()) not in [b'\r\n', b'']:
        name, value = line.decode().split(':', 1)
        headers[name.lower()] = value.strip()
    if status == b'101':
        expected = base64.b64encode(hashlib.sha1((key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
        assert headers['sec-websocket-accept'] == expected
    return conn, stream, int(status)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/artifactory-gateway'))
    parser.add_argument('--idle-seconds', type=float, default=32)
    args = parser.parse_args()
    upstream = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
    upstream.daemon_threads = True
    upstream.held, upstream.release = threading.Event(), threading.Event()
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    port = free_port()
    origin = f'http://127.0.0.1:{upstream.server_port}'
    env = dict(os.environ, ARTIFACTORY_INSECURE_DEV='true', ARTIFACTORY_GATEWAY_PROFILE='all',
               ARTIFACTORY_GATEWAY_TOKEN='isolated-terminal-gateway-' + 'a' * 32,
               ARTIFACTORY_GATEWAY_HTTP_BIND=f'127.0.0.1:{port}', ARTIFACTORY_GATEWAY_CONTROL=origin,
               ARTIFACTORY_GATEWAY_WEB=origin, ARTIFACTORY_GATEWAY_REGISTRY=origin,
               ARTIFACTORY_S3_ENDPOINT=origin, ARTIFACTORY_GATEWAY_HTTP_CONNECTIONS='1',
               ARTIFACTORY_GATEWAY_TERMINAL_CONNECTIONS='2')
    connections = []
    with tempfile.TemporaryFile() as log:
        process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 10
            while True:
                assert process.poll() is None, 'gateway exited'
                try:
                    if request(port, '/healthz') == 200:
                        break
                except OSError:
                    pass
                assert time.monotonic() < deadline, 'gateway startup timeout'
                time.sleep(.05)
            for _ in range(2):
                conn, stream, status = connect(port)
                connections.append((conn, stream))
                assert status == 101, f'terminal upgrade returned {status}'
            # 两条终端已占满终端预算；普通 HTTP 仍能进入上游。
            held = []
            pending = threading.Thread(target=lambda: held.append(request(port, '/api/hold')))
            pending.start()
            assert upstream.held.wait(5), 'terminal streams exhausted ordinary HTTP budget'
            conn, stream, status = connect(port)
            stream.close(); conn.close()
            assert status == 503, 'terminal budget did not reject excess stream'
            assert request(port, '/api/ordinary') == 503, 'ordinary HTTP budget did not remain independent'
            upstream.release.set()
            pending.join(10)
            assert held == [200]
            first = connections[0][1]
            for kind, data in [(1, '中文终端'.encode()), (2, bytes(range(256)) * 64), (9, b'ping')]:
                write_frame(first, kind, data, masked=True)
                assert read_frame(first) == (10 if kind == 9 else kind, data)
            print('PASS: real WebSocket upgrade, 16KiB bytes, Unicode, ping and isolated quotas', flush=True)
            time.sleep(args.idle_seconds)
            write_frame(first, 1, b'after-idle', masked=True)
            assert read_frame(first) == (1, b'after-idle')
            print(f'PASS: upgraded stream survived {args.idle_seconds:g}s idle', flush=True)
        finally:
            upstream.release.set()
            for conn, stream in connections:
                stream.close(); conn.close()
            process.terminate()
            try:
                process.wait(5)
            except subprocess.TimeoutExpired:
                process.kill(); process.wait(5)
            upstream.shutdown(); upstream.server_close()


if __name__ == '__main__':
    main()
