#!/usr/bin/env python3
"""[INPUT]: 私有临时续签邮箱及原子发布的冻结请求与回复。
[OUTPUT]: 验证观察器匹配身份、拒绝缺失回复并隐藏URL正文。
[POS]: 续签验收证据回归，不调用产品RPC。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
from pathlib import Path
import tempfile
import time
import unittest
import uuid

from check_emissary_legacy_sender import private_json
from emissary_ota_mailbox import RenewalObserver


class RenewalEvidence(unittest.TestCase):
    def test_exact_reply_is_observed_and_url_is_only_returned_as_digest(self):
        with tempfile.TemporaryDirectory(prefix='ota-mailbox-test-') as temporary:
            root = Path(temporary)
            request = {'request_id':str(uuid.uuid4()),'update_id':str(uuid.uuid4()),'generation':3,
                       'manifest_sha256':'a'*64,'asset_sha256':'b'*64,'asset_size':42,
                       'os':'darwin','arch':'arm64'}
            with RenewalObserver(root) as observer:
                private_json(root/'url-refresh-request.json',request)
                private_json(root/'url-refresh-reply.json',{'request':request,
                    'url':'http://127.0.0.1/artifacts/object?ticket=private-fixture'})
                end = time.monotonic()+3
                while not observer.replies and time.monotonic()<end:
                    time.sleep(.002)
            asset = {'sha256':'b'*64,'size':42,'os':'darwin','arch':'arm64'}
            proof = observer.receipt(update=request['update_id'],generation=3,manifest='a'*64,asset=asset)
            self.assertTrue(proof['matching_private_reply_observed'])
            self.assertEqual(proof['request_id'],request['request_id'])
            self.assertNotIn('private-fixture',json.dumps(proof))
            self.assertEqual(len(proof['url_sha256']),64)
            with self.assertRaises(AssertionError):
                observer.receipt(update=request['update_id'],generation=4,manifest='a'*64,asset=asset)

    def test_request_without_actual_reply_cannot_prove_renewal(self):
        with tempfile.TemporaryDirectory(prefix='ota-mailbox-test-') as temporary:
            root = Path(temporary)
            with RenewalObserver(root) as observer:
                private_json(root/'url-refresh-request.json',{'request_id':str(uuid.uuid4())})
                end = time.monotonic()+3
                while not observer.requests and time.monotonic()<end:
                    time.sleep(.002)
            with self.assertRaisesRegex(AssertionError,'matching actual renewal'):
                observer.receipt(update='id',generation=1,manifest='a'*64,asset={})


if __name__ == '__main__':
    unittest.main()
