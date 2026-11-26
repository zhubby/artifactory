#!/usr/bin/env python3
"""[INPUT]: 真实Control门禁、共享探测目录与独占Telemetry事务围栏。
[OUTPUT]: 验证成功刷新、原观察事务确认和启动调度快照不变。
[POS]: 显式私有工具夹具；不声称宿主已安装对应工具链。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import ast
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid

import check_emissary_legacy_receiver as legacy
from check_emissary_legacy_sender import private_json


def probe_ids():
    source = (legacy.ROOT/'crates/emissary-protocol/src/operations.rs').read_text()
    block = re.search(r'pub const PROBE_IDS: &\[&str\] = &\[(.*?)\];',source,re.S)
    if block is None:
        raise RuntimeError('shared probe catalog missing')
    ids = re.findall(r'"([a-z][a-z0-9-]*)"',block[1])
    if not ids or len(ids)!=len(set(ids)):
        raise RuntimeError('invalid shared probe catalog')
    return ids


def prepare(directory,config):
    root = directory/'controlled-tools'
    root.mkdir(mode=0o700)
    config['tools'].update({name:str(root/name) for name in probe_ids()})
    write_version(root,'1.2.3')
    return root


def write_version(root,version):
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+',version):
        raise RuntimeError('invalid controlled probe version')
    for name in probe_ids():
        target = root/name
        temporary = root/(name+'.new')
        temporary.write_text('#!/bin/sh\nprintf x >> "$0.count"\n'
                             "printf 'controlled probe "+version+"\\n'\n")
        temporary.chmod(0o700)
        os.replace(temporary,target)


def counts(root):
    return {name:(root/(name+'.count')).read_bytes() for name in probe_ids()}


def decode_observation(path,environment):
    body = path.read_bytes()
    decoded = subprocess.run(['protoc','--decode=artifactory.emissary.v1.InventoryObservation',
                              '-I',str(legacy.ROOT/'proto'),'artifactory/emissary/v1/emissary.proto'],
                             input=body,env=environment,stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE,check=True,timeout=5).stdout.decode()
    def bytes_field(name):
        match = re.search(r'^'+name+r': (".*")$',decoded,re.M)
        return ast.literal_eval('b'+match[1]) if match else b''
    def integer(name):
        match = re.search(r'^'+name+r': ([0-9]+)$',decoded,re.M)
        return int(match[1]) if match else 0
    fields = [bytes_field('observation_id'),bytes_field('source_startup_id'),
              integer('revision').to_bytes(8,'big'),integer('observed_at_ms').to_bytes(8,'big'),
              integer('monotonic_ms').to_bytes(8,'big'),integer('inventory_schema').to_bytes(4,'big'),
              bytes_field('inventory_json'),bytes_field('source_batch_id')]
    digest = hashlib.sha256(b'artifactory.inventory.delivery.v1\0')
    for field in fields:
        digest.update(len(field).to_bytes(8,'big'))
        digest.update(field)
    value = {'observation':str(uuid.UUID(fields[0].decode())),
             'startup':str(uuid.UUID(fields[1].decode())),
             'revision':integer('revision'),'observed_at_ms':integer('observed_at_ms'),
             'digest':bytes_field('delivery_digest').decode(),
             'inventory':json.loads(fields[6]),'source_batch_id':fields[7].decode(),
             'schema':integer('inventory_schema'),'path':path,'bytes':body}
    assert value['digest']==digest.hexdigest(), 'refresh source delivery digest differs'
    return value


def exercise(*,directory,tools,node,startup,supervisor,pg_environment,database,environment,
             nonce,sql,business_sql,api,wait,operation_acknowledged):
    snapshot_sql = f"SELECT json_build_object('metadata',metadata,'digest',metadata_digest,'received_at',metadata_received_at,'capabilities',capabilities,'startup',startup_id) FROM nodes WHERE id='{node}'"
    snapshot = business_sql(snapshot_sql)
    startup_sql = f"SELECT to_jsonb(s) FROM node_startups s WHERE node_id='{node}' AND startup_id='{startup}'"
    startup_row = business_sql(startup_sql)
    old = api('/api/v1/nodes/'+node+'/telemetry')['inventory']
    assert old['revision']==0 and old['startup_id']==startup
    assert all(old['inventory']['tools'][name]['version']=='1.2.3' for name in probe_ids())
    before = counts(tools)
    assert all(value==b'x' for value in before.values()), 'startup must probe each fixture once'
    write_version(tools,'9.8.7')
    ready = directory/'refresh-fence-ready'
    script = directory/'refresh-fence.sql'
    script.write_text(f"BEGIN;\nLOCK TABLE inventory_deliveries IN SHARE MODE;\n\\o {ready}\nSELECT 'locked';\n\\o\nSELECT pg_sleep(40);\nROLLBACK;\n")
    application = 'refresh_fence_'+nonce
    holder = supervisor.start(['psql','-X','-qAt','-v','ON_ERROR_STOP=1','-v','VERBOSITY=sqlstate',
                               '-d',database,'-f',script],cwd=legacy.ROOT,
                              env=dict(pg_environment,PGAPPNAME=application,
                                       PGOPTIONS='-c statement_timeout=45000'),
                              log=directory/'refresh-fence.log',label='refresh-fence')
    wait(lambda:ready.exists(),'refresh transaction fence acquired')
    pid = int(wait(lambda:sql(f"SELECT pid FROM pg_stat_activity WHERE application_name='{application}' AND datname=current_database()"),'refresh fence backend'))
    body = {'request_id':str(uuid.uuid4()),'kind':'refresh_inventory','schema':1,'input':{'mode':'full'}}
    operation = api('/api/v1/nodes/'+node+'/operations',body)
    operation_id = str(uuid.UUID(operation['id']))
    def finished():
        value = api('/api/v1/node-operations/'+operation_id)
        return value if value['state'] in ('succeeded','failed','rejected','interrupted','expired','canceled') else None
    result = wait(finished,'controlled full refresh result',20)
    assert result['state']=='succeeded' and result['delivery_state']=='confirmed'
    assert result['result_digest'] and not result['result']['skipped']
    assert not result['result']['truncated'] and result['result']['omitted']==0
    assert result['startup_id']==startup and result['input']==body['input'] and result['request_id']==body['request_id']
    reports = result['result']['probes']
    assert set(reports)==set(probe_ids()) and all(report['status']=='available' and report['version']=='9.8.7' for report in reports.values())
    refresh = result['result']['inventory']
    assert refresh['revision']==1 and refresh['delivery']=='pending'
    observation = str(uuid.UUID(refresh['observation_id']))
    path = directory/'node/telemetry/inventories'/(observation+'.inventory')
    source = decode_observation(path,environment)
    assert source['observation']==observation and source['startup']==startup
    assert source['revision']==1 and source['schema']==1 and source['source_batch_id']==''
    assert all(source['inventory']['tools'][name]['version']=='9.8.7' for name in probe_ids())
    wait(lambda:sql(f"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND {pid}=ANY(pg_blocking_pids(pid)) AND query LIKE 'INSERT INTO inventory_deliveries%')")=='t','actual refresh ingest transaction waiting',10)
    assert sql(f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND observation_id='{observation}'")=='0'
    assert api('/api/v1/nodes/'+node+'/telemetry')['inventory']==old
    assert path.read_bytes()==source['bytes']
    wait(lambda:operation_acknowledged(directory/'node/operations/journal.json',result),'controlled operation result ACK')
    assert path.exists(), 'operation result ACK must not acknowledge independent inventory'
    assert sql(f"SELECT pg_cancel_backend(pid) FROM pg_stat_activity WHERE pid={pid} AND application_name='{application}' AND datname=current_database()")=='t'
    assert holder.wait(10)!=0 and '57014' in (directory/'refresh-fence.log').read_text()
    assert sql(f"SELECT count(*) FROM pg_stat_activity WHERE pid={pid} AND application_name='{application}'")=='0'
    query = f"SELECT json_build_object('digest',d.delivery_digest,'time',(extract(epoch FROM d.observed_at)*1000)::bigint,'inventory',v.inventory) FROM inventory_deliveries d JOIN inventory_versions v ON v.node_id=d.node_id AND v.digest=d.inventory_digest WHERE d.node_id='{node}' AND d.observation_id='{observation}' AND d.startup_id='{startup}' AND d.revision=1 AND d.source_batch_id IS NULL"
    received = json.loads(wait(lambda:sql(query),'controlled refresh receipt committed',30))
    assert received=={'digest':source['digest'],'time':source['observed_at_ms'],'inventory':source['inventory']}
    wait(lambda:not path.exists(),'controlled original observation ACK and source removal',20)
    latest = api('/api/v1/nodes/'+node+'/telemetry')['inventory']
    assert latest['observation_id']==observation and latest['revision']==1
    assert latest['provenance']=='native' and latest['startup_id']==startup
    assert latest['inventory']==source['inventory']
    assert business_sql(snapshot_sql)==snapshot and business_sql(startup_sql)==startup_row
    after = counts(tools)
    assert all(after[name]==before[name]+b'x' for name in before)
    duplicate = api('/api/v1/nodes/'+node+'/operations',body)
    for key in ('id','result','result_digest','sequence','issued_at','expires_at','finished_at'):
        assert duplicate[key]==result[key]
    time.sleep(2)
    assert counts(tools)==after
    checkpoint = json.loads((path.parent/'revision.json').read_bytes())
    assert checkpoint=={'startup_id':startup,'last':1}
    assert sql(f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND startup_id='{startup}' AND revision>0")=='1'
    assert business_sql(snapshot_sql)==snapshot and business_sql(startup_sql)==startup_row
    audit = json.loads(business_sql(f"SELECT json_object_agg(action,total) FROM (SELECT action,count(*) AS total FROM audit_events WHERE resource_id='{operation_id}' GROUP BY action) counts"))
    assert audit=={'node_operation_created':1,'node_operation_finished':1}
    assert json.loads(sql(f"SELECT json_build_object('inventory',v.inventory,'observation',d.observation_id) FROM inventory_deliveries d JOIN inventory_versions v ON v.node_id=d.node_id AND v.digest=d.inventory_digest WHERE d.node_id='{node}' AND d.startup_id='{startup}' AND d.revision=0 AND d.source_batch_id IS NULL"))=={'inventory':old['inventory'],'observation':old['observation_id']}
    private_json(directory/'controlled-refresh-result.json',result)
    return {'controlled_tool_fixture':True,'probe_count':len(before),'all_changed_versions_observed':True,
            'real_ingest_transaction_wait':True,'operation_ack_before_inventory_commit':True,
            'original_inventory_digest_and_time':True,'original_source_removed_after_ack':True,
            'immutable_scheduling_snapshot':True,'duplicate_request_no_probe_or_revision':True,
            'single_create_and_finish_audit':True,'original_revision_zero_preserved':True,
            'inventory_revision':1,'source_sha256':hashlib.sha256(source['bytes']).hexdigest()}
