#!/usr/bin/env python3
"""[INPUT]: 回环HTTP对端、自有bucket与受监督的S3故障夹具。
[OUTPUT]: 验证单次403、签名头透传、正文一致和精确bucket边界。
[POS]: 传输故障夹具回归；真实mTLS续签由组合门禁证明。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

from check_emissary_legacy_sender import Supervisor


class S3FaultBoundary(unittest.TestCase):
    def test_only_first_armed_asset_get_is_denied_without_logging_signed_query(self):
        calls = []
        class Upstream(BaseHTTPRequestHandler):
            def log_message(self,*_args): pass
            def do_GET(self):
                calls.append((self.path,self.headers.get('Host'),self.headers.get('Authorization')))
                body = b'original upstream bytes'
                self.send_response(200)
                self.send_header('Content-Length',str(len(body)))
                self.send_header('ETag','"exact-object"')
                self.end_headers()
                self.wfile.write(body)
        server = ThreadingHTTPServer(('127.0.0.1',0),Upstream)
        thread = threading.Thread(target=server.serve_forever,daemon=True)
        thread.start()
        supervisor = Supervisor()
        bucket = 'emissary-acceptance-'+'1'*32
        try:
            with tempfile.TemporaryDirectory(prefix='emissary-s3-relay-test-') as temporary:
                root = Path(temporary)
                supervisor.start([sys.executable,Path(__file__).with_name('emissary_s3_fault.py'),
                    '--origin','http://127.0.0.1:'+str(server.server_port),'--bucket',bucket,
                    '--directory',root],cwd=root,env={'PATH':os.environ['PATH']},
                    log=root/'relay.log',label='owned-test-s3-relay')
                end = time.monotonic()+5
                while not (root/'address').exists() and time.monotonic()<end:
                    time.sleep(.02)
                base = (root/'address').read_text()
                client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                query = 'X-Amz-Signature=private-test-signature'
                path = '/'+bucket+'/emissary-releases/hash/asset?'+query
                headers = {'Authorization':'test-authorization','Host':'signed-host:8333'}
                def get(target):
                    request = urllib.request.Request(base+target,headers=headers)
                    return client.open(request,timeout=5)
                with get(path) as reply:
                    self.assertEqual(reply.read(),b'original upstream bytes')
                (root/'arm').touch()
                with self.assertRaises(urllib.error.HTTPError) as raised:
                    get(path)
                with raised.exception as error:
                    self.assertEqual(error.code,403)
                    self.assertEqual(error.read(),b'')
                with get(path) as reply:
                    self.assertEqual(reply.read(),b'original upstream bytes')
                    self.assertEqual(reply.headers['ETag'],'"exact-object"')
                self.assertEqual(calls,[(path,'signed-host:8333','test-authorization')]*2)
                with self.assertRaises(urllib.error.HTTPError) as raised:
                    get('/foreign-bucket/asset')
                with raised.exception as error:
                    self.assertEqual(error.code,403)
                self.assertEqual(len(calls),2)
                raw = (root/'requests.jsonl').read_text()
                records = [json.loads(line) for line in raw.splitlines()]
                self.assertEqual([record['denied'] for record in records],[False,True,False])
                self.assertTrue(all(record['query_sha256']==hashlib.sha256(query.encode()).hexdigest()
                                    for record in records))
                self.assertNotIn('private-test-signature',raw)
                self.assertNotIn('test-authorization',raw)
                supervisor.cleanup()
                self.assertTrue(supervisor.cleanup_confirmed())
        finally:
            supervisor.cleanup()
            server.shutdown()
            server.server_close()
            thread.join(timeout=3)


if __name__ == '__main__':
    unittest.main()
