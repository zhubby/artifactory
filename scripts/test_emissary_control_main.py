#!/usr/bin/env python3
"""[INPUT]: 真实Control门禁的环境、容器端口与精确资源回收边界。
[OUTPUT]: 验证资源边界、刷新原摘要与工具夹具，拒绝未确认结果。
[POS]: 无服务故障回归；完整注册与操作仍由实际main门禁验证。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
import hashlib
import contextlib
import io
import os
from pathlib import Path
import sys
import tempfile
import subprocess
import uuid
import unittest
from unittest.mock import patch

import check_emissary_control_main as driver
import emissary_controlled_refresh as refresh


class ResourceBoundaries(unittest.TestCase):
    def test_refresh_source_decoder_verifies_delivery_digest_and_native_identity(self):
        with tempfile.TemporaryDirectory(prefix='controlled-refresh-decode-') as temporary:
            path = Path(temporary)/'source.inventory'
            observation,startup = str(uuid.uuid4()),str(uuid.uuid4())
            inventory = b'{"os":"linux","arch":"arm64"}'
            observed,monotonic = 1795643965000,123
            fields = [observation.encode(),startup.encode(),(1).to_bytes(8,'big'),observed.to_bytes(8,'big'),monotonic.to_bytes(8,'big'),(1).to_bytes(4,'big'),inventory,b'']
            digest = hashlib.sha256(b'artifactory.inventory.delivery.v1\0')
            for field in fields:
                digest.update(len(field).to_bytes(8,'big')); digest.update(field)
            def encode(delivery_digest):
                text = f'observation_id: "{observation}"\nsource_startup_id: "{startup}"\nrevision: 1\nobserved_at_ms: {observed}\nmonotonic_ms: {monotonic}\ninventory_schema: 1\ninventory_json: '+json.dumps(inventory.decode())+'\ndelivery_digest: '+json.dumps(delivery_digest)+'\n'
                path.write_bytes(subprocess.run(['protoc','--encode=artifactory.emissary.v1.InventoryObservation','-I',str(driver.ROOT/'proto'),'artifactory/emissary/v1/emissary.proto'],input=text.encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True,timeout=5).stdout)
            encode(digest.hexdigest())
            value = refresh.decode_observation(path,driver.runtime_environment())
            self.assertEqual(value['inventory'],json.loads(inventory))
            self.assertEqual(value['revision'],1)
            self.assertEqual(value['source_batch_id'],'')
            self.assertEqual(value['observed_at_ms'],observed)
            encode('a'*64)
            with self.assertRaisesRegex(AssertionError,'delivery digest differs'):
                refresh.decode_observation(path,driver.runtime_environment())

    def test_controlled_tools_use_shared_catalog_and_change_versions_without_config_rewrite(self):
        with tempfile.TemporaryDirectory(prefix='controlled-refresh-tools-') as temporary:
            cfg = {'tools':{}}
            root = refresh.prepare(Path(temporary),cfg)
            self.assertEqual(set(cfg['tools']),set(refresh.probe_ids()))
            original = dict(cfg['tools'])
            for name in refresh.probe_ids():
                reply = subprocess.run([cfg['tools'][name],'ignored','; exit 7'],capture_output=True,check=True,timeout=3)
                self.assertEqual(reply.stdout,b'controlled probe 1.2.3\n')
            self.assertTrue(all(value==b'x' for value in refresh.counts(root).values()))
            refresh.write_version(root,'9.8.7')
            self.assertEqual(cfg['tools'],original)
            reply = subprocess.run([cfg['tools']['git'],'--version'],capture_output=True,check=True,timeout=3)
            self.assertEqual(reply.stdout,b'controlled probe 9.8.7\n')
            self.assertEqual(refresh.counts(root)['git'],b'xx')
            with self.assertRaisesRegex(RuntimeError,'invalid controlled probe version'):
                refresh.write_version(root,'9.8.7; exit 7')

    def test_restart_probe_is_bounded_and_counts_only_armed_diagnostics(self):
        with tempfile.TemporaryDirectory(prefix='control-restart-probe-') as temporary:
            cfg = {'tools':{}}
            probe = driver.prepare_restart_probe(Path(temporary),cfg)
            self.assertEqual(cfg['tools']['git'],str(probe))
            initial = subprocess.run([probe,'--version'],capture_output=True,timeout=3,check=True)
            self.assertEqual(initial.stdout,b'git version 2.40.1\n')
            self.assertFalse(Path(str(probe)+'.count').exists())
            Path(str(probe)+'.arm').touch()
            timed_out = subprocess.run([probe,'--version'],capture_output=True,timeout=3)
            self.assertEqual(timed_out.returncode,71)
            self.assertTrue(Path(str(probe)+'.ready').exists())
            Path(str(probe)+'.go').touch()
            subprocess.run([probe,'--version'],capture_output=True,timeout=3,check=True)
            self.assertEqual(Path(str(probe)+'.count').read_bytes(),b'xx')

    def test_actual_proto_source_decode_keeps_identity_digest_and_time(self):
        with tempfile.TemporaryDirectory(prefix='control-source-') as temporary:
            root = Path(temporary)
            observation, startup = str(uuid.uuid4()),str(uuid.uuid4())
            saved = {'observation_id':observation,'startup_id':startup,'acknowledged':False}
            (root/'startup.json').write_text(json.dumps(saved))
            text = f'observation_id: "{observation}"\nsource_startup_id: "{startup}"\nobserved_at_ms: 1795643965000\ninventory_schema: 1\ndelivery_digest: "'+('a'*64)+'"\n'
            body = subprocess.run(['protoc','--encode=artifactory.emissary.v1.InventoryObservation',
                                   '-I',str(driver.ROOT/'proto'),'artifactory/emissary/v1/emissary.proto'],
                                  input=text.encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                                  check=True,timeout=5).stdout
            (root/(observation+'.inventory')).write_bytes(body)
            value = driver.inventory_source(root,driver.runtime_environment())
            self.assertEqual(value['bytes'],body)
            self.assertEqual(value['startup'],startup)
            self.assertEqual(value['observed_at_ms'],1795643965000)
            self.assertEqual(value['delivery_digest'],'a'*64)
            saved['startup_id'] = str(uuid.uuid4())
            (root/'startup.json').write_text(json.dumps(saved))
            with self.assertRaisesRegex(RuntimeError,'identity differs'):
                driver.inventory_source(root,driver.runtime_environment())

    def test_startup_ack_requires_original_observation_and_source_removal(self):
        with tempfile.TemporaryDirectory(prefix='control-startup-ack-') as temporary:
            root = Path(temporary)
            path = root/'startup.json'
            source = {'observation':'original','path':root/'original.inventory'}
            checkpoint = {'startup_id':'startup','observation_id':'original','acknowledged':False}
            def check():
                path.write_text(json.dumps(checkpoint))
                return driver.startup_acknowledged(path,'startup',source)
            source['path'].write_bytes(b'original')
            self.assertFalse(check())
            checkpoint['acknowledged'] = True
            self.assertFalse(check())
            source['path'].unlink()
            self.assertTrue(check())
            checkpoint['observation_id'] = 'replacement'
            self.assertFalse(check())
            checkpoint['observation_id'] = 'original'
            checkpoint['startup_id'] = 'other-startup'
            self.assertFalse(check())

    def test_device_operation_ack_requires_matching_identity_state_and_cleared_body(self):
        with tempfile.TemporaryDirectory(prefix='control-operation-ack-') as temporary:
            path = Path(temporary)/'journal.json'
            expected = {'id':'operation','tenant_id':'tenant','node_id':'node','startup_id':'startup',
                        'control_session':'session','actor_id':'actor','kind':'collect_diagnostics',
                        'schema':1,'input_digest':'input','state':'succeeded','sequence':3,
                        'reason':None,'result_digest':'digest'}
            entry = {'envelope':dict(expected,operation_id='operation'),'acknowledged':False,
                     'created_at':'2026-11-25T22:00:00Z','finished_at':'2026-11-25T22:00:01Z',
                     'update':dict(expected,operation_id='operation',result={})}
            journal = {'entries':[entry],'result_expired':0}
            def check():
                path.write_text(json.dumps(journal))
                return driver.operation_acknowledged(path,expected)
            self.assertFalse(check())
            entry['acknowledged'] = True
            self.assertFalse(check())
            entry['update']['result'] = None
            self.assertTrue(check())
            for section,key,value in [('envelope','node_id','other'),('envelope','startup_id','other'),
                                      ('envelope','kind','refresh_inventory'),('update','operation_id','other'),
                                      ('update','state','failed'),('update','sequence',2),
                                      ('update','result_digest','other')]:
                previous = entry[section][key]
                entry[section][key] = value
                self.assertFalse(check(),section+'.'+key)
                entry[section][key] = previous
            journal['result_expired'] = 1
            self.assertFalse(check())
            journal['result_expired'] = 0
            entry['finished_at'] = None
            self.assertFalse(check())

    def test_early_dependency_failure_replaces_prior_passed_evidence(self):
        previous = os.umask(0o077)
        self.addCleanup(os.umask,previous)
        with tempfile.TemporaryDirectory(prefix='control-main-early-failure-') as temporary:
            root = Path(temporary)
            directory = root/'private'
            directory.mkdir()
            evidence = root/'evidence.json'
            evidence.write_text(json.dumps({'status':'passed','nonce':'old'}))
            arguments = ['driver','--database-container','test-db','--s3-container','test-s3',
                         '--evidence',str(evidence)]
            with patch.object(sys,'argv',arguments), patch.object(driver.tempfile,'mkdtemp',return_value=str(directory)), \
                    patch.object(driver.legacy,'source_manifest',return_value='current'), \
                    patch.object(driver.legacy,'database_url',side_effect=RuntimeError('dependency unavailable')), \
                    contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(RuntimeError,'dependency unavailable'):
                    driver.main()
            value = json.loads(evidence.read_text())
            self.assertEqual(value['status'],'failed')
            self.assertEqual(value['phase'],'prepare')
            self.assertNotEqual(value['nonce'],'old')
            self.assertIn('bucket',json.loads((directory/'resources.json').read_text()))

    def test_runtime_does_not_inherit_credentials_or_optional_external_services(self):
        environment = {'PATH':'/usr/bin', 'HOME':'/private/test', 'DATABASE_URL':'private',
                       'ARTIFACTORY_S3_SECRET_KEY':'private', 'ARTIFACTORY_PLANNING_URL':'https://external',
                       'KUBECONFIG':'/private/config', 'HTTP_PROXY':'http://external', 'PGPASSWORD':'private'}
        with patch.dict(driver.os.environ, environment, clear=True):
            self.assertEqual(driver.runtime_environment(), {'PATH':'/usr/bin', 'HOME':'/private/test'})

    def test_s3_rejects_non_loopback_or_ambiguous_published_ports(self):
        details = {'Config':{'Image':'seaweedfs:4.53'},
                   'NetworkSettings':{'Ports':{'8333/tcp':[{'HostIp':'127.0.0.1','HostPort':'19000'}]}}}
        self.assertEqual(driver.s3_host(details),'127.0.0.1:19000')
        for bindings in ([{'HostIp':'0.0.0.0','HostPort':'19000'}], [],
                         [{'HostIp':'127.0.0.1','HostPort':'19000'}]*2):
            details['NetworkSettings']['Ports']['8333/tcp'] = bindings
            with self.assertRaises(RuntimeError):
                driver.s3_host(details)

    def test_unmarked_database_is_retained_after_create_comment_interruption(self):
        statements = []
        def sql(_name, statement):
            statements.append(statement)
            return json.dumps({'marker':None})
        with self.assertRaisesRegex(RuntimeError,'ownership differs'):
            driver.cleanup_databases(sql,'business','telemetry','role','nonce')
        self.assertFalse(any('DROP' in item for item in statements))

    def test_foreign_role_is_retained_and_absent_databases_are_read_back(self):
        statements = []
        def sql(_name, statement):
            statements.append(statement)
            if 'json_build_object' in statement:
                return json.dumps({'marker':'foreign'}) if 'pg_roles' in statement else ''
            return '0'
        with self.assertRaisesRegex(RuntimeError,'role ownership differs'):
            driver.cleanup_databases(sql,'business','telemetry','role','nonce')
        self.assertFalse(any('DROP' in item for item in statements))

    def test_owned_resources_require_absence_after_drop(self):
        statements = []
        def sql(_name, statement):
            statements.append(statement)
            if 'json_build_object' in statement:
                return json.dumps({'marker':'nonce'})
            if 'count(*)' in statement:
                return '1'
            return ''
        with self.assertRaisesRegex(RuntimeError,'deletion readback differs'):
            driver.cleanup_databases(sql,'business','telemetry','role','nonce')
        self.assertIn('DROP DATABASE telemetry WITH (FORCE);',statements)
        self.assertFalse(any('DROP ROLE' in item for item in statements))


if __name__ == '__main__':
    unittest.main()
