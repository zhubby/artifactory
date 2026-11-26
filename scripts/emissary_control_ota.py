#!/usr/bin/env python3
"""[INPUT]: 当前程序、同源码旧版本号夹具、真实Control与私有S3。
[OUTPUT]: 验证HTTP签名导入、显式升级、固定launcher和候选健康确认。
[POS]: 普通进程OTA组合验收；不证明历史版本或六平台服务兼容。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import uuid

from check_emissary_legacy_sender import private_json
import check_emissary_legacy_receiver as legacy
from emissary_ota_mailbox import RenewalObserver

ROOT = Path(__file__).resolve().parents[1]
BASELINE_VERSION = '0.0.0-acceptance'


def build_baseline(supervisor, directory, environment, timeout, current):
    candidate = directory/'ota-current-emissary'
    shutil.copy2(current,candidate)
    candidate.chmod(0o700)
    version = (ROOT/'VERSION').read_text().strip()
    commit = subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,env=environment,text=True,timeout=5).strip()
    expected = f'artifactory-emissary {version} '
    output = subprocess.check_output([candidate,'version'],env=environment,text=True,timeout=5)
    if not output.startswith(expected) or not output.strip().endswith('commit='+commit):
        raise RuntimeError('candidate must report current product and full source commit')
    platform = re.fullmatch(re.escape(expected)+r'(linux|darwin|windows)/(amd64|arm64) protocol=1 commit='+commit+r'\s*',output)
    if not platform:
        raise RuntimeError('candidate platform or protocol differs')
    build_env = dict(environment,ARTIFACTORY_BUILD_VERSION=BASELINE_VERSION,ARTIFACTORY_BUILD_COMMIT=commit)
    for key in ('CARGO_HOME','RUSTUP_HOME','RUSTUP_TOOLCHAIN','CARGO_TARGET_DIR',
                'CARGO_BUILD_JOBS','RUSTFLAGS','CARGO_PROFILE_DEV_DEBUG'):
        if key in os.environ:
            build_env[key] = os.environ[key]
    log = directory/'ota-baseline-build.log'
    child = supervisor.start(['cargo','build','--locked','-p','artifactory-emissary',
                              '--bin','artifactory-emissary','--message-format=json'],
                             cwd=ROOT,env=build_env,log=log,label='build-ota-version-fixture')
    if child.wait(timeout):
        raise RuntimeError('controlled baseline compilation failed; inspect private log')
    events = [json.loads(line) for line in log.read_text().splitlines() if line.startswith('{')]
    artifacts = [Path(event['executable']) for event in events if event.get('reason')=='compiler-artifact'
                 and event.get('target',{}).get('name')=='artifactory-emissary' and event.get('executable')]
    if len(artifacts)!=1:
        raise RuntimeError('baseline executable not uniquely identified')
    baseline = directory/'ota-baseline-emissary'
    shutil.copy2(artifacts[0],baseline)
    baseline.chmod(0o700)
    report = subprocess.check_output([baseline,'version'],env=environment,text=True,timeout=5)
    if report != output.replace(version,BASELINE_VERSION,1):
        raise RuntimeError('controlled baseline metadata differs')
    if legacy.digest(candidate)==legacy.digest(baseline):
        raise RuntimeError('controlled baseline must differ from candidate')
    # 测试覆盖同一Cargo输出；恢复正常构建，避免后续误用夹具版本。
    build_env.pop('ARTIFACTORY_BUILD_VERSION',None)
    restore = supervisor.start(['cargo','build','--locked','-p','artifactory-emissary',
                                '--bin','artifactory-emissary'],cwd=ROOT,env=build_env,
                               log=directory/'ota-current-restore.log',label='restore-current-runtime')
    if restore.wait(timeout):
        raise RuntimeError('current runtime restoration failed; inspect private log')
    restored = subprocess.check_output([artifacts[0],'version'],env=environment,text=True,timeout=5)
    restored_hash = legacy.digest(artifacts[0])
    if restored!=output or restored_hash==legacy.digest(baseline):
        raise RuntimeError('restored Cargo runtime retains controlled baseline metadata')
    return {'candidate':candidate,'baseline':baseline,'version':version,'commit':commit,
            'os':platform[1],'arch':platform[2],'restored_build_sha256':restored_hash}


def prepare(release, *, directory, binaries, supervisor, environment, run, wait):
    assets = directory/'ota-release'
    assets.mkdir()
    real_name = f'artifactory-emissary-{release["os"]}-{release["arch"]}'
    if release['os']=='windows':
        real_name += '.exe'
    for operating in ('linux','darwin','windows'):
        for arch in ('amd64','arm64'):
            name = f'artifactory-emissary-{operating}-{arch}'+('.exe' if operating=='windows' else '')
            if name==real_name:
                shutil.copy2(release['candidate'],assets/name)
            else:
                (assets/name).write_bytes(('NONEXECUTABLE ACCEPTANCE FIXTURE '+name).encode())
    seed = os.urandom(32)
    private_key = directory/'ota-test-key.der'
    private_key.write_bytes(bytes.fromhex('302e020100300506032b657004220420')+seed)
    public = subprocess.check_output(['openssl','pkey','-inform','DER','-in',private_key,
                                     '-pubout','-outform','DER'],env=environment,timeout=5)
    if len(public)!=44 or public[:12]!=bytes.fromhex('302a300506032b6570032100'):
        raise RuntimeError('unexpected Ed25519 public key encoding')
    release['public_key'] = base64.b64encode(public[-32:]).decode()
    run([binaries['emissary-release'],'--directory',assets,'--version',release['version'],
         '--commit',release['commit']],dict(environment,ARTIFACTORY_EMISSARY_RELEASE_SIGNING_SEED=base64.b64encode(seed).decode()))
    raw = (assets/'emissary-manifest.json').read_bytes()
    manifest = json.loads(raw)
    release.update(manifest=manifest,manifest_sha256=hashlib.sha256(raw).hexdigest(),assets=assets)
    source_address = directory/'ota-source-address'
    release['source_process'] = supervisor.start([sys.executable,ROOT/'scripts/emissary_ota_fixture.py','--directory',assets,
                      '--version',release['version'],'--address',source_address],cwd=ROOT,
                     env=environment,log=directory/'ota-source.log',label='owned-release-source')
    source = wait(lambda:source_address.read_text() if source_address.exists() else None,'owned release source',10)
    configuration = directory/'control/emissary-releases.json'
    configuration.parent.mkdir(exist_ok=True)
    private_json(configuration,{'repository':'acceptance/emissary','api_base':source,
                                'public_keys':[release['public_key']]})
    return [f'emissary-releases/{release["manifest_sha256"]}/{asset["name"]}' for asset in manifest['assets']]


def initialize(release, config_path, config, run):
    config['allow_http_uploads'] = True
    private_json(config_path,config)
    run([release['baseline'],'ota-init','--config',config_path,'--public-keys',release['public_key']])
    state = json.loads((Path(config['root'])/'ota/state.json').read_bytes())
    assert state['current']['version']==BASELINE_VERSION and state['generation']==0
    assert legacy.digest(Path(state['current']['path']))==legacy.digest(release['baseline'])


def renewal_proxy(release, *, directory, origin, bucket, supervisor, environment, wait):
    fault = directory/'s3-fault'
    fault.mkdir()
    supervisor.start([sys.executable,ROOT/'scripts/emissary_s3_fault.py','--origin',origin,
                      '--bucket',bucket,'--directory',fault],cwd=ROOT,env=environment,
                     log=directory/'s3-fault.log',label='owned-s3-fault-relay')
    release['fault'] = fault
    return wait(lambda:(fault/'address').read_text() if (fault/'address').exists() else None,
                'S3 fault relay ready',10)


def exercise(release, *, directory, launcher, node, startup, config, api, sql, wait):
    ota = Path(config['root'])/'ota'
    before = json.loads((ota/'state.json').read_bytes())
    before_health = json.loads((ota/'health.json').read_bytes())
    capabilities = json.loads((ota/'capabilities.json').read_bytes())
    baseline_hash = legacy.digest(Path(before['current']['path']))
    launcher_hash = legacy.digest(release['baseline'])
    certificate_hash = legacy.digest(Path(config['certificate_file']))
    assert not launcher.finished()
    row = json.loads(sql(f"SELECT json_build_object('version',emissary_current_version,'startup',startup_id,'session',active_session,'updates',(SELECT count(*) FROM emissary_updates)) FROM nodes WHERE id='{node}'"))
    assert row['version']==BASELINE_VERSION and row['startup']==startup and row['updates']==0
    assert row['session'] and before_health['session']
    imported = api('/api/v1/emissary-releases',{'tag':'v'+release['version']})
    release_id = str(uuid.UUID(imported['id']))
    assert api('/api/v1/emissary-version')['release_id']==release_id
    assert sql('SELECT count(*) FROM emissary_updates')=='0'
    time.sleep(2)
    assert json.loads((ota/'state.json').read_bytes())==before
    assert sql('SELECT count(*) FROM emissary_updates')=='0'
    assert api('/api/v1/emissary-releases',{'tag':'v'+release['version']})['id']==release_id
    release['source_process'].close()
    assert release['source_process'].confirmed
    candidate_hash = legacy.digest(release['candidate'])
    assert not (ota/'versions'/candidate_hash).exists(), 'candidate asset was already cached'
    request = {'product_version':release['version'],'release_id':release_id}
    if 'fault' in release:
        (release['fault']/'arm').touch()
    phases, health_ids, sessions = set(),set(),set()
    def activated():
        assert not launcher.finished(), 'fixed launcher exited during runtime update'
        state = json.loads((ota/'state.json').read_bytes())
        if state['update_id']!=update_id:
            return None
        phases.add(state['phase'])
        assert state['phase'] not in ('failed','rolled_back','canceled'), state['phase']+': '+state['error']
        health_path = ota/'health.json'
        if state['phase'] in ('activating','succeeded') and health_path.exists():
            health = json.loads(health_path.read_bytes())
            active = json.loads(sql(f"SELECT json_build_object('session',active_session,'startup',startup_id) FROM nodes WHERE id='{node}'"))
            # Runner连接标识与服务端授权Session是两个独立命名空间。
            if health['session']!=before_health['session'] and active['startup']!=startup and health['id']:
                sessions.add(health['session'])
                health_ids.add(health['id'])
        updates = api('/api/v1/emissary-updates')
        rows = [value for value in updates if value['id']==update_id]
        if state['phase']=='succeeded' and len(health_ids)>=3 and rows and rows[0]['status']=='succeeded' and rows[0]['finished_at']:
            return state,rows[0]
        return None
    with RenewalObserver(ota) as mailboxes:
        update = api('/api/v1/nodes/'+node+'/emissary-update',request)
        update_id = str(uuid.UUID(update['id']))
        after,result = wait(activated,'actual candidate health and Control terminal confirmation',110)
    assert len(sessions)==1 and len(health_ids)>=3
    assert after['generation']>0 and after['current']['version']==release['version']
    assert after['previous']==before['current'] and not after['candidate']['path']
    assert legacy.digest(Path(after['current']['path']))==legacy.digest(release['candidate'])
    assert legacy.digest(Path(after['previous']['path']))==baseline_hash
    assert legacy.digest(release['baseline'])==launcher_hash
    assert json.loads((ota/'capabilities.json').read_bytes())['instance_id']==capabilities['instance_id']
    assert legacy.digest(Path(config['certificate_file']))==certificate_hash
    assert json.loads((directory/'emissary.json').read_bytes())==config
    current = json.loads(sql(f"SELECT json_build_object('startup',startup_id,'session',active_session,'version',emissary_current_version,'draining',ota_draining) FROM nodes WHERE id='{node}'"))
    assert current['startup']!=startup and current['session']!=row['session'] and current['session']
    assert current['version']==release['version'] and not current['draining']
    try:
        api('/api/v1/nodes/'+node+'/emissary-update',request)
    except urllib.error.HTTPError as error:
        assert error.code==409 and 'already matches' in error.read().decode()
    else:
        raise AssertionError('same current product was accepted for another upgrade')
    assert sql('SELECT count(*) FROM emissary_updates')=='1'
    saved = json.loads(sql(f"SELECT json_build_object('manifest',r.manifest,'hash',r.manifest_hash,'status',u.status,'generation',u.generation) FROM emissary_updates u JOIN emissary_releases r ON r.id=u.release_id WHERE u.id='{update_id}'"))
    assert saved['hash']==release['manifest_sha256'] and json.loads(saved['manifest'])==release['manifest']
    assert saved['status']=='succeeded' and saved['generation']==after['generation']
    renewal = None
    if 'fault' in release:
        transfers = [json.loads(line) for line in (release['fault']/'requests.jsonl').read_text().splitlines()]
        assert len(transfers)==2 and [value['denied'] for value in transfers]==[True,False]
        assert transfers[0]['path']==transfers[1]['path']
        assert transfers[0]['query_sha256']!=transfers[1]['query_sha256']
        assert not (ota/'url-refresh-request.json').exists() and not (ota/'url-refresh-reply.json').exists()
        renewal = {'actual_s3_get_403':True,'renewed_s3_query_differs':True,
                   'same_frozen_object_path':True,'get_requests':2,'private_mailboxes_removed':True}
        asset = next(asset for asset in release['manifest']['assets']
                     if (asset['os'],asset['arch'])==(release['os'],release['arch']))
        renewal['exchange'] = mailboxes.receipt(update=update_id,generation=after['generation'],
            manifest=release['manifest_sha256'],asset=asset)
    requests = [json.loads(line) for line in (release['assets']/'requests.jsonl').read_text().splitlines()]
    assert {request['path'] for request in requests}=={'/repos/acceptance/emissary/releases/tags/v'+release['version']} | {'/repos/acceptance/emissary/releases/assets/'+str(index) for index in range(1,9)}
    private_json(directory/'ota-terminal.json',{'state':after,'update':result,'node':current})
    return current['startup'],{'signed_http_import':True,'explicit_admin_apply':True,
        'release_import_did_not_start_update':True,'current_version_reapply_denied':True,
        'release_source_stopped_before_apply':True,'candidate_not_precached':True,
        'full_control_gateway_s3_download':True,'unchanged_launcher_pid':launcher.process.pid,
        'unchanged_launcher_instance':capabilities['instance_id'],
        'actual_candidate_new_startup':current['startup'],'same_session_health_ack_ids':len(health_ids),
        'control_terminal_succeeded':True,'frozen_manifest_sha256':release['manifest_sha256'],
        'candidate_sha256':legacy.digest(release['candidate']),'baseline_sha256':baseline_hash,
        'restored_cargo_binary_sha256':release['restored_build_sha256'],
        'controlled_baseline_version':BASELINE_VERSION,'baseline_uses_current_source':True,
        'other_five_platform_assets':'nonexecutable test fixtures; not cross-platform acceptance',
        'observed_phases':sorted(phases),'previous_runtime_retained':True,
        'native_service_executed':False,'historical_release_compatibility':False,
        'frozen_asset_renewal':renewal}
