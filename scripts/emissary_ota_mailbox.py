#!/usr/bin/env python3
"""[INPUT]: 本轮独占OTA目录内的真实续签请求与回复。
[OUTPUT]: 观察并校验冻结身份，只返回不含票据的摘要回执。
[POS]: 普通进程验收观察器；不写邮箱、不签发URL。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import hashlib
import json
import threading
import uuid


class RenewalObserver:
    def __init__(self,directory):
        self.directory = directory
        self.stop = threading.Event()
        self.requests,self.replies = {},{}
        self.error = None
        self.thread = threading.Thread(target=self.observe,daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self,*_args):
        self.stop.set()
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            raise RuntimeError('renewal observer did not stop')

    def observe(self):
        try:
            while not self.stop.is_set():
                for suffix,values in (('request',self.requests),('reply',self.replies)):
                    try:
                        value = json.loads((self.directory/('url-refresh-'+suffix+'.json')).read_bytes())
                    except FileNotFoundError:
                        continue
                    request = value if suffix=='request' else value['request']
                    identity = str(uuid.UUID(request['request_id']))
                    if len(values)>=8 and identity not in values:
                        raise RuntimeError('renewal observation exceeded budget')
                    values[identity] = value
                self.stop.wait(.001)
        except Exception as error:
            self.error = type(error).__name__

    def receipt(self, *, update, generation, manifest, asset):
        if self.error or self.thread.is_alive():
            raise RuntimeError('renewal observation unavailable')
        if len(self.requests)!=1 or set(self.requests)!=set(self.replies):
            raise AssertionError('matching actual renewal request/reply was not observed')
        identity,request = next(iter(self.requests.items()))
        reply = self.replies[identity]
        expected = {'request_id':identity,'update_id':update,'generation':generation,
                    'manifest_sha256':manifest,'asset_sha256':asset['sha256'],
                    'asset_size':asset['size'],'os':asset['os'],'arch':asset['arch']}
        assert request==expected and reply['request']==expected
        assert isinstance(reply['url'],str) and reply['url']
        return dict(expected,matching_private_reply_observed=True,
                    url_sha256=hashlib.sha256(reply['url'].encode()).hexdigest())
