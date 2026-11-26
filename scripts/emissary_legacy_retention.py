#!/usr/bin/env python3
"""[INPUT]: 冻结旧完整main、当前注册门禁与本轮双库资源。
[OUTPUT]: 监督旧清扫、SQLSTATE外键拒收和迁移后清单正文读回。
[POS]: 普通进程混部故障夹具；不修改冻结源码或迁移历史。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import tarfile
import time
import urllib.error
import urllib.request
import urllib.parse

import check_emissary_legacy_receiver as legacy
from check_emissary_legacy_sender import private_json


def compile_frozen(supervisor, directory, environment, timeout):
    source = directory/'frozen'
    source.mkdir()
    archive = directory/'frozen.tar'
    child = supervisor.start(['git','archive','--output',archive,legacy.BASELINE],
                             cwd=legacy.ROOT,env=environment,log=directory/'frozen-export.log',label='frozen-export')
    if child.wait(30):
        raise RuntimeError('frozen source export failed')
    with tarfile.open(archive) as stream:
        manifest = {member.name:hashlib.sha256(stream.extractfile(member).read()).hexdigest()
                    for member in stream.getmembers() if member.isfile()}
        stream.extractall(source,filter='data')
    archive.unlink()
    private_json(directory/'frozen-manifest.json',manifest)
    names = ('artifactory-control-plane','artifactory-telemetry-service')
    args = ['cargo','build','--locked','--bins','--message-format=json']
    for name in names:
        args.extend(['-p',name])
    env = dict(environment,CARGO_TARGET_DIR=os.environ.get('ARTIFACTORY_TEST_FROZEN_TARGET_DIR',str(directory/'frozen-target')))
    log = directory/'frozen-build.log'
    child = supervisor.start(args,cwd=source,env=env,log=log,label='frozen-build')
    if child.wait(timeout):
        raise RuntimeError('frozen binaries build failed; inspect private log')
    binaries = {}
    for line in log.read_bytes().splitlines():
        try:
            value = json.loads(line)
        except (ValueError,UnicodeDecodeError):
            continue
        name = value.get('target',{}).get('name')
        if value.get('reason')=='compiler-artifact' and name in names and value.get('executable'):
            if name in binaries:
                raise RuntimeError('duplicate frozen binary')
            binaries[name] = Path(value['executable'])
    if set(binaries)!=set(names):
        raise RuntimeError('frozen binaries missing')
    return binaries


def verify_frozen(directory):
    manifest = json.loads((directory/'frozen-manifest.json').read_bytes())
    if not manifest or any(legacy.digest(directory/'frozen'/name)!=digest for name,digest in manifest.items()):
        raise RuntimeError('frozen product source changed')
    return hashlib.sha256(json.dumps(manifest,sort_keys=True).encode()).hexdigest()


def retention_foreign_key_guard(sql,database,node,digest):
    node = str(__import__('uuid').UUID(str(node)))
    if len(digest)!=64 or any(value not in '0123456789abcdef' for value in digest):
        raise RuntimeError('invalid retained inventory digest')
    statement = f"DELETE FROM inventory_versions WHERE node_id='{node}' AND digest='{digest}'"
    result = sql(database,"""CREATE FUNCTION pg_temp.emissary_retention_guard() RETURNS text
    LANGUAGE plpgsql AS $guard$
    DECLARE rejected_constraint text;
    BEGIN
      BEGIN
        """+statement+""";
        RAISE EXCEPTION 'retention guard unexpectedly accepted deletion';
      EXCEPTION WHEN foreign_key_violation THEN
        GET STACKED DIAGNOSTICS rejected_constraint = CONSTRAINT_NAME;
        RETURN SQLSTATE || ':' || rejected_constraint;
      END;
    END;
    $guard$;
    SELECT pg_temp.emissary_retention_guard();""")
    if result not in ('23503:inventory_delivery_content','23503:startup_inventory_content'):
        raise RuntimeError('retention guard did not reject the referenced inventory body')
    return result


class Retention:
    def __init__(self, *, supervisor, directory, environment, pg_environment, binaries,
                 control_environment, telemetry_environment, business, telemetry, role, nonce, sql, address, wait):
        self.supervisor,self.directory,self.environment = supervisor,directory,environment
        self.sql,self.wait,self.telemetry,self.nonce = sql,wait,telemetry,nonce
        self.binaries = binaries
        self.old_env = dict(telemetry_environment,
                            ARTIFACTORY_TELEMETRY_GRPC_BIND=address(),
                            ARTIFACTORY_TELEMETRY_HTTP_BIND=address(),
                            PGAPPNAME='legacy_retention_'+nonce)
        parsed = urllib.parse.urlsplit(self.old_env['DATABASE_URL'])
        pairs = urllib.parse.parse_qsl(parsed.query)
        pairs.append(('application_name','legacy_retention_'+nonce))
        self.old_env['DATABASE_URL'] = urllib.parse.urlunsplit(parsed._replace(query=urllib.parse.urlencode(pairs)))
        self.token = telemetry_environment['ARTIFACTORY_TELEMETRY_TOKEN']
        self.context = __import__('ssl').create_default_context(cafile=directory/'pki/ca.pem')
        migrate = self.start(binaries['artifactory-control-plane'],['migrate'],control_environment,'frozen-business-migrate')
        if migrate.wait(45):
            raise RuntimeError('frozen business migration failed')
        sql(business,f'GRANT CONNECT ON DATABASE {business} TO {role}; GRANT USAGE ON SCHEMA public TO {role}; GRANT SELECT ON nodes,attempts,__toasty_migrations TO {role};')
        initial = self.start(binaries['artifactory-telemetry-service'],[],self.old_env,'frozen-initial')
        wait(self.ready,'frozen main initial startup')
        self.stop(initial)
        assert sql(telemetry,'SELECT count(*) FROM __toasty_migrations')=='3'
        self.sentinel_node,self.sentinel_batch = __import__('uuid').uuid4(),__import__('uuid').uuid4()
        sql(telemetry,f"INSERT INTO telemetry_batches(node_id,batch_id,digest,received_at) VALUES('{self.sentinel_node}','{self.sentinel_batch}','{'a'*64}',now()-interval '4 days')")
        ready = directory/'retention-fence-ready'
        script = directory/'retention-fence.sql'
        script.write_text(f"BEGIN;\nLOCK TABLE inventory_observations IN SHARE MODE;\n\\o {ready}\nSELECT 'locked';\n\\o\nSELECT pg_sleep(180);\nROLLBACK;\n")
        self.lock_name = 'retention_fence_'+nonce
        self.holder = self.start('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-v','VERBOSITY=sqlstate','-d',telemetry,'-f',script],
                                dict(pg_environment,PGAPPNAME=self.lock_name,PGOPTIONS='-c statement_timeout=185000'),'retention-fence')
        wait(lambda:ready.exists(),'old retention fence acquired')
        self.fence_pid = int(wait(lambda:sql(telemetry,f"SELECT pid FROM pg_stat_activity WHERE application_name='{self.lock_name}' AND datname=current_database()"),'old retention fence backend'))
        self.old = self.start(binaries['artifactory-telemetry-service'],[],self.old_env,'frozen-retention')
        wait(self.ready,'frozen main fenced startup')
        self.blocked = wait(self.blocked_query,'actual frozen retention blocked')

    def start(self,binary,args,environment,label):
        return self.supervisor.start([binary,*args],cwd=legacy.ROOT,env=environment,
                                     log=self.directory/(label+'.log'),label=label)

    @staticmethod
    def stop(child):
        __import__('os').killpg(child.process.pid,signal.SIGINT)
        if child.wait(15):
            raise RuntimeError('frozen main graceful stop failed')

    def ready(self):
        request = urllib.request.Request('https://'+self.old_env['ARTIFACTORY_TELEMETRY_HTTP_BIND']+'/metrics',
                                         headers={'Authorization':'Bearer '+self.token})
        try:
            with urllib.request.build_opener(urllib.request.ProxyHandler({}),urllib.request.HTTPSHandler(context=self.context)).open(request,timeout=2) as reply:
                return reply.status==200
        except (OSError,urllib.error.URLError):
            return False

    def blocked_query(self):
        if self.old.finished():
            raise RuntimeError('old main exited before retention probe')
        raw = self.sql(self.telemetry,f"SELECT json_build_object('pid',pid,'query',query) FROM pg_stat_activity WHERE datname=current_database() AND application_name='legacy_retention_{self.nonce}' AND wait_event_type='Lock' AND {self.fence_pid}=ANY(pg_blocking_pids(pid)) AND query LIKE 'DELETE FROM inventory_observations%'")
        return json.loads(raw) if raw else None

    def release_and_observe(self,node,startup):
        assert self.blocked_query()
        original = json.loads(self.sql(self.telemetry,f"SELECT json_build_object('observation',i.observation_id,'digest',i.inventory_digest,'inventory',v.inventory) FROM startup_inventories i JOIN inventory_versions v ON v.node_id=i.node_id AND v.digest=i.inventory_digest WHERE i.node_id='{node}' AND i.startup_id='{startup}'"))
        assert self.sql(self.telemetry,f"SELECT count(*) FROM inventory_observations WHERE node_id='{node}' AND digest='{original['digest']}'")=='0'
        assert self.sql(self.telemetry,f"SELECT count(*) FROM latest_inventories WHERE node_id='{node}' AND digest='{original['digest']}'")=='0'
        # 真实Runner继续上报会重建legacy投影；独立引用夹具固定旧清扫候选。
        guard_observation,guard_startup,guard_batch = (__import__('uuid').uuid4() for _ in range(3))
        guard_body = json.dumps({'retention_fixture':self.nonce},sort_keys=True,separators=(',',':'))
        guard_digest = hashlib.sha256(guard_body.encode()).hexdigest()
        self.sql(self.telemetry,f"INSERT INTO inventory_versions(node_id,digest,first_seen,inventory) VALUES('{self.sentinel_node}','{guard_digest}',now(),'{guard_body}'::jsonb); INSERT INTO inventory_deliveries(node_id,observation_id,startup_id,revision,source_batch_id,observed_at,received_at,delivery_digest,inventory_digest) VALUES('{self.sentinel_node}','{guard_observation}','{guard_startup}',0,'{guard_batch}',now(),now(),'{guard_digest}','{guard_digest}');")
        assert self.sql(self.telemetry,f"SELECT pg_cancel_backend(pid) FROM pg_stat_activity WHERE pid={self.fence_pid} AND application_name='{self.lock_name}' AND datname=current_database()")=='t'
        assert self.holder.wait(10)!=0
        assert '57014' in (self.directory/'retention-fence.log').read_text()
        self.wait(lambda:self.sql(self.telemetry,f"SELECT count(*) FROM pg_stat_activity WHERE pid={self.blocked['pid']} AND state<>'idle'")=='0','actual old retention finished',20)
        assert not self.old.finished() and self.ready()
        body_count = int(self.sql(self.telemetry,f"SELECT count(*) FROM inventory_versions WHERE node_id='{node}' AND digest='{original['digest']}'"))
        receipt_count = int(self.sql(self.telemetry,f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND observation_id='{original['observation']}'"))
        proof = {'frozen_main_remained_live':True,'real_old_retention_wait_observed':True,
                 'body_count_after_old_retention':body_count,'receipt_count_after_old_retention':receipt_count,
                 'deterministic_reference_fixture':True,
                 'old_binary_sha256':legacy.digest(self.binaries['artifactory-telemetry-service']),
                 'frozen_manifest_sha256':verify_frozen(self.directory)}
        private_json(self.directory/'legacy-retention-proof.json',proof)
        if body_count!=1 or receipt_count!=1:
            raise RuntimeError('frozen retention deleted independently referenced inventory body')
        retained = json.loads(self.sql(self.telemetry,f"SELECT json_build_object('observation',i.observation_id,'digest',i.inventory_digest,'inventory',v.inventory) FROM startup_inventories i JOIN inventory_versions v ON v.node_id=i.node_id AND v.digest=i.inventory_digest WHERE i.node_id='{node}' AND i.startup_id='{startup}'"))
        assert retained==original, 'old retention changed original inventory content or provenance'
        proof['original_inventory_and_provenance_unchanged'] = True
        self.wait(lambda:'telemetry retention maintenance failed' in
                  (self.directory/'frozen-retention.log').read_text(),
                  'actual old retention rejected',5)
        # 驱动Display不保证展开原因；后续legacy投影也会改变原清扫谓词。
        # 对刚保留的同一正文执行删除，以SQLSTATE核验实际外键。
        rejection = retention_foreign_key_guard(self.sql,self.telemetry,node,original['digest'])
        proof['old_retention_guard_sqlstate'] = rejection
        assert 'telemetry retention maintenance failed' in (self.directory/'frozen-retention.log').read_text()
        assert self.sql(self.telemetry,f"SELECT count(*) FROM telemetry_batches WHERE node_id='{self.sentinel_node}' AND batch_id='{self.sentinel_batch}'")=='1'
        proof['old_retention_restricted_and_rolled_back'] = True
        # 旧删除也必须失败并回滚，不能返回成功却留下新回执。
        tables = ('telemetry_batches','host_samples','latest_hosts','startup_hosts','inventory_versions',
                  'inventory_deliveries','startup_inventories','inventory_ingress')
        before = {table:self.sql(self.telemetry,f"SELECT count(*) FROM {table} WHERE node_id='{node}'") for table in tables}
        request = urllib.request.Request('https://'+self.old_env['ARTIFACTORY_TELEMETRY_HTTP_BIND']+'/v1/purge',
            data=json.dumps({'nodes':[node],'attempts':[]}).encode(),
            headers={'Authorization':'Bearer '+self.token,'Content-Type':'application/json'})
        try:
            with urllib.request.build_opener(urllib.request.ProxyHandler({}),urllib.request.HTTPSHandler(context=self.context)).open(request,timeout=5):
                raise RuntimeError('old purge unexpectedly accepted new references')
        except urllib.error.HTTPError as error:
            assert error.code==500
        assert {table:self.sql(self.telemetry,f"SELECT count(*) FROM {table} WHERE node_id='{node}'") for table in tables}==before
        assert self.sql(self.telemetry,f"SELECT count(*) FROM telemetry_deleted_identities WHERE kind='node' AND id='{node}'")=='0'
        proof['old_https_purge_failed_without_partial_delete'] = True
        private_json(self.directory/'legacy-retention-proof.json',proof)
        return proof

    def close(self):
        self.stop(self.old)
