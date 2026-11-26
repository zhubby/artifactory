#!/usr/bin/env python3
"""[INPUT]: 私有发行目录与当前平台真实候选程序。
[OUTPUT]: 提供只读回环发行API，记录导入请求。
[POS]: 普通进程OTA的外部发行源夹具；不模拟Control或网关。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import os
import urllib.parse


def routes(directory, version):
    manifest = json.loads((directory/'emissary-manifest.json').read_bytes())
    names = ['emissary-manifest.json', 'emissary-manifest.sig']
    names.extend(asset['name'] for asset in manifest['assets'])
    if manifest['version'] != version or len(names) != 8 or len(set(names)) != 8:
        raise RuntimeError('fixture manifest differs from expected release')
    prefix = '/repos/acceptance/emissary/releases/'
    release = {'tag_name':'v'+version, 'draft':False,
               'assets':[{'id':index, 'name':name} for index,name in enumerate(names,1)]}
    values = {prefix+'tags/v'+version:json.dumps(release).encode()}
    for index,name in enumerate(names,1):
        if Path(name).name != name:
            raise RuntimeError('fixture asset escaped release directory')
        values[prefix+'assets/'+str(index)] = directory/name
    return values


def serve(directory, version, address):
    values = routes(directory,version)
    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'
        def log_message(self, *_args):
            pass
        def do_GET(self):
            parsed = urllib.parse.urlsplit(self.path)
            value = values.get(parsed.path) if not parsed.query and not parsed.fragment else None
            if value is None:
                self.send_error(404)
                return
            body = value.read_bytes() if isinstance(value,Path) else value
            with (directory/'requests.jsonl').open('a') as output:
                output.write(json.dumps({'path':parsed.path,'bytes':len(body)})+'\n')
            self.send_response(200)
            self.send_header('Content-Length',str(len(body)))
            self.send_header('Content-Type','application/octet-stream')
            self.end_headers()
            self.wfile.write(body)
    server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
    address.write_text('http://127.0.0.1:'+str(server.server_port))
    server.serve_forever(poll_interval=.1)


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory',required=True,type=Path)
    parser.add_argument('--version',required=True)
    parser.add_argument('--address',required=True,type=Path)
    arguments = parser.parse_args()
    serve(arguments.directory,arguments.version,arguments.address)
