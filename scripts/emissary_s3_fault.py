#!/usr/bin/env python3
"""[INPUT]: 回环S3地址、精确自有bucket及私有故障开关。
[OUTPUT]: 只对已武装资产GET拒绝一次，其余请求透传真实S3。
[POS]: OTA续签的传输故障夹具；不签发URL或伪造成功正文。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import threading
import time
import urllib.parse


def serve(origin, bucket, directory):
    target = urllib.parse.urlsplit(origin)
    if target.scheme!='http' or target.hostname!='127.0.0.1' or target.path not in ('','/'):
        raise RuntimeError('fault relay requires explicit loopback S3 origin')
    if not re.fullmatch(r'emissary-acceptance-[0-9a-f]{32}',bucket):
        raise RuntimeError('fault relay requires exact owned bucket')
    mutex, denied = threading.Lock(),False
    blocked = {'connection','transfer-encoding','keep-alive','proxy-connection'}
    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'
        def log_message(self,*_args):
            pass
        def do_HEAD(self): self.forward()
        def do_GET(self): self.forward()
        def do_PUT(self): self.forward()
        def do_DELETE(self): self.forward()
        def forward(self):
            nonlocal denied
            parsed = urllib.parse.urlsplit(self.path)
            if parsed.scheme or parsed.netloc or not (parsed.path=='/'+bucket or parsed.path.startswith('/'+bucket+'/')):
                self.send_error(403)
                return
            with mutex:
                reject = bool(self.command=='GET' and '/emissary-releases/' in parsed.path
                              and (directory/'arm').exists() and not denied)
                denied |= reject
                if self.command=='GET' and '/emissary-releases/' in parsed.path:
                    with (directory/'requests.jsonl').open('a') as output:
                        output.write(json.dumps({'path':parsed.path,'denied':reject,
                            'query_sha256':hashlib.sha256(parsed.query.encode()).hexdigest()})+'\n')
            if reject:
                # 让下一张S3签名票据具有独立签发秒，不延长产品期限。
                time.sleep(1.1)
                self.send_response(403)
                self.send_header('Content-Length','0')
                self.end_headers()
                return
            length = int(self.headers.get('Content-Length','0'))
            if length<0 or length>128*1024*1024 or self.headers.get('Transfer-Encoding'):
                self.send_error(413)
                return
            connection = http.client.HTTPConnection(target.hostname,target.port,timeout=15)
            try:
                headers = {key:value for key,value in self.headers.items() if key.lower() not in blocked}
                connection.request(self.command,self.path,self.rfile.read(length),headers)
                response = connection.getresponse()
                self.send_response(response.status)
                for key,value in response.getheaders():
                    if key.lower() not in blocked:
                        self.send_header(key,value)
                self.send_header('Connection','close')
                self.end_headers()
                self.close_connection = True
                if self.command!='HEAD':
                    while chunk:=response.read(65536):
                        self.wfile.write(chunk)
            finally:
                connection.close()
    server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
    (directory/'address').write_text('http://127.0.0.1:'+str(server.server_port))
    server.serve_forever(poll_interval=.1)


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--origin',required=True)
    parser.add_argument('--bucket',required=True)
    parser.add_argument('--directory',required=True,type=Path)
    args = parser.parse_args()
    serve(args.origin,args.bucket,args.directory)
