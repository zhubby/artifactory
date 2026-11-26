#!/usr/bin/env python3
"""[INPUT]: 当前三类二进制、显式回环Timescale和认证S3容器。
[OUTPUT]: 验证注册、操作ACK、停旧切换与可选真实网关OTA。
[POS]: 普通进程组合门禁；不安装系统服务，不替代原生或历史版本验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import base64
import hashlib
import hmac
import http.cookiejar
import json
import os
from pathlib import Path
import re
import signal
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import check_emissary_legacy_receiver as legacy
from check_emissary_legacy_sender import Supervisor, private_json


def runtime_environment():
    allowed = ('PATH', 'HOME', 'USER', 'LOGNAME', 'TMPDIR', 'LANG', 'LC_ALL',
               'SYSTEMROOT', 'SSL_CERT_FILE', 'SSL_CERT_DIR')
    return {key: os.environ[key] for key in allowed if key in os.environ}


def compile_binaries(supervisor, directory, environment, timeout, with_ota=False):
    names = ('artifactory-control-plane', 'artifactory-telemetry-service', 'artifactory-emissary')
    packages = names + (('artifactory-gateway',) if with_ota else ())
    if with_ota:
        names += ('artifactory-gateway','emissary-release')
    arguments = ['cargo', 'build', '--locked', '--bins', '--message-format=json']
    for name in packages:
        arguments.extend(['-p', name])
    build_env = dict(environment)
    if with_ota:
        build_env['ARTIFACTORY_BUILD_COMMIT'] = subprocess.check_output(
            ['git','rev-parse','HEAD'],cwd=ROOT,env=environment,text=True,timeout=5).strip()
    for key in ('CARGO_HOME', 'RUSTUP_HOME', 'RUSTUP_TOOLCHAIN', 'CARGO_TARGET_DIR',
                'CARGO_BUILD_JOBS', 'RUSTFLAGS', 'CARGO_PROFILE_DEV_DEBUG'):
        if key in os.environ:
            build_env[key] = os.environ[key]
    log = directory / 'build.log'
    child = supervisor.start(arguments, cwd=ROOT, env=build_env, log=log, label='build-real-mains')
    if child.wait(timeout):
        raise RuntimeError('current binaries compilation failed; inspect private log')
    binaries = {}
    for line in log.read_bytes().splitlines():
        try:
            event = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        name = event.get('target', {}).get('name')
        if event.get('reason') == 'compiler-artifact' and event.get('executable') and name in names:
            if name in binaries:
                raise RuntimeError('build returned duplicate executable')
            binaries[name] = Path(event['executable'])
    if set(binaries) != set(names):
        raise RuntimeError('build did not identify all current executables')
    return binaries


def s3_host(details):
    if 'seaweed' not in details.get('Config', {}).get('Image', '').lower():
        raise RuntimeError('explicit S3 test container must use SeaweedFS')
    ports = details.get('NetworkSettings', {}).get('Ports', {}).get('8333/tcp') or []
    if len(ports) != 1 or ports[0]['HostIp'] != '127.0.0.1':
        raise RuntimeError('S3 test container must publish one IPv4 loopback port')
    port = int(ports[0]['HostPort'])
    if not 1 <= port <= 65535:
        raise RuntimeError('invalid loopback S3 port')
    return '127.0.0.1:' + str(port)


def cleanup_databases(sql, business, telemetry, role, nonce):
    for name in (telemetry, business):
        raw = sql('postgres', f"SELECT json_build_object('marker',shobj_description(oid,'pg_database')) FROM pg_database WHERE datname='{name}'")
        if raw:
            if json.loads(raw)['marker'] != nonce:
                raise RuntimeError('database ownership differs; cleanup_unconfirmed')
            sql('postgres', f'DROP DATABASE {name} WITH (FORCE);')
        if sql('postgres', f"SELECT count(*) FROM pg_database WHERE datname='{name}'") != '0':
            raise RuntimeError('database deletion readback differs')
    raw = sql('postgres', f"SELECT json_build_object('marker',shobj_description(oid,'pg_authid')) FROM pg_roles WHERE rolname='{role}'")
    if raw:
        if json.loads(raw)['marker'] != nonce:
            raise RuntimeError('role ownership differs; cleanup_unconfirmed')
        sql('postgres', f'DROP ROLE {role};')
    if sql('postgres', f"SELECT count(*) FROM pg_roles WHERE rolname='{role}'") != '0':
        raise RuntimeError('role deletion readback differs')


def hold_node(supervisor, directory, environment, database, node, application):
    node = str(uuid.UUID(node))
    if not re.fullmatch(r'emissary_startup_[0-9a-f]{32}', application):
        raise RuntimeError('invalid startup fence identity')
    ready = directory/'startup-fence-ready'
    script = directory/'startup-fence.sql'
    script.write_text(f"BEGIN;\nSELECT id FROM nodes WHERE id='{node}' FOR UPDATE;\n"
                      f"\\o {ready}\nSELECT 'locked';\n\\o\nSELECT pg_sleep(120);\nROLLBACK;\n")
    return supervisor.start(['psql','-X','-qAt','-v','ON_ERROR_STOP=1','-v','VERBOSITY=sqlstate',
                             '-d',database,'-f',script], cwd=ROOT,
                            env=dict(environment, PGAPPNAME=application,
                                     PGOPTIONS='-c statement_timeout=125000'),
                            log=directory/'startup-fence.log',label='startup-fence')


def inventory_source(directory, environment):
    checkpoint = directory/'startup.json'
    if not checkpoint.exists():
        return None
    saved = json.loads(checkpoint.read_bytes())
    if saved['acknowledged']:
        raise RuntimeError('startup source acknowledged before registration release')
    observation = str(uuid.UUID(saved['observation_id']))
    startup = str(uuid.UUID(saved['startup_id']))
    path = directory/(observation+'.inventory')
    body = path.read_bytes()
    decoded = subprocess.run(['protoc','--decode=artifactory.emissary.v1.InventoryObservation',
                              '-I',str(ROOT/'proto'), 'artifactory/emissary/v1/emissary.proto'],
                             input=body,env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                             timeout=5,check=True).stdout.decode()
    def string(name):
        match = re.search(r'^'+name+r': (".*")$',decoded,re.MULTILINE)
        if not match:
            raise RuntimeError('source observation missing '+name)
        return json.loads(match[1])
    if string('observation_id') != observation or string('source_startup_id') != startup:
        raise RuntimeError('source observation identity differs from checkpoint')
    if re.search(r'^(revision|monotonic_ms|source_batch_id):',decoded,re.MULTILINE):
        raise RuntimeError('expected native startup revision zero')
    if not re.search(r'^inventory_schema: 1$',decoded,re.MULTILINE):
        raise RuntimeError('expected startup inventory schema one')
    if not re.fullmatch('[0-9a-f]{64}',string('delivery_digest')):
        raise RuntimeError('invalid startup delivery digest')
    observed = re.search(r'^observed_at_ms: ([0-9]+)$',decoded,re.MULTILINE)
    if not observed:
        raise RuntimeError('source observed time missing')
    return {'path':path,'bytes':body,'observation':observation,'startup':startup,
            'delivery_digest':string('delivery_digest'),'observed_at_ms':int(observed[1])}


def startup_acknowledged(path, startup, source=None):
    if not path.exists():
        return False
    checkpoint = json.loads(path.read_bytes())
    if checkpoint['startup_id'] != startup or not checkpoint['acknowledged']:
        return False
    if source and (checkpoint['observation_id'] != source['observation'] or source['path'].exists()):
        return False
    return not (path.parent/(checkpoint['observation_id']+'.inventory')).exists()


def operation_acknowledged(path, expected):
    if not path.exists():
        return False
    journal = json.loads(path.read_bytes())
    entries = [entry for entry in journal['entries'] if entry['envelope']['operation_id']==expected['id']]
    if len(entries) != 1 or journal['result_expired'] != 0:
        return False
    entry = entries[0]
    envelope, update = entry['envelope'],entry['update']
    envelope_fields = ('tenant_id','node_id','startup_id','control_session','actor_id','kind','schema','input_digest')
    update_fields = ('startup_id','state','sequence','reason','result_digest')
    return bool(entry['acknowledged'] and entry['created_at'] and entry['finished_at']
                and update['result'] is None and update['operation_id']==expected['id']
                and expected['state'] in ('succeeded','failed') and expected['result_digest']
                and all(envelope[key]==expected[key] for key in envelope_fields)
                and all(update[key]==expected[key] for key in update_fields))


def prepare_restart_probe(directory, config):
    probe = directory/'probe-git'
    probe.write_text('#!/bin/sh\n'
                     'if [ -f "$0.arm" ]; then\n'
                     '  printf x >> "$0.count"\n'
                     '  : > "$0.ready"\n'
                     '  remaining=40\n'
                     '  while [ ! -f "$0.go" ]; do\n'
                     '    [ "$remaining" -gt 0 ] || exit 71\n'
                     '    remaining=$((remaining - 1))\n'
                     '    /bin/sleep .02\n'
                     '  done\n'
                     'fi\n'
                     "printf 'git version 2.40.1\\n'\n")
    probe.chmod(0o700)
    config['tools']['git'] = str(probe)
    return probe


def exercise_control_restart(controller, *, directory, node, startup, probe, api, sql, wait, restart):
    journal_path = directory/'node/operations/journal.json'
    body = {'request_id':str(uuid.uuid4()),'kind':'collect_diagnostics','schema':1,'input':{'probes':['git']}}
    before_node = json.loads(sql(f"SELECT json_build_object('session',active_session,'owner',connection_owner,'digest',metadata_digest,'startup',startup_id) FROM nodes WHERE id='{node}'"))
    Path(str(probe)+'.arm').touch()
    operation = api('/api/v1/nodes/'+node+'/operations',body)
    operation_id = str(uuid.UUID(operation['id']))
    wait(lambda:Path(str(probe)+'.ready').exists(),'restart diagnostic probe entered',12)
    failed_at = time.monotonic()
    original_pid = controller.process.pid
    os.killpg(controller.process.pid,signal.SIGKILL)
    # 先释放有界探测；进程组回收会扫描宿主进程表，不占探测预算。
    Path(str(probe)+'.go').touch()
    assert time.monotonic()-failed_at < 1, 'Control failure exceeded probe release budget'
    assert controller.wait(10) == -signal.SIGKILL
    def offline_result():
        journal = json.loads(journal_path.read_bytes())
        entries = [entry for entry in journal['entries'] if entry['envelope']['operation_id']==operation_id]
        if len(entries)!=1 or entries[0]['update']['state']!='succeeded':
            return None
        entry = entries[0]
        assert journal['result_expired']==0 and not entry['acknowledged'] and entry['update']['result']
        assert entry['envelope']['node_id']==node and entry['envelope']['startup_id']==startup
        assert entry['envelope']['input_digest']==operation['input_digest']
        assert entry['update']['result']['probes']['git']['version']=='2.40.1'
        return journal
    offline = wait(offline_result,'durable offline operation result',5)
    private_json(directory/'before-restart-journal.json',offline)
    saved = next(entry for entry in offline['entries'] if entry['envelope']['operation_id']==operation_id)
    assert Path(str(probe)+'.count').read_bytes()==b'x'
    row = json.loads(sql(f"SELECT to_jsonb(o) FROM node_operations o WHERE id='{operation_id}'"))
    assert row['delivery_state']!='confirmed' and row['finished_at'] is None
    controller = restart()
    def reconnected():
        current = json.loads(sql(f"SELECT json_build_object('session',active_session,'owner',connection_owner,'digest',metadata_digest,'startup',startup_id) FROM nodes WHERE id='{node}'"))
        if not current['session'] or not current['owner'] or current['session']==before_node['session'] or current['owner']==before_node['owner']:
            return None
        assert current['startup']==startup==before_node['startup'] and current['digest']==before_node['digest']
        return current
    after_node = wait(reconnected,'real Runner reconnected to restarted Control',45)
    def confirmed():
        current = api('/api/v1/node-operations/'+operation_id)
        return current if current['delivery_state']=='confirmed' and current['state']=='succeeded' else None
    result = wait(confirmed,'offline operation result replay',35)
    for key in ('id','request_id','node_id','startup_id','input_digest','issued_at','expires_at'):
        assert result[key]==operation[key]
    assert result['control_session']==after_node['session']
    assert datetime.fromisoformat(result['execution_cutoff']) <= datetime.fromisoformat(row['execution_cutoff'])
    assert result['result_digest']==saved['update']['result_digest']
    assert result['result']==saved['update']['result'] and result['sequence']==saved['update']['sequence']
    wait(lambda:operation_acknowledged(journal_path,result),'replayed operation device ACK',10)
    assert api('/api/v1/nodes/'+node+'/operations',body)['id']==operation_id
    assert Path(str(probe)+'.count').read_bytes()==b'x'
    Path(str(probe)+'.arm').unlink()
    audit = json.loads(sql(f"SELECT json_object_agg(action,total) FROM (SELECT action,count(*) AS total FROM audit_events WHERE resource_id='{operation_id}' GROUP BY action) counts"))
    assert audit=={'node_operation_created':1,'node_operation_finished':1}
    private_json(directory/'after-restart-result.json',result)
    private_json(directory/'after-restart-ack-journal.json',json.loads(journal_path.read_bytes()))
    return controller, {'ordinary_control_sigkill':True,'same_runner_and_startup':True,
                        'original_pid':original_pid,'original_exit_code':-signal.SIGKILL,
                        'restarted_pid':controller.process.pid,
                        'new_control_owner_and_session':True,'offline_result_unacknowledged':True,
                        'same_result_replayed_and_acked':True,'receive_deadline_unchanged':True,
                        'execution_cutoff_not_extended':True,
                        'probe_calls':1,'duplicate_request_same_id':True,'single_create_and_finish_audit':True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--database-container', required=True)
    parser.add_argument('--s3-container', required=True)
    parser.add_argument('--evidence', type=Path, default=Path('/tmp/emissary-real-control-main-evidence.json'))
    parser.add_argument('--build-timeout', type=int, default=900)
    parser.add_argument('--with-control-restart', action='store_true')
    parser.add_argument('--with-legacy-retention', action='store_true')
    parser.add_argument('--with-controlled-refresh', action='store_true')
    parser.add_argument('--with-runtime-ota', action='store_true')
    parser.add_argument('--with-ota-renewal', action='store_true')
    parser.add_argument('--with-safe-cutover', action='store_true')
    args = parser.parse_args()
    if args.with_safe_cutover and any((args.with_control_restart,args.with_legacy_retention,args.with_controlled_refresh,args.with_runtime_ota,args.with_ota_renewal)):
        parser.error('--with-safe-cutover is a separate integration scenario')
    if args.with_ota_renewal and not args.with_runtime_ota:
        parser.error('--with-ota-renewal requires --with-runtime-ota')
    if args.with_control_restart and args.with_controlled_refresh:
        raise RuntimeError('controlled refresh and restart use distinct tool fixtures; run separately')
    if sys.flags.optimize:
        raise RuntimeError('acceptance assertions require Python without optimization')
    if os.name != 'posix' or not hasattr(os, 'WNOWAIT'):
        raise RuntimeError('ordinary Control main driver requires POSIX process ownership')
    os.umask(0o077)
    directory = Path(tempfile.mkdtemp(prefix='emissary-real-control-', dir='/tmp'))
    print('Private diagnostic directory:', directory, flush=True)
    supervisor = Supervisor()
    nonce = uuid.uuid4().hex
    args.evidence.parent.mkdir(parents=True, exist_ok=True)
    proof = {'status':'failed', 'phase':'prepare', 'nonce':nonce,
             'boundary':'ordinary real Control main, Telemetry main and enrolled Runner; no native service or deployment',
             'source_manifest_sha256':legacy.source_manifest()}
    # 先替换同名旧证据，任何早期失败都不能留下旧passed。
    private_json(args.evidence, proof)
    business, telemetry, role = ('emissary_business_' + nonce, 'emissary_telemetry_' + nonce, 'emissary_auth_' + nonce)
    bucket = 'emissary-acceptance-' + nonce
    private_json(directory/'resources.json', {'nonce':nonce, 'business':business,
                 'telemetry':telemetry, 'role':role, 'bucket':bucket})
    password = uuid.uuid4().hex
    database = urllib.parse.urlsplit(legacy.database_url(args.database_container))
    base_env = runtime_environment()
    base_env.update(NO_PROXY='127.0.0.1,localhost,::1', no_proxy='127.0.0.1,localhost,::1')
    pg_env = dict(base_env, PGHOST=database.hostname, PGPORT=str(database.port),
                  PGUSER=urllib.parse.unquote(database.username),
                  PGPASSWORD=urllib.parse.unquote(database.password or ''), PGCONNECT_TIMEOUT='3', PGOPTIONS='-c statement_timeout=15000')
    def db_url(name, restricted=False):
        user = role if restricted else urllib.parse.unquote(database.username)
        secret = password if restricted else urllib.parse.unquote(database.password or '')
        return 'postgresql://' + urllib.parse.quote(user, safe='') + ':' + urllib.parse.quote(secret, safe='') + '@127.0.0.1:' + str(database.port) + '/' + name
    def sql(name, statement, restricted=False, rejected=False):
        environment = dict(pg_env, PGUSER=role, PGPASSWORD=password) if restricted else pg_env
        result = subprocess.run(['psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-d', name],
                                input=statement, env=environment, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20)
        if rejected:
            if result.returncode == 0 or 'permission denied' not in result.stderr.lower():
                raise RuntimeError('restricted business ACL did not reject access')
            return None
        if result.returncode:
            (directory / 'sql-error.log').write_text(result.stderr)
            raise RuntimeError('database command failed; inspect private log')
        return result.stdout.strip()
    counter = 0
    def run(args, env=None, timeout=30, rejected=False):
        nonlocal counter
        counter += 1
        child = supervisor.start(args, cwd=ROOT, env=env or base_env,
                                 log=directory / f'command-{counter}.log', label=f'command-{counter}')
        code = child.wait(timeout)
        if rejected:
            if code == 0:
                raise RuntimeError('reused enrollment token was accepted')
            log = (directory/f'command-{counter}.log').read_text()
            if ('The request does not have valid authentication credentials' not in log
                    or 'invalid or expired enrollment token' not in log
                    or 'device enrollment rejected' not in log):
                raise RuntimeError('token reuse failed for unexpected reason')
        elif code:
            raise RuntimeError('ordinary process command failed; inspect private log')
    def start(binary, args, env, label):
        return supervisor.start([binaries[binary], *args], cwd=ROOT, env=env,
                                log=directory/(label+'.log'), label=label)
    def stop(child):
        if child.finished():
            raise RuntimeError('ordinary service exited unexpectedly')
        if runtime_release and child.label in ('runner-0','runner-1'):
            from emissary_ota_processes import close_launcher
            if close_launcher(child):
                raise RuntimeError('ordinary launcher did not exit cleanly')
            return
        os.killpg(child.process.pid, signal.SIGINT)
        if child.wait(20):
            raise RuntimeError('ordinary service did not exit cleanly')
    def address():
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            return '127.0.0.1:' + str(listener.getsockname()[1])
    deadline = float('inf')
    def wait(check, label, timeout=45):
        end = min(deadline, time.monotonic() + timeout)
        while time.monotonic() < end:
            value = check()
            if value:
                return value
            time.sleep(.15)
        raise RuntimeError(label + ' deadline exceeded')
    details = json.loads(subprocess.check_output(['docker','inspect',args.s3_container], timeout=10))[0]
    s3_address = s3_host(details)
    config_arg = next(value.split('=',1)[1] for value in details['Config']['Cmd'] if value.startswith('-s3.config='))
    config = json.loads(subprocess.check_output(['docker','exec',args.s3_container,'cat',config_arg], timeout=10))
    identity = next(item for item in config['identities'] if 'Admin' in item['actions'])
    credentials = identity['credentials'][0]
    def s3(method, key=None):
        now = datetime.now(timezone.utc)
        stamp, date = now.strftime('%Y%m%dT%H%M%SZ'), now.strftime('%Y%m%d')
        payload = hashlib.sha256(b'').hexdigest()
        host, path = s3_address, '/' + bucket
        if key is not None:
            path += '/' + urllib.parse.quote(key,safe='/')
        canonical = method+'\n'+path+'\n\nhost:'+host+'\nx-amz-content-sha256:'+payload+'\nx-amz-date:'+stamp+'\n\nhost;x-amz-content-sha256;x-amz-date\n'+payload
        scope = date+'/us-east-1/s3/aws4_request'
        signing = ('AWS4'+credentials['secretKey']).encode()
        for value in (date,'us-east-1','s3','aws4_request'):
            signing = hmac.new(signing,value.encode(),hashlib.sha256).digest()
        signature = hmac.new(signing,('AWS4-HMAC-SHA256\n'+stamp+'\n'+scope+'\n'+hashlib.sha256(canonical.encode()).hexdigest()).encode(),hashlib.sha256).hexdigest()
        request = urllib.request.Request('http://'+host+path, method=method, headers={
            'x-amz-content-sha256':payload,'x-amz-date':stamp,
            'Authorization':'AWS4-HMAC-SHA256 Credential='+credentials['accessKey']+'/'+scope+', SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature='+signature})
        try:
            with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request,timeout=5) as response:
                return response.status
        except urllib.error.HTTPError as error:
            return error.code
    reserved = False
    bucket_created = False
    bucket_attempted = False
    ota_keys = []
    runtime_release = None
    started_at = time.monotonic()
    try:
        proof['phase'] = 'compile'
        binaries = compile_binaries(supervisor, directory, base_env, args.build_timeout,args.with_runtime_ota)
        runtime_release = None
        if args.with_runtime_ota:
            import emissary_control_ota
            runtime_release = emissary_control_ota.build_baseline(supervisor,directory,base_env,
                args.build_timeout,binaries['artifactory-emissary'])
            binaries['artifactory-emissary'] = runtime_release['candidate']
            ota_keys = emissary_control_ota.prepare(runtime_release,directory=directory,
                binaries=binaries,supervisor=supervisor,environment=base_env,run=run,wait=wait)
        frozen = None
        if args.with_legacy_retention or args.with_safe_cutover:
            import emissary_legacy_retention
            frozen = emissary_legacy_retention.compile_frozen(supervisor,directory,base_env,args.build_timeout)
        if legacy.source_manifest() != proof['source_manifest_sha256']:
            raise RuntimeError('source changed during build')
        deadline = time.monotonic() + 240
        proof['phase'] = 'resources'
        assert sql('postgres',f"SELECT count(*) FROM pg_database WHERE datname IN ('{business}','{telemetry}')") == '0'
        assert sql('postgres',f"SELECT count(*) FROM pg_roles WHERE rolname='{role}'") == '0'
        assert s3('HEAD') == 404
        reserved = True
        for name in (business,telemetry):
            sql('postgres',f'CREATE DATABASE {name}; COMMENT ON DATABASE {name} IS \'{nonce}\';')
            sql(name,'CREATE EXTENSION IF NOT EXISTS timescaledb; SELECT _timescaledb_functions.stop_background_workers();')
        sql('postgres',f"CREATE ROLE {role} LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD '{password}'; COMMENT ON ROLE {role} IS '{nonce}';")
        bucket_attempted = True
        assert s3('PUT') in (200,201)
        bucket_created = True
        pki = directory/'pki'
        pki.mkdir()
        run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','1','-sha256','-subj','/CN=Emissary isolated test CA',
             '-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign',
             '-addext','subjectKeyIdentifier=hash','-keyout',pki/'ca.key','-out',pki/'ca.pem'])
        run(['openssl','req','-new','-newkey','rsa:2048','-nodes','-subj','/CN=localhost','-keyout',pki/'server.key','-out',pki/'server.csr'])
        (pki/'server.ext').write_text('subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n')
        run(['openssl','x509','-req','-in',pki/'server.csr','-CA',pki/'ca.pem','-CAkey',pki/'ca.key','-set_serial','1','-days','1','-sha256','-extfile',pki/'server.ext','-out',pki/'server.pem'])
        run(['openssl','verify','-x509_strict','-purpose','sslserver','-verify_ip','127.0.0.1',
             '-CAfile',pki/'ca.pem',pki/'server.pem'])
        control_http, control_grpc, telemetry_http, telemetry_grpc = (address() for _ in range(4))
        assert len({control_http,control_grpc,telemetry_http,telemetry_grpc}) == 4
        gateway_http = address() if runtime_release else control_http
        base = 'http://'+gateway_http
        token = uuid.uuid4().hex
        bootstrap = uuid.uuid4().hex + 'Ax9!'
        control_env = dict(base_env, DATABASE_URL=db_url(business), ARTIFACTORY_HTTP_BIND=control_http,
            ARTIFACTORY_GRPC_BIND=control_grpc, ARTIFACTORY_PUBLIC_URL=base, ARTIFACTORY_INSECURE_DEV='true',
            ARTIFACTORY_PKI_DIR=str(pki), ARTIFACTORY_DATA_DIR=str(directory/'control'),
            ARTIFACTORY_MASTER_KEY=base64.b64encode(os.urandom(32)).decode(),
            ARTIFACTORY_S3_ENDPOINT='http://'+s3_address, ARTIFACTORY_S3_BUCKET=bucket,
            ARTIFACTORY_S3_ACCESS_KEY=credentials['accessKey'], ARTIFACTORY_S3_SECRET_KEY=credentials['secretKey'],
            ARTIFACTORY_BOOTSTRAP_USERNAME='acceptance', ARTIFACTORY_BOOTSTRAP_PASSWORD=bootstrap,
            ARTIFACTORY_DB_MAX_CONNECTIONS='8', ARTIFACTORY_CONTROL_DRAIN_SECONDS='1',
            ARTIFACTORY_TERMINAL_RELAY_BIND='127.0.0.1:0', ARTIFACTORY_REGISTRATION_ENABLED='false',
            ARTIFACTORY_TELEMETRY_ENDPOINT='https://'+telemetry_grpc, ARTIFACTORY_TELEMETRY_URL='https://'+telemetry_http,
            ARTIFACTORY_TELEMETRY_TOKEN=token)
        private_json(directory/'control-env.json',control_env)
        if runtime_release:
            gateway_token = uuid.uuid4().hex+uuid.uuid4().hex
            control_env['ARTIFACTORY_GATEWAY_TOKEN'] = gateway_token
            object_origin = 'http://'+s3_address
            if args.with_ota_renewal:
                object_origin = emissary_control_ota.renewal_proxy(runtime_release,directory=directory,
                    origin=object_origin,bucket=bucket,supervisor=supervisor,environment=base_env,wait=wait)
                control_env['ARTIFACTORY_S3_ENDPOINT'] = object_origin
            gateway_env = dict(base_env,ARTIFACTORY_INSECURE_DEV='true',
                ARTIFACTORY_GATEWAY_PROFILE='all',ARTIFACTORY_GATEWAY_HTTP_BIND=gateway_http,
                ARTIFACTORY_GATEWAY_CONTROL='http://'+control_http,
                ARTIFACTORY_GATEWAY_WEB='http://'+control_http,
                ARTIFACTORY_GATEWAY_REGISTRY='http://'+control_http,
                ARTIFACTORY_S3_ENDPOINT=object_origin,ARTIFACTORY_GATEWAY_TOKEN=gateway_token)
            start('artifactory-gateway',[],gateway_env,'gateway')
        telemetry_env = dict(base_env, DATABASE_URL=db_url(telemetry), ARTIFACTORY_BUSINESS_DATABASE_URL=db_url(business,True),
            ARTIFACTORY_TELEMETRY_GRPC_BIND=telemetry_grpc, ARTIFACTORY_TELEMETRY_HTTP_BIND=telemetry_http,
            ARTIFACTORY_PKI_DIR=str(pki), ARTIFACTORY_SERVICE_TLS_CERT=str(pki/'server.pem'), ARTIFACTORY_SERVICE_TLS_KEY=str(pki/'server.key'),
            ARTIFACTORY_TELEMETRY_TOKEN=token, ARTIFACTORY_TELEMETRY_WRITE_CONNECTIONS='4',
            ARTIFACTORY_TELEMETRY_READ_CONNECTIONS='2', ARTIFACTORY_TELEMETRY_AUTH_CONNECTIONS='2')
        old_retention = None
        if args.with_safe_cutover:
            import emissary_safe_cutover
            emissary_safe_cutover.exercise(supervisor=supervisor,directory=directory,environment=base_env,
                binaries=binaries,frozen=frozen,control_environment=control_env,telemetry_environment=telemetry_env,
                business=business,telemetry=telemetry,role=role,nonce=nonce,sql=sql,run=run,stop=stop,
                address=address,wait=wait,proof=proof)
            return 0
        if args.with_legacy_retention:
            old_retention = emissary_legacy_retention.Retention(supervisor=supervisor,directory=directory,
                environment=base_env,pg_environment=pg_env,binaries=frozen,control_environment=control_env,
                telemetry_environment=telemetry_env,business=business,telemetry=telemetry,role=role,nonce=nonce,
                sql=sql,address=address,wait=wait)
        run([binaries['artifactory-control-plane'],'migrate'],control_env,45)
        grant = (ROOT/'deploy/telemetry-business-readonly.sql').read_text().replace(':"business_database"','"'+business+'"').replace(':"business_schema"','"public"').replace(':"telemetry_business_role"','"'+role+'"')
        sql(business,grant)
        assert sql(business,'SHOW transaction_read_only',restricted=True) == 'off'
        for table in ('nodes','attempts','node_startups','__toasty_migrations'):
            sql(business,'SELECT count(*) FROM '+table+' WHERE false',restricted=True)
        for statement in ('UPDATE nodes SET revoked=true WHERE false',
                          'SELECT count(*) FROM users', 'CREATE TABLE forbidden_by_acl(id int)'):
            sql(business,statement,restricted=True,rejected=True)
        private_json(directory/'telemetry-env.json',telemetry_env)
        receiver = start('artifactory-telemetry-service',[],telemetry_env,'telemetry')
        controller = start('artifactory-control-plane',[],control_env,'control')
        ca_context = ssl.create_default_context(cafile=pki/'ca.pem')
        jar = http.cookiejar.CookieJar()
        client = urllib.request.build_opener(urllib.request.ProxyHandler({}),urllib.request.HTTPCookieProcessor(jar),urllib.request.HTTPSHandler(context=ca_context))
        def api(path,body=None,authenticated=True):
            headers = {'Origin':base,'x-artifactory-csrf':'1','Content-Type':'application/json'}
            if authenticated:
                headers['x-artifactory-tenant'] = '00000000-0000-0000-0000-000000000001'
            request = urllib.request.Request(base+path, data=None if body is None else json.dumps(body).encode(),headers=headers)
            with client.open(request,timeout=10) as response:
                return json.load(response)
        def ready():
            try:
                if runtime_release:
                    with client.open(base+'/healthz',timeout=5) as response:
                        if response.status!=200 or response.read():
                            return None
                    with client.open('http://'+control_http+'/healthz',timeout=5) as response:
                        return json.load(response)
                return api('/healthz',authenticated=False)
            except (OSError,urllib.error.URLError):
                return None
        wait(ready,'Control readiness')
        proof['phase'] = 'enrollment'
        api('/api/v1/auth/login',{'username':'acceptance','password':bootstrap},False)
        enrollment = api('/api/v1/enrollment-tokens',{'pool_id':'00000000-0000-0000-0000-000000000001'})['token']
        cfg = directory/'emissary.json'
        register_env = dict(base_env, ARTIFACTORY_ENROLLMENT_TOKEN=enrollment)
        run([binaries['artifactory-emissary'],'register','--config',cfg,'--root',directory/'node','--endpoint','https://'+control_grpc,'--ca',pki/'ca.pem','--name','isolated-uplink-evolution'], register_env,45)
        config = json.loads(cfg.read_text())
        probe = None
        controlled_tools = None
        if args.with_control_restart:
            probe = prepare_restart_probe(directory,config)
            private_json(cfg,config)
        if args.with_controlled_refresh:
            import emissary_controlled_refresh
            controlled_tools = emissary_controlled_refresh.prepare(directory,config)
            private_json(cfg,config)
        if runtime_release:
            emissary_control_ota.initialize(runtime_release,cfg,config,run)
        node = str(uuid.UUID(config['emissary_id']))
        assert config['telemetry_endpoint'] == 'https://'+telemetry_grpc
        identity_hash = legacy.digest(Path(config['certificate_file']))
        assert sql(business,"SELECT count(*) FROM enrollment_tokens WHERE used_at IS NOT NULL") == '1'
        token_state = sql(business,"SELECT json_agg(json_build_object('id',id,'used_at',used_at) ORDER BY id) FROM enrollment_tokens")
        node_state = sql(business,f"SELECT json_build_object('id',id,'certificate_hash',certificate_hash,'startup',startup_id,'session',active_session,'owner',connection_owner) FROM nodes WHERE id='{node}'")
        assert all(json.loads(node_state)[key] is None for key in ('startup','session','owner'))
        run([binaries['artifactory-emissary'],'register','--config',directory/'reused.json',
             '--root',directory/'reused-node','--endpoint','https://'+control_grpc,
             '--ca',pki/'ca.pem','--name','reused-token'],register_env,45,rejected=True)
        assert not (directory/'reused.json').exists()
        assert not any((directory/'reused-node'/name).exists() for name in ('node.key','node.pem'))
        assert sql(business,'SELECT count(*) FROM nodes') == '1'
        assert sql(business,"SELECT json_agg(json_build_object('id',id,'used_at',used_at) ORDER BY id) FROM enrollment_tokens") == token_state
        assert sql(business,f"SELECT json_build_object('id',id,'certificate_hash',certificate_hash,'startup',startup_id,'session',active_session,'owner',connection_owner) FROM nodes WHERE id='{node}'") == node_state
        proof['enrollment_token_reuse_denied'] = True
        print('Real Control login and node enrollment passed',flush=True)
        proof['phase'] = 'runner'
        def startup_rejections():
            request = urllib.request.Request('https://'+telemetry_http+'/metrics',headers={'Authorization':'Bearer '+token})
            with client.open(request,timeout=5) as response:
                content = response.read().decode()
            match = re.search(r'^artifactory_telemetry_rejection_reason_total\{reason="startup_unregistered"\} ([0-9]+)$',content,re.MULTILINE)
            if not match:
                raise RuntimeError('startup rejection metric missing')
            return int(match[1])
        rejection_baseline = startup_rejections()
        lock_name = 'emissary_startup_'+nonce
        fence = hold_node(supervisor,directory,pg_env,business,node,lock_name)
        wait(lambda:(directory/'startup-fence-ready').exists(),'startup row lock acquired')
        fence_pid = int(wait(lambda:sql(business,f"SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND application_name='{lock_name}'"),'unique owned startup fence backend'))
        source = None
        startups = []
        for index in range(2):
            runner_binary = runtime_release['baseline'] if runtime_release else binaries['artifactory-emissary']
            runner = supervisor.start([runner_binary,'run','--config',cfg],cwd=ROOT,env=base_env,
                log=directory/('runner-'+str(index)+'.log'),label='runner-'+str(index))
            if index == 0:
                source = wait(lambda:inventory_source(directory/'node/telemetry/inventories',base_env),'real pending startup source')
                wait(lambda:sql(business,f"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND {fence_pid}=ANY(pg_blocking_pids(pid)) AND query LIKE '%SELECT startup_id FROM nodes%FOR UPDATE%')")=='t','real Control blocked registration')
                wait(lambda:startup_rejections()>rejection_baseline,'real Telemetry unregistered-startup rejection')
                assert sql(business,f"SELECT count(*) FROM node_startups WHERE node_id='{node}'") == '0'
                assert sql(business,f"SELECT json_build_object('id',id,'certificate_hash',certificate_hash,'startup',startup_id,'session',active_session,'owner',connection_owner) FROM nodes WHERE id='{node}'") == node_state
                assert sql(telemetry,f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}'") == '0'
                assert source['path'].read_bytes() == source['bytes']
                assert inventory_source(directory/'node/telemetry/inventories',base_env) == source
                assert sql(business,f"SELECT pg_cancel_backend(pid) FROM pg_stat_activity WHERE pid={fence_pid} AND application_name='{lock_name}' AND datname=current_database()") == 't'
                assert fence.wait(10) != 0
                assert '57014' in (directory/'startup-fence.log').read_text()
                assert sql(business,f"SELECT count(*) FROM pg_stat_activity WHERE pid={fence_pid} AND application_name='{lock_name}'") == '0'
                proof['startup_registration_fence'] = {'actual_control_wait':True,
                    'actual_telemetry_retryable_rejection':True,'receipts_zero_before_commit':True,
                    'source_bytes_retained':True,'source_sha256':hashlib.sha256(source['bytes']).hexdigest(),
                    'source_bytes':len(source['bytes']),'lock_owner_reaped':True}
            def current():
                raw = sql(business,f"SELECT json_build_object('startup',startup_id,'digest',metadata_digest,'session',active_session) FROM nodes WHERE id='{node}' AND connection_owner IS NOT NULL AND startup_id IS NOT NULL")
                value = json.loads(raw) if raw else None
                return value if value and value['startup'] not in startups else None
            registered = wait(current,'real Runner Hello')
            startup = str(uuid.UUID(registered['startup']))
            startups.append(startup)
            assert sql(business,f"SELECT digest FROM node_startups WHERE node_id='{node}' AND startup_id='{startup}'") == registered['digest']
            def received():
                return sql(telemetry,f"SELECT EXISTS(SELECT 1 FROM inventory_deliveries WHERE node_id='{node}' AND startup_id='{startup}' AND revision=0) AND EXISTS(SELECT 1 FROM startup_hosts WHERE node_id='{node}' AND startup_id='{startup}')") == 't'
            wait(received,'real Runner telemetry',65)
            checkpoint_path = directory/'node/telemetry/inventories/startup.json'
            wait(lambda:startup_acknowledged(checkpoint_path,startup,source if index==0 else None),
                 'real Runner startup inventory ACK and source removal')
            if index == 0:
                assert startup == source['startup']
                raw = sql(telemetry,f"SELECT json_build_object('digest',delivery_digest,'time',(extract(epoch FROM observed_at)*1000)::bigint) FROM inventory_deliveries WHERE node_id='{node}' AND observation_id='{source['observation']}' AND startup_id='{startup}' AND revision=0 AND source_batch_id IS NULL")
                receipt = json.loads(raw)
                assert receipt == {'digest':source['delivery_digest'],'time':source['observed_at_ms']}
                proof['startup_registration_fence']['original_receipt_and_client_ack'] = True
                if old_retention:
                    proof['legacy_retention'] = old_retention.release_and_observe(node,startup)
                    old_retention.close()
            latest = api('/api/v1/nodes/'+node+'/telemetry')
            assert latest['host']['startup_id'] == startup and latest['inventory']['startup_id'] == startup
            assert latest['inventory']['provenance'] == 'native'
            before = registered['digest']
            if index == 0:
                if controlled_tools:
                    proof['controlled_refresh'] = emissary_controlled_refresh.exercise(
                        directory=directory,tools=controlled_tools,node=node,startup=startup,
                        supervisor=supervisor,pg_environment=pg_env,database=telemetry,
                        environment=base_env,nonce=nonce,sql=lambda query:sql(telemetry,query),
                        business_sql=lambda query:sql(business,query),api=api,wait=wait,
                        operation_acknowledged=operation_acknowledged)
                    print('Controlled full refresh committed original observation without changing startup metadata',flush=True)
                if args.with_control_restart:
                    controller, proof['control_restart'] = exercise_control_restart(controller,
                        directory=directory,node=node,startup=startup,probe=probe,api=api,
                        sql=lambda query:sql(business,query),wait=wait,
                        restart=lambda:start('artifactory-control-plane',[],control_env,'control-restarted'))
                    print('Offline operation replayed once after actual Control process failure',flush=True)
                operations = [('collect_diagnostics',{'probes':['git']})]
                if not controlled_tools:
                    operations.append(('refresh_inventory',{'mode':'full'}))
                for kind,data in operations:
                    body = {'request_id':str(uuid.uuid4()),'kind':kind,'schema':1,'input':data}
                    operation = api('/api/v1/nodes/'+node+'/operations',body)
                    operation_id = str(uuid.UUID(operation['id']))
                    duplicate = api('/api/v1/nodes/'+node+'/operations',body)
                    assert duplicate['id'] == operation_id
                    def finished():
                        value = api('/api/v1/node-operations/'+operation_id)
                        return value if value['state'] in ('succeeded','failed','interrupted','canceled','expired','rejected') else None
                    result = wait(finished,kind,55)
                    private_json(directory/(kind+'-result.json'),result)
                    reports = result['result']['probes']
                    failed_probes = {name: {'status':report['status'],'error_code':report['error_code']}
                                     for name,report in reports.items() if report['status'] not in ('available','unsupported')}
                    if kind == 'collect_diagnostics':
                        assert result['state'] == 'succeeded'
                    elif result['state'] == 'failed':
                        assert failed_probes and result['result']['inventory'] is None
                        assert result['reason'] is None
                        proof['refresh_partial_failure'] = failed_probes
                        proof['refresh_outcome'] = 'partial_failure'
                        print('Full refresh returned explicit partial probe failure:',json.dumps(failed_probes),flush=True)
                    else:
                        assert result['state'] == 'succeeded'
                        proof['refresh_outcome'] = 'succeeded'
                    assert result['delivery_state'] == 'confirmed' and result['result_digest']
                    assert result['startup_id'] == startup
                    assert result['request_id'] == body['request_id'] and result['input'] == data
                    wait(lambda:operation_acknowledged(directory/'node/operations/journal.json',result),kind+' device result ACK')
                if proof.get('refresh_partial_failure'):
                    assert sql(telemetry,f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND startup_id='{startup}' AND revision>0") == '0'
                elif not controlled_tools:
                    wait(lambda: sql(telemetry,f"SELECT EXISTS(SELECT 1 FROM inventory_deliveries WHERE node_id='{node}' AND startup_id='{startup}' AND revision=1)")=='t','refreshed inventory persisted')
                if controlled_tools:
                    calls = emissary_controlled_refresh.counts(controlled_tools)
                    assert calls=={name:(b'xxx' if name=='git' else b'xx') for name in calls}
                    assert sql(telemetry,f"SELECT count(*) FROM inventory_deliveries WHERE node_id='{node}' AND startup_id='{startup}' AND revision>0")=='1'
                    proof['refresh_outcome'] = 'succeeded_with_controlled_tool_fixture'
                assert sql(business,f"SELECT metadata_digest FROM nodes WHERE id='{node}'") == before
                assert sql(business,f"SELECT count(*) FROM node_operations WHERE node_id='{node}' AND state IN ('succeeded','failed') AND delivery_state='confirmed'") == str(3 if args.with_control_restart else 2)
                print('Real finite diagnostics and full refresh result confirmed',flush=True)
                if runtime_release:
                    startup,proof['runtime_ota'] = emissary_control_ota.exercise(runtime_release,
                        directory=directory,launcher=runner,node=node,startup=startup,config=config,
                        api=api,sql=lambda query:sql(business,query),wait=wait)
                    startups.append(startup)
                    wait(received,'OTA candidate inventory and host transactions',30)
                    wait(lambda:startup_acknowledged(checkpoint_path,startup),'OTA candidate inventory ACK',30)
                    candidate_latest = api('/api/v1/nodes/'+node+'/telemetry')
                    assert candidate_latest['host']['startup_id']==startup
                    assert candidate_latest['inventory']['startup_id']==startup
                    assert candidate_latest['inventory']['provenance']=='native'
                    assert sql(business,f"SELECT count(*) FROM node_startups WHERE node_id='{node}' AND startup_id='{startup}'")=='1'
                    proof['runtime_ota']['candidate_inventory_transaction_and_ack'] = True
                    proof['runtime_ota']['candidate_current_native_projection'] = True
                    print('Actual signed runtime OTA confirmed through real Control and gateway',flush=True)
            assert legacy.digest(Path(config['certificate_file'])) == identity_hash
            assert json.loads(cfg.read_text()) == config
            stop(runner)
            assert startup_acknowledged(checkpoint_path,startup,source if index==0 and not runtime_release else None)
        proof.update(real_control_main=True, real_telemetry_main=True, real_enrollment=True,
            real_runner_startups=len(startups), distinct_startups=True, immutable_startup_digest_readback=True,
            current_startup_projection=True, finite_operations_confirmed=3 if args.with_control_restart else 2,
            operation_duplicate_request_same_id=True, refresh_preserved_scheduling_digest=True,
            two_startup_inventory_receipts=True, two_startup_client_acks=True,
            operation_client_acks=3 if args.with_control_restart else 2, same_configuration_and_identity=True,
            business_read_role=True, strict_internal_https=True, public_http_loopback_dev=True,
            native_service_executed=False)
        for binary in ('artifactory-control-plane','artifactory-telemetry-service','artifactory-emissary'):
            proof[binary+'_sha256'] = legacy.digest(binaries[binary])
        if runtime_release:
            proof['artifactory-gateway_sha256'] = legacy.digest(binaries['artifactory-gateway'])
        stop(controller)
        stop(receiver)
        assert proof['source_manifest_sha256'] == legacy.source_manifest()
        proof['work_passed'] = True
    except (Exception, KeyboardInterrupt) as error:
        proof['error_type'] = type(error).__name__
        proof['error'] = 'acceptance failed; inspect private diagnostic directory'
        raise
    finally:
        # 清理必须完成所有读回；二次普通中断不能跳过剩余资源。
        prior = {sig: signal.signal(sig, signal.SIG_IGN) for sig in (signal.SIGINT, signal.SIGTERM)}
        errors = []
        if runtime_release:
            try:
                from emissary_ota_processes import close_launchers
                # Supervisor在重放延迟中断前已登记；不依赖调用点再次登记。
                close_launchers([child for child in supervisor.children if child.label in ('runner-0','runner-1')])
                proof['runtime_sessions_cleanup_confirmed'] = True
            except Exception:
                errors.append('runtime_session_cleanup_unconfirmed')
        try:
            supervisor.cleanup()
            proof['process_cleanup_confirmed'] = supervisor.cleanup_confirmed()
            if not proof['process_cleanup_confirmed']:
                errors.append('prior_process_cleanup_unconfirmed')
        except Exception:
            errors.append('process_cleanup_unconfirmed')
        if bucket_created:
            try:
                for key in ota_keys:
                    assert s3('DELETE',key) in (200,204)
                    assert s3('HEAD',key)==404
                if ota_keys:
                    proof['owned_release_objects_removed'] = len(ota_keys)
                assert s3('DELETE') in (200,204)
                assert s3('HEAD') == 404
                proof['empty_owned_bucket_removed'] = True
            except Exception:
                errors.append('bucket_cleanup_unconfirmed')
        elif bucket_attempted:
            # PUT响应不明时不盲删；保留清单供人工确认。
            errors.append('bucket_creation_unconfirmed')
        if reserved:
            try:
                cleanup_databases(sql, business, telemetry, role, nonce)
                proof['two_databases_and_role_removed'] = True
            except Exception:
                errors.append('database_cleanup_unconfirmed')
        proof['cleanup_errors'] = errors
        if proof.get('work_passed') and not errors:
            proof['status'] = 'passed'
            proof['phase'] = 'complete'
        proof['elapsed_seconds'] = round(time.monotonic() - started_at, 3)
        private_json(directory/'proof.json',proof)
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        private_json(args.evidence,proof)
        print('Real Control main acceptance:',proof['status'],'; cleanup errors:',errors,flush=True)
        for sig, handler in prior.items():
            signal.signal(sig, handler)
        if errors:
            raise RuntimeError('owned resource cleanup unconfirmed')
    return 0 if proof['status'] == 'passed' else 1


if __name__ == '__main__':
    signal.signal(signal.SIGTERM,lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    raise SystemExit(main())
