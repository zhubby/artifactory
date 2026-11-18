#!/usr/bin/env python3
"""[INPUT]: 已保存的隔离命名空间与遥测服务。
[OUTPUT]: 验证离线重放，保存白名单构建与准入诊断。
[POS]: 只改变遥测副本；准入检查保留原池容量。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
from datetime import datetime, timedelta, timezone
import importlib.util
import json
import os
from pathlib import Path
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('operator_acceptance', Path(__file__).with_name('run.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def record_build(a, build, detail=None):
    snapshot = {'id': build}
    try:
        detail = detail if detail is not None else a.api('/api/v1/builds/' + build)
        snapshot.update({key: detail.get(key) for key in ('id', 'status', 'source_manifest_id', 'failure_kind')})
        snapshot['jobs'] = [
            {'id': job.get('id'), 'status': job.get('status'), 'attempts': [
                {key: attempt.get(key) for key in ('id', 'status', 'node_id', 'failure_kind')}
                for attempt in job.get('attempts', [])]}
            for job in detail.get('jobs', [])]
    except (OSError, RuntimeError, ValueError):
        snapshot['build_unavailable'] = True
    runs = []
    try:
        runs = a.api('/api/v1/builds/' + build + '/plans')['runs']
        snapshot['runs'] = [{key: run.get(key) for key in ('id', 'status', 'generation')} for run in runs]
        if runs:
            run_id = str(uuid.UUID(runs[0]['id']))
            raw = a.sql('planner', "SELECT jsonb_build_object('id',id,'status',status,'generation',lifecycle->'generation','calls',lifecycle->'calls','pending',lifecycle->'pending','canceled',lifecycle->'canceled','deadline_ms',lifecycle->'deadline_ms') FROM runtime_runs WHERE id='" + run_id + "'")
            state = json.loads(raw) if raw else None
            snapshot['current_run'] = None if state is None else {key: state.get(key) for key in
                ('id', 'status', 'generation', 'calls', 'pending', 'canceled', 'deadline_ms')}
    except (OSError, RuntimeError, ValueError, KeyError):
        snapshot['runtime_unavailable'] = True
    try:
        resources = json.loads(a.kubectl('get', 'sourcetasks,agentruns,agentpools', '-o', 'json'))['items']
        run_ids = {run['id'] for run in runs}
        snapshot['resources'] = []
        for value in resources:
            spec = value.get('spec', {})
            if (value['kind'] != 'AgentPool' and spec.get('taskId') != build and
                    spec.get('runId') not in run_ids):
                continue
            snapshot['resources'].append({
                'kind': value['kind'],
                'metadata': {key: value.get('metadata', {}).get(key) for key in ('name', 'uid', 'generation', 'deletionTimestamp')},
                'spec': {key: spec.get(key) for key in ('taskId', 'runId', 'generation', 'pool', 'maxActive', 'cancelRequested') if key in spec},
                'status': {key: value.get('status', {}).get(key) for key in ('phase', 'observedGeneration', 'podUid')},
            })
    except (OSError, RuntimeError, ValueError, KeyError):
        snapshot['resources_unavailable'] = True
    a.evidence['telemetry_build_snapshot'] = snapshot
    (a.run / 'build.json').write_text(json.dumps(snapshot, indent=2))
    (a.run / 'evidence.json').write_text(json.dumps(a.evidence, indent=2))
    return detail


def require_admission(a):
    pool = json.loads(a.kubectl('get', 'agentpool', 'default', '-o', 'json'))
    status = pool.get('status', {})
    a.evidence['telemetry_pool'] = {'max_active': pool['spec']['maxActive'], 'phase': status.get('phase')}
    (a.run / 'evidence.json').write_text(json.dumps(a.evidence, indent=2))
    a.check('telemetry build has an admitting AgentPool', pool['spec']['maxActive'] > 0 and
            status.get('phase') == 'Ready' and status.get('observedGeneration') == pool['metadata']['generation'])


def run(args):
    a = module.Acceptance(args.context, args.namespace, resume=True)
    stopped = False
    build = None
    try:
        a.resume()
        require_admission(a)
        for database in ('business', 'planner', 'telemetry'):
            a.check(database+' uses TimescaleDB', a.sql(database, "SELECT extversion FROM pg_extension WHERE extname='timescaledb'") == '2.32.0')
        node = next(n for n in a.api('/api/v1/nodes') if n['name'] == 'operator-native-device' and n['online'])
        latest = a.wait(lambda: (v if (v := a.api('/api/v1/nodes/'+node['id']+'/telemetry'))['host'] and v['host']['fresh'] and v['inventory'] else None))
        inventory = latest['inventory']['inventory']
        a.check('versioned OS, hardware, quota and tool inventory is available', inventory['logical_cpus'] > 0 and inventory['physical_memory_bytes'] > 0 and inventory['tools']['git']['available'] and 'collectors' in inventory)
        a.check('business tables remain ordinary relational tables', all(a.sql(db,"SELECT count(*) FROM timescaledb_information.hypertables WHERE hypertable_schema='public'") == '0' for db in ('business','planner')))
        a.kubectl('scale','deploy/native-telemetry','--replicas=0');stopped=True
        a.wait(lambda: not json.loads(a.kubectl('get','pods','-l','app.kubernetes.io/name=artifactory-telemetry','-o','json'))['items'])
        a.service_request(39443,'/test/mode',{'build_seconds':25})
        build = a.api(f'/api/v1/projects/{a.project}/builds',{'ref':'v-fixture','goal':'Build dist/result.txt while telemetry is unavailable','target_requirements':[{'os':'linux','arch':a.arch}]})['id']
        def terminal():
            value=record_build(a, build)
            if value is None:return None
            if value['status'] in ('failed','lost','canceled','needs_input'):
                raise AssertionError('Build failed during telemetry outage: '+json.dumps(a.evidence['telemetry_build_snapshot']))
            return value if value['status']=='succeeded' else None
        detail=a.wait(terminal,600)
        current=next(n for n in a.api('/api/v1/nodes') if n['id']==node['id'])
        age=datetime.now(timezone.utc)-datetime.fromisoformat(current['last_seen'].replace('Z','+00:00'))
        a.check('telemetry outage leaves planning, leases, heartbeats and artifacts operational',detail['status']=='succeeded' and age.total_seconds()<20 and bool(detail['artifacts']))
        buffered=a.kubectl('exec','deploy/device','--','sh','-ec','find /tmp/emissary/telemetry -name "*.batch" | wc -l').strip()
        a.check('offline telemetry persists in its independent spool',int(buffered)>0)
        attempt=a.sql('business',f"SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id WHERE j.build_id='{build}' ORDER BY a.created_at DESC LIMIT 1")
        assert len(attempt)==36
        a.kubectl('scale','deploy/native-telemetry','--replicas=1');stopped=False
        a.kubectl('rollout','status','deploy/native-telemetry','--timeout=180s')
        a.wait(lambda: int(a.sql('telemetry',f"SELECT count(*) FROM process_samples WHERE attempt_id='{attempt}'"))>0)
        a.wait(lambda: int(a.kubectl('exec','deploy/device','--','sh','-ec','find /tmp/emissary/telemetry -name "*.batch" | wc -l').strip())==0)
        a.check('restart replays build process samples and drains the spool',True)
        now=datetime.now(timezone.utc)
        params=urllib.parse.urlencode({'from':(now-timedelta(minutes=10)).isoformat(),'to':(now+timedelta(seconds=5)).isoformat(),'resolution':'raw'})
        samples=a.api(f'/api/v1/attempts/{attempt}/processes?'+params)
        a.check('authenticated build process history includes PID identity and step',bool(samples['samples']) and not samples['truncated'] and any(p['step']==0 for p in samples['samples']) and all(p['started_at']>0 for p in samples['samples']))
        a.check('process protocol excludes command lines and environment values',all(not {'cmd','command_line','environment','environ','cwd','exe'} & set(p) for p in samples['samples']))
        host=a.api('/api/v1/nodes/'+node['id']+'/telemetry/history?'+params)
        a.check('host history is independent of the heartbeat table',bool(host['samples']))
        a.check('raw hypertables, continuous aggregates and retention policies are installed',a.sql('telemetry',"SELECT count(*) FROM timescaledb_information.hypertables WHERE hypertable_name IN ('host_samples','process_samples','build_samples')")=='3' and a.sql('telemetry',"SELECT count(*) FROM timescaledb_information.continuous_aggregates")=='4' and a.sql('telemetry',"SELECT count(*) FROM timescaledb_information.jobs WHERE proc_name='policy_retention'")=='7')
        a.evidence['telemetry']={'build_id':build,'attempt_id':attempt,'process_samples':len(samples['samples']),'host_samples':len(host['samples']),'database':'TimescaleDB 2.32.0 / PostgreSQL 17','outage_build_succeeded':True,'spool_drained':True}
        a.check('Timescale telemetry end-to-end acceptance',True)
        print('EVIDENCE',a.run/'evidence.json',flush=True)
    finally:
        try:
            if build:record_build(a, build)
        finally:
            try:
                if stopped:a.kubectl('scale','deploy/native-telemetry','--replicas=1')
                a.service_request(39443,'/test/mode',{})
            finally:a.cleanup()


if __name__=='__main__':
    os.umask(0o077)
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--context',required=True)
    parser.add_argument('--namespace',required=True)
    run(parser.parse_args())
