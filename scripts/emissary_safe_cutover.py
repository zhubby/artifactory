#!/usr/bin/env python3
"""[INPUT]: 冻结旧程序、当前程序、隔离双库与既有进程监督器。
[OUTPUT]: 验证停旧迁移、双副本读删及同源码兼容副本补交原件。
[POS]: 普通进程切换门禁；不证明历史版本回退或随机混部。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import shutil
import ssl
import urllib.parse
import urllib.request
import uuid

import check_emissary_control_main as driver
import check_emissary_legacy_receiver as legacy
import emissary_controlled_refresh as refresh
import emissary_legacy_retention as frozen_source
from check_emissary_legacy_sender import private_json


def attributed(environment, application):
    result = dict(environment)
    for key in ('DATABASE_URL', 'ARTIFACTORY_BUSINESS_DATABASE_URL'):
        if key in result:
            url = urllib.parse.urlsplit(result[key])
            query = urllib.parse.parse_qsl(url.query)
            if any(name == 'application_name' for name, _ in query):
                raise RuntimeError('database application identity already set')
            result[key] = urllib.parse.urlunsplit(url._replace(
                query=urllib.parse.urlencode(query+[('application_name', application)])))
    return result


def require_stopped(children, applications, sql, databases):
    if not children or any(not child.confirmed or child.process.returncode is None
                           or child.cleanup_failed for child in children):
        raise RuntimeError('replica stop is not confirmed; next phase forbidden')
    for database in databases:
        quoted = ','.join("'"+name+"'" for name in applications)
        if sql(database, 'SELECT count(*) FROM pg_stat_activity WHERE '
               'datname=current_database() AND application_name IN ('+quoted+')') != '0':
            raise RuntimeError('replica database session remains; next phase forbidden')


def freeze_binaries(binaries, directory):
    directory.mkdir(mode=0o700)
    copies = {}
    for name in ('artifactory-control-plane', 'artifactory-telemetry-service'):
        destination = directory/name
        with destination.open('xb') as target, binaries[name].open('rb') as source:
            shutil.copyfileobj(source, target)
            target.flush()
            os.fsync(target.fileno())
        destination.chmod(0o700)
        if destination.resolve() == binaries[name].resolve() or legacy.digest(destination) != legacy.digest(binaries[name]):
            raise RuntimeError('compatible recovery copy differs')
        copies[name] = destination
    descriptor = os.open(directory, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    return copies


def verify_artifact(path, expected):
    if legacy.digest(path) != expected:
        raise RuntimeError('frozen executable changed; launch forbidden')


def require_connections(sql, databases, application):
    return all(int(sql(database,"SELECT count(*) FROM pg_stat_activity WHERE "
                       "datname=current_database() AND application_name='"+application+"'"))>0
               for database in databases)


def exercise(*, supervisor, directory, environment, binaries, frozen, control_environment,
             telemetry_environment, business, telemetry, role, nonce, sql, run, stop,
             address, wait, proof):
    binaries = dict(binaries,**freeze_binaries(binaries,directory/'current-artifacts'))
    frozen = freeze_binaries(frozen,directory/'old-artifacts')
    recovery = freeze_binaries(binaries, directory/'compatible-recovery')
    hashes = {name: legacy.digest(path) for name, path in recovery.items()}
    artifacts = {str(path):legacy.digest(path) for paths in (binaries,frozen,recovery) for path in paths.values()}
    processes = []
    context = ssl.create_default_context(cafile=directory/'pki/ca.pem')
    client = urllib.request.build_opener(urllib.request.ProxyHandler({}),
        urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()),
        urllib.request.HTTPSHandler(context=context))
    origin = control_environment['ARTIFACTORY_PUBLIC_URL']
    tenant = '00000000-0000-0000-0000-000000000001'
    token = telemetry_environment['ARTIFACTORY_TELEMETRY_TOKEN']
    applications, current, old = [], [], []

    def request(base, path, body=None, service=False):
        headers = {'Content-Type':'application/json'}
        if service:
            headers['Authorization'] = 'Bearer '+token
        else:
            headers.update(Origin=origin, **{'x-artifactory-csrf':'1','x-artifactory-tenant':tenant})
        req = urllib.request.Request(base+path, headers=headers,
            data=None if body is None else json.dumps(body).encode())
        with client.open(req, timeout=5) as response:
            value = response.read()
            return json.loads(value) if value else None

    def ready(base, service=False):
        try:
            return request(base, '/metrics' if service else '/healthz', service=service) if not service else metrics(base)
        except (OSError, ValueError):
            return None

    def metrics(base):
        req = urllib.request.Request(base+'/metrics',headers={'Authorization':'Bearer '+token})
        with client.open(req,timeout=3) as response:
            return response.status == 200 and bool(response.read())

    def launch(paths, kind, env, label, group):
        application = 'cutover_'+nonce+'_'+label.replace('-','_')
        applications.append(application)
        verify_artifact(paths[kind],artifacts[str(paths[kind])])
        child = supervisor.start([paths[kind]], cwd=legacy.ROOT,
            env=attributed(env,application),log=directory/(label+'.log'),label=label)
        group.append(child)
        databases = sorted(set(urllib.parse.urlsplit(env[key]).path.removeprefix('/')
            for key in ('DATABASE_URL','ARTIFACTORY_BUSINESS_DATABASE_URL') if key in env))
        wait(lambda:require_connections(sql,databases,application),'actual replica database attribution')
        processes.append({'role':label,'executable':str(paths[kind]),'sha256':artifacts[str(paths[kind])],
            'pid':child.process.pid,'application_name':application,'databases':databases,
            'binds':{key:value for key,value in env.items() if key.endswith('_BIND')},
            'database_connections_observed':True})
        return child

    def ledgers():
        return {name:sql(name,'SELECT json_agg(to_jsonb(m) ORDER BY id) FROM __toasty_migrations m')
                for name in (business,telemetry)}

    def pair(index, prefix, original_control=control_environment, original_telemetry=telemetry_environment):
        c,t = dict(original_control),dict(original_telemetry)
        if index:
            c.update(ARTIFACTORY_HTTP_BIND=address(),ARTIFACTORY_GRPC_BIND=address(),
                     ARTIFACTORY_DATA_DIR=str(directory/(prefix+'-control-data')))
            t.update(ARTIFACTORY_TELEMETRY_HTTP_BIND=address(),ARTIFACTORY_TELEMETRY_GRPC_BIND=address())
            c.update(ARTIFACTORY_TELEMETRY_ENDPOINT='https://'+t['ARTIFACTORY_TELEMETRY_GRPC_BIND'],
                     ARTIFACTORY_TELEMETRY_URL='https://'+t['ARTIFACTORY_TELEMETRY_HTTP_BIND'])
        return c,t

    # 旧副本全部停止后才运行当前迁移；不使用故意跨迁移存活的Retention夹具。
    proof['phase'] = 'cutover-old'
    run([frozen['artifactory-control-plane'],'migrate'],control_environment,45)
    sql(business,f'GRANT CONNECT ON DATABASE {business} TO {role}; GRANT USAGE ON SCHEMA public TO {role}; GRANT SELECT ON nodes,attempts,__toasty_migrations TO {role};')
    old_pairs = [pair(index,'old-'+str(index)) for index in range(2)]
    for index,(c,t) in enumerate(old_pairs):
        launch(frozen,'artifactory-control-plane',c,'old-c'+str(index),old)
        base = 'http://'+c['ARTIFACTORY_HTTP_BIND']
        wait(lambda:ready(base),'old Control readiness')
        request(base,'/api/v1/auth/login',{'username':'acceptance','password':c['ARTIFACTORY_BOOTSTRAP_PASSWORD']})
        launch(frozen,'artifactory-telemetry-service',t,'old-t'+str(index),old)
        wait(lambda:ready('https://'+t['ARTIFACTORY_TELEMETRY_HTTP_BIND'],True),'old Telemetry readiness')
    assert len(old)==4 and all(not child.finished() for child in old)
    old_ledgers = ledgers()
    administrator = sql(business,'SELECT json_agg(to_jsonb(u) ORDER BY id) FROM users u')
    # 先停止旧接收端，阻止其清扫、查询和删除跨过迁移边界。
    for child in sorted(old,key=lambda child:0 if child.label.startswith('old-t') else 1):
        stop(child)
    require_stopped(old,applications,sql,(business,telemetry))
    assert ledgers()==old_ledgers
    old_manifest = frozen_source.verify_frozen(directory)
    proof['phase'] = 'cutover-migrate'
    run([binaries['artifactory-control-plane'],'migrate'],control_environment,45)
    grant = (legacy.ROOT/'deploy/telemetry-business-readonly.sql').read_text()
    for source,value in ((":\"business_database\"",business),(":\"business_schema\"",'public'),(":\"telemetry_business_role\"",role)):
        grant = grant.replace(source,'"'+value+'"')
    sql(business,grant)
    for table in ('nodes','attempts','node_startups','__toasty_migrations'):
        sql(business,'SELECT count(*) FROM '+table+' WHERE false',restricted=True)
    for statement in ('UPDATE nodes SET revoked=true WHERE false','SELECT count(*) FROM users','CREATE TABLE forbidden_by_acl(id int)'):
        sql(business,statement,restricted=True,rejected=True)
    assert sql(business,'SHOW transaction_read_only',restricted=True)=='off'
    pairs = [pair(index,'current-'+str(index)) for index in range(2)]
    controllers,receivers = [],[]
    for index,(c,t) in enumerate(pairs):
        receivers.append(launch(binaries,'artifactory-telemetry-service',t,'current-t'+str(index),current))
        wait(lambda:ready('https://'+t['ARTIFACTORY_TELEMETRY_HTTP_BIND'],True),'current Telemetry readiness')
        controllers.append(launch(binaries,'artifactory-control-plane',c,'current-c'+str(index),current))
        wait(lambda:ready('http://'+c['ARTIFACTORY_HTTP_BIND']),'current Control readiness')
    assert all(not child.finished() for child in current)
    current_ledgers = ledgers()
    assert current_ledgers!=old_ledgers
    assert sql(business,'SELECT json_agg(to_jsonb(u) ORDER BY id) FROM users u')==administrator
    assert sql(telemetry,"SELECT count(*) FROM pg_constraint WHERE conname IN ('inventory_delivery_content','startup_inventory_content') AND convalidated")=='2'
    bases = ['http://'+c['ARTIFACTORY_HTTP_BIND'] for c,_ in pairs]
    api = lambda path,body=None:request(bases[0],path,body)
    other_api = lambda path,body=None:request(bases[1],path,body)
    enrollment = api('/api/v1/enrollment-tokens',{'pool_id':tenant})['token']
    cfg = directory/'emissary.json'
    run([binaries['artifactory-emissary'],'register','--config',cfg,'--root',directory/'node',
         '--endpoint','https://'+pairs[0][0]['ARTIFACTORY_GRPC_BIND'],'--ca',directory/'pki/ca.pem',
         '--name','isolated-safe-cutover'],dict(environment,ARTIFACTORY_ENROLLMENT_TOKEN=enrollment),45)
    config = json.loads(cfg.read_bytes())
    tools = refresh.prepare(directory,config)
    probe = driver.prepare_restart_probe(directory,config)
    private_json(cfg,config)
    node = str(uuid.UUID(config['emissary_id']))
    identity = legacy.digest(Path(config['certificate_file']))
    checkpoint = directory/'node/telemetry/inventories/startup.json'
    proof['phase'] = 'cutover-current'
    runner = supervisor.start([binaries['artifactory-emissary'],'run','--config',cfg],
        cwd=legacy.ROOT,env=environment,log=directory/'cutover-runner.log',label='cutover-runner')
    def registered():
        raw = sql(business,f"SELECT json_build_object('startup',startup_id,'owner',connection_owner,'session',active_session) FROM nodes WHERE id='{node}' AND connection_owner IS NOT NULL")
        return json.loads(raw) if raw else None
    registered_node = wait(registered,'current Runner registration')
    startup = str(uuid.UUID(registered_node['startup']))
    wait(lambda:driver.startup_acknowledged(checkpoint,startup),'startup original ACK',65)
    wait(lambda:sql(telemetry,f"SELECT EXISTS(SELECT 1 FROM startup_hosts WHERE node_id='{node}' AND startup_id='{startup}')")=='t','current host transaction',45)
    for base in bases:
        latest = request(base,'/api/v1/nodes/'+node+'/telemetry')
        assert latest['host']['startup_id']==startup and latest['inventory']['startup_id']==startup
        assert latest['inventory']['provenance']=='native'
    snapshot_query = f"SELECT json_build_object('metadata',metadata,'digest',metadata_digest,'received',metadata_received_at,'capabilities',capabilities) FROM nodes WHERE id='{node}'"
    startup_query = f"SELECT to_jsonb(s) FROM node_startups s WHERE node_id='{node}' AND startup_id='{startup}'"
    snapshot,startup_row = sql(business,snapshot_query),sql(business,startup_query)
    purge_proof = purge_each_receiver(request,pairs,node,sql,telemetry)
    # 两个接收端均退出，真实刷新只得到操作ACK，原观察必须留在设备。
    for child in receivers:
        stop(child)
    receiver_apps = [name for name in applications if '_current_t' in name]
    require_stopped(receivers,receiver_apps,sql,(business,telemetry))
    refresh.write_version(tools,'9.8.7')
    body = {'request_id':str(uuid.uuid4()),'kind':'refresh_inventory','schema':1,'input':{'mode':'full'}}
    operation = other_api('/api/v1/nodes/'+node+'/operations',body)
    assert api('/api/v1/nodes/'+node+'/operations',body)['id']==operation['id']
    def complete_refresh():
        value = api('/api/v1/node-operations/'+operation['id'])
        return value if value['delivery_state']=='confirmed' and value['state']=='succeeded' else None
    result = wait(complete_refresh,'non-owner full refresh',35)
    wait(lambda:driver.operation_acknowledged(directory/'node/operations/journal.json',result),'refresh result ACK')
    assert registered()['owner']==registered_node['owner']
    assert result['result']['inventory']['delivery']=='pending'
    observation = str(uuid.UUID(result['result']['inventory']['observation_id']))
    source = refresh.decode_observation(directory/'node/telemetry/inventories'/(observation+'.inventory'),environment)
    assert source['startup']==startup and source['revision']==1 and source['source_batch_id']==''
    assert sql(telemetry,f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND observation_id='{observation}'")=='0'
    assert source['path'].read_bytes()==source['bytes']
    refresh_counts = {name:(tools/(name+'.count')).read_bytes() for name in refresh.probe_ids() if name!='git'}
    assert api('/api/v1/nodes/'+node+'/operations',body)['id']==operation['id']
    stop(controllers[1])
    proof['phase'] = 'cutover-compatible-recovery'
    def restart():
        require_stopped(current,applications,sql,(business,telemetry))
        assert source['path'].read_bytes()==source['bytes']
        assert sql(telemetry,f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND observation_id='{observation}'")=='0'
        assert ledgers()==current_ledgers
        assert {name:legacy.digest(path) for name,path in recovery.items()}==hashes
        recovered = []
        launch(recovery,'artifactory-telemetry-service',pairs[0][1],'recovery-t',recovered)
        wait(lambda:ready('https://'+pairs[0][1]['ARTIFACTORY_TELEMETRY_HTTP_BIND'],True),'recovery Telemetry readiness')
        return launch(recovery,'artifactory-control-plane',pairs[0][0],'recovery-c',recovered)
    _,operation_proof = driver.exercise_control_restart(controllers[0],directory=directory,
        node=node,startup=startup,probe=probe,api=api,sql=lambda query:sql(business,query),wait=wait,restart=restart)
    query = f"SELECT json_build_object('digest',d.delivery_digest,'time',(extract(epoch FROM d.observed_at)*1000)::bigint,'inventory',v.inventory) FROM inventory_deliveries d JOIN inventory_versions v ON v.node_id=d.node_id AND v.digest=d.inventory_digest WHERE d.node_id='{node}' AND d.observation_id='{observation}' AND d.startup_id='{startup}' AND d.revision=1 AND d.source_batch_id IS NULL"
    received = json.loads(wait(lambda:sql(telemetry,query),'recovery original inventory transaction',65))
    assert received=={'digest':source['digest'],'time':source['observed_at_ms'],'inventory':source['inventory']}
    wait(lambda:not source['path'].exists(),'recovery original inventory ACK',20)
    latest = api('/api/v1/nodes/'+node+'/telemetry')['inventory']
    assert latest['observation_id']==observation and latest['startup_id']==startup and latest['revision']==1
    duplicate = api('/api/v1/nodes/'+node+'/operations',body)
    for key in ('id','result','result_digest','sequence','issued_at','expires_at','finished_at'):
        assert duplicate[key]==result[key]
    assert {name:(tools/(name+'.count')).read_bytes() for name in refresh_counts}==refresh_counts
    assert json.loads((source['path'].parent/'revision.json').read_bytes())=={'startup_id':startup,'last':1}
    assert sql(telemetry,f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND startup_id='{startup}' AND revision>0")=='1'
    audit = json.loads(sql(business,f"SELECT json_object_agg(action,total) FROM (SELECT action,count(*) AS total FROM audit_events WHERE resource_id='{operation['id']}' GROUP BY action) counts"))
    assert audit=={'node_operation_created':1,'node_operation_finished':1}
    assert sql(business,snapshot_query)==snapshot and sql(business,startup_query)==startup_row
    assert ledgers()==current_ledgers and not runner.finished()
    assert legacy.digest(Path(config['certificate_file']))==identity and json.loads(cfg.read_bytes())==config
    assert frozen_source.verify_frozen(directory)==old_manifest
    assert legacy.source_manifest()==proof['source_manifest_sha256']
    for path,expected in artifacts.items():
        verify_artifact(Path(path),expected)
    proof.update(safe_cutover={'old_full_processes':4,'current_full_processes':4,
        'all_old_stopped_before_current_migration':True,'current_pairs_ready_concurrently':True,
        'both_current_pairs_read_native_startup':True,'per_receiver_purge':purge_proof,
        'cross_owner_refresh_and_duplicate':True,'pending_original_inventory_recovered':True,
        'original_observation':observation,'original_source_sha256':hashlib.sha256(source['bytes']).hexdigest(),
        'same_source_compatible_recovery':True,'historical_feature_rollback':False,
        'continuous_rolling_or_random_mix':False,'same_runner_pid':runner.process.pid,
        'same_runner_startup_config_and_certificate':True,'migration_ledgers_unchanged_on_recovery':True,
        'recovery_binary_sha256':hashes,'all_artifact_sha256':artifacts,'processes':processes,
        'frozen_source_manifest_sha256':old_manifest,'refresh_replay_no_new_probe_or_revision':True,
        'old_ledger_sha256':hashlib.sha256(json.dumps(old_ledgers,sort_keys=True).encode()).hexdigest(),
        'current_ledger_sha256':hashlib.sha256(json.dumps(current_ledgers,sort_keys=True).encode()).hexdigest(),
        'pending_operation':operation_proof,'both_telemetry_ingested_real_runner':False},
        real_control_main=True,real_telemetry_main=True,real_enrollment=True,
        native_service_executed=False,work_passed=True)


def purge_each_receiver(request,pairs,node,sql,database):
    # 复制实际行到明确标识的牺牲节点，只证明端点事务与删除路由。
    tables = ('telemetry_batches','host_samples','latest_hosts','startup_hosts',
              'inventory_observations','latest_inventories','startup_resources',
              'inventory_versions','inventory_deliveries','startup_inventories','inventory_ingress')
    sentinels = [str(uuid.uuid4()) for _ in pairs]
    def counts(identity):
        return {table:int(sql(database,f"SELECT count(*) FROM {table} WHERE node_id='{identity}'")) for table in tables}
    immutable = ('telemetry_batches','host_samples','inventory_versions','inventory_deliveries')
    snapshot = {}
    statements = ['BEGIN ISOLATION LEVEL REPEATABLE READ;']
    statements.append('SELECT json_build_object('+','.join("'"+table+"',(SELECT coalesce(json_agg(to_jsonb(t)), '[]') FROM "+table+" t WHERE node_id='"+node+"')" for table in immutable)+');')
    for identity in sentinels:
        for table in tables:
            statements.append(f"INSERT INTO {table} SELECT (jsonb_populate_record(NULL::{table},to_jsonb(t)||jsonb_build_object('node_id','{identity}'))).* FROM {table} t WHERE node_id='{node}';")
        statements.extend([
            f"INSERT INTO inventory_observations SELECT node_id,observed_at,inventory_digest FROM startup_inventories WHERE node_id='{identity}' ON CONFLICT DO NOTHING;",
            f"INSERT INTO latest_inventories SELECT node_id,observed_at,inventory_digest FROM startup_inventories WHERE node_id='{identity}' ON CONFLICT DO NOTHING;",
            f"INSERT INTO startup_resources SELECT node_id,startup_id,sampled_at,received_at,sample FROM startup_hosts WHERE node_id='{identity}' ON CONFLICT DO NOTHING;"])
    statements.append('COMMIT;')
    snapshot = json.loads(sql(database,'\n'.join(statements)))
    expected = {identity:counts(identity) for identity in sentinels}
    assert all(all(value>0 for value in values.values()) for values in expected.values()), 'purge fixture cannot be empty'
    def original_rows_preserved():
        for table,rows in snapshot.items():
            current = json.loads(sql(database,f"SELECT coalesce(json_agg(to_jsonb(t)),'[]') FROM {table} t WHERE node_id='{node}'"))
            assert all(row in current for row in rows), 'purge removed original immutable row'
    for index,(_,t) in enumerate(pairs):
        identity = sentinels[index]
        result = request('https://'+t['ARTIFACTORY_TELEMETRY_HTTP_BIND'],'/v1/purge',{'nodes':[identity],'attempts':[]},True)
        assert result=={'ok':True} and all(value==0 for value in counts(identity).values())
        assert sql(database,f"SELECT count(*) FROM telemetry_deleted_identities WHERE kind='node' AND id='{identity}'")=='1'
        original_rows_preserved()
        if index==0:
            assert counts(sentinels[1])==expected[sentinels[1]]
    return {'receivers':2,'actual_new_table_rows_removed':True,'other_node_preserved':True,
            'legacy_inventory_and_startup_resources':True,
            'sentinel_source':'repeatable-read SQL copies and explicit legacy projections; not device lifecycle proof',
            'excluded':'build/process samples and continuous aggregates'}
