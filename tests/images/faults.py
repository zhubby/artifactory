"""[INPUT]: 隔离验收客户端、专用 kind 与当前 Pod/CR UID 链。
[OUTPUT]: 注入实际容器/daemon 退出、取消与 Operator 重启。
[POS]: 以 UID 和测试 finalizer 保存终态；以 CRI 和内核记录证明 OOM。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
from pathlib import PurePosixPath
import re
import time
import uuid
from node_unreachable import NodeUnreachable


class FaultFailure(ValueError):
    def __init__(self,code):
        self.reason_code=code
        super().__init__(code)


def _deadline_patch(a, name, uid, finalizer, install):
    for attempt in range(6):
        raw=a.kubectl('get','pod',name,'--ignore-not-found=true','-o','json')
        if not raw.strip():
            if not install:
                return None
            raise FaultFailure('deadline-pod-disappeared-before-proof')
        current=json.loads(raw)
        metadata=current['metadata']
        if metadata['uid']!=uid:
            raise FaultFailure('deadline-pod-uid-changed')
        finalizers=metadata.get('finalizers',[])
        if install:
            if finalizer in finalizers and current.get('spec',{}).get('activeDeadlineSeconds')==1:
                return current
            if metadata.get('deletionTimestamp'):
                raise FaultFailure('deadline-pod-already-deleting')
            finalizers=[*finalizers,finalizer] if finalizer not in finalizers else finalizers
        else:
            if finalizer not in finalizers:
                return current
            finalizers=[value for value in finalizers if value!=finalizer]
        patch={'metadata':{'uid':uid,'resourceVersion':metadata['resourceVersion'],'finalizers':finalizers}}
        if install:
            patch['spec']={'activeDeadlineSeconds':1}
        try:
            return json.loads(a.kubectl('patch','pod',name,'--type=merge','-p',json.dumps(patch),'-o','json'))
        except RuntimeError:
            if attempt==5:
                raise
            # Kubernetes 状态更新和 finalizer 写入均推进版本；重读后保留其他写入。
            time.sleep(0.05)


def pod_deadline(a, pod):
    name=pod['metadata']['name'];uid=pod['metadata']['uid']
    finalizer='tests.artifactory.dev/deadline-'+uuid.uuid4().hex
    try:
        # 同一 CAS 安装保护和 deadline，防止 Operator 在观察终态前回收 Pod。
        _deadline_patch(a,name,uid,finalizer,True)
        deadline=time.monotonic()+30
        while time.monotonic()<deadline:
            raw=a.kubectl('get','pod',name,'--ignore-not-found=true','-o','json')
            if not raw.strip():
                raise FaultFailure('deadline-pod-disappeared-before-proof')
            current=json.loads(raw)
            if current['metadata']['uid']!=uid:
                raise FaultFailure('deadline-pod-uid-changed')
            if current.get('status',{}).get('phase')=='Failed' and current.get('status',{}).get('reason')=='DeadlineExceeded':
                return {'fault':'pod-deadline','pod_uid':uid,'deadline_seconds':1,
                    'observed_reason':'DeadlineExceeded','condition_observed':True}
            time.sleep(0.2)
        raise TimeoutError('Kubernetes Pod deadline failure was not observed')
    finally:
        # 即使注入或观察失败，也仅撤下本次测试的保护，不改其他 finalizer。
        _deadline_patch(a,name,uid,finalizer,False)


def container_identity(node,a,pod,name,container_id):
    value=json.loads(node.docker('crictl','inspect',container_id).stdout)
    status=value['status']
    labels=status.get('labels',{})
    if (status['id']!=container_id or status['metadata']['name']!=name
        or labels.get('io.kubernetes.pod.uid')!=pod['metadata']['uid']
        or labels.get('io.kubernetes.pod.namespace')!=a.ns
        or labels.get('io.kubernetes.pod.name')!=pod['metadata']['name']):
        raise ValueError('CRI container identity differs from the current Pod')
    sandbox=json.loads(node.docker('crictl','inspectp',value['info']['sandboxID']).stdout)['status']['metadata']
    if (sandbox['uid']!=pod['metadata']['uid'] or sandbox['namespace']!=a.ns or sandbox['name']!=pod['metadata']['name']):
        raise ValueError('CRI sandbox identity differs from the current Pod')
    return status


def container_cgroup(relative,pod_uid,container_id):
    path=PurePosixPath(relative)
    if not path.is_absolute() or '..' in path.parts:
        raise FaultFailure('oom-cgroup-pod-container-identity-unproven')
    allowed=set()
    for qos in ['', 'burstable', 'besteffort']:
        cgroupfs='/kubepods'+('/'+qos if qos else '')
        allowed.add(cgroupfs+'/pod'+pod_uid+'/'+container_id)
        for root,base in [('/kubepods.slice','kubepods'),
                          ('/kubelet.slice/kubelet-kubepods.slice','kubelet-kubepods')]:
            prefix=root+('/'+base+'-'+qos+'.slice' if qos else '')
            pod_slice=base+('-'+qos if qos else '')+'-pod'+pod_uid.replace('-','_')+'.slice'
            allowed.add(prefix+'/'+pod_slice+'/cri-containerd-'+container_id+'.scope')
    if relative not in allowed:
        raise FaultFailure('oom-cgroup-pod-container-identity-unproven')
    return '/sys/fs/cgroup'+relative


def force_oom(node,pod,container_id):
    if node.docker('stat','-fc','%T','/sys/fs/cgroup').stdout.strip()!=b'cgroup2fs':
        raise FaultFailure('oom-requires-cgroup-v2')
    value=json.loads(node.docker('crictl','inspect',container_id).stdout)
    pid=value['info']['pid']
    if not isinstance(pid,int) or pid<=1:
        raise FaultFailure('oom-container-pid-unproven')
    lines=node.docker('cat','/proc/'+str(pid)+'/cgroup').stdout.decode().splitlines()
    if len(lines)!=1 or not lines[0].startswith('0::/'):
        raise FaultFailure('oom-requires-one-unified-cgroup')
    relative=lines[0].removeprefix('0::')
    target=container_cgroup(relative,pod['metadata']['uid'],container_id)
    try:
        original=node.docker('cat',target+'/memory.oom.group').stdout.strip()
        node.docker('tee',target+'/memory.oom.group',data=b'1\n')
    except Exception:
        raise FaultFailure('oom-cgroup-group-write-unavailable') from None
    try:
        # 对已证实的单个容器施加真实 OOM，整组终止才会包含 PID 1。
        node.docker('tee',target+'/memory.max',data=b'1\n')
    except Exception:
        node.docker('tee',target+'/memory.oom.group',data=original+b'\n',check=False)
        raise FaultFailure('oom-cgroup-limit-write-unavailable') from None
    return relative


def container_oom_observed(output,cgroup_relative):
    constrained=False
    group_killed=False
    for line in output.decode(errors='replace').splitlines():
        if 'oom-kill:constraint=CONSTRAINT_MEMCG,' in line:
            match=re.search(r'(?:^|,)oom_memcg=([^,\s]+)',line)
            constrained |= bool(match and match[1].endswith(cgroup_relative))
        match=re.search(r'Tasks in (\S+) are going to be killed due to memory\.oom\.group set',line)
        group_killed |= bool(match and match[1].endswith(cgroup_relative))
    return constrained and group_killed


def terminate(a,node_name,pod,container,daemon=False,oom=False):
    if not node_name or pod['spec']['nodeName']!=node_name:
        raise ValueError('Container fault requires the explicitly selected dedicated kind node')
    node=NodeUnreachable(a,node_name)
    node.validate()
    runtime=next(c for c in pod['status']['containerStatuses'] if c['name']==container)['containerID']
    if not runtime.startswith('containerd://'):
        raise ValueError('Container fault requires the dedicated kind containerd runtime')
    container_id=runtime.removeprefix('containerd://')
    if not re.fullmatch(r'[0-9a-f]{64}',container_id):
        raise ValueError('Container runtime ID must be a full immutable ID')
    before=container_identity(node,a,pod,container,container_id)
    if before['state']!='CONTAINER_RUNNING':
        return None
    daemon_pid=None
    if daemon or (oom and container=='buildkit'):
        try:
            a.kubectl('exec',pod['metadata']['name'],'-c','buildkit','--','buildctl','--addr','unix:///run/buildkit/buildkitd.sock','debug','workers')
        except RuntimeError:
            return None
    if daemon:
        script='set -eu; for entry in /proc/[0-9]*; do IFS= read -r comm < "$entry/comm" || continue; if [ "$comm" = buildkitd ]; then pid=${entry##*/}; test "$pid" -gt 1; kill -KILL "$pid"; printf "%s\\n" "$pid"; exit 0; fi; done; exit 1'
        daemon_pid=int(a.kubectl('exec',pod['metadata']['name'],'-c','buildkit','--','sh','-c',script).strip())
        if daemon_pid<=1:
            raise AssertionError('BuildKit daemon PID must differ from namespace init')
    elif oom:
        relative=force_oom(node,pod,container_id)
        return _confirmed_exit(node,a,pod,container,container_id,daemon_pid,relative)
    else:
        node.docker('crictl','stop','--timeout=0',container_id)
    return _confirmed_exit(node,a,pod,container,container_id,daemon_pid,None)


def _confirmed_exit(node,a,pod,container,container_id,daemon_pid,oom_path):
    deadline=time.monotonic()+20
    while time.monotonic()<deadline:
        status=container_identity(node,a,pod,container,container_id)
        if status['state']=='CONTAINER_EXITED':
            code=status.get('exitCode')
            if not isinstance(code,int) or code==0:
                raise AssertionError('Injected container did not actually exit nonzero')
            observed=False
            if oom_path is not None:
                # 容器被迅速清理时，CRI 会丢失 OOM 原因。内核同组记录才是注入证明。
                observed=container_oom_observed(node.docker('dmesg','--raw').stdout,oom_path)
                if code!=137 or not observed:
                    raise FaultFailure('oom-runtime-exit-or-kernel-event-unproven')
            return {'container':container,'container_id':container_id,'exit_code':code,
                'runtime_exit_observed':True,'sandbox_uid_verified':True,'daemon_pid':daemon_pid,
                'kernel_cgroup_oom_observed':observed,
                'reason':status.get('reason') if status.get('reason') in ['OOMKilled','Error','Completed'] else 'Other',
                'method':'cgroup-v2-group-oom' if oom_path is not None else ('daemon-sigkill' if daemon_pid is not None else 'runtime-stop-timeout-zero')}
        time.sleep(0.2)
    raise TimeoutError('Container runtime did not confirm an actual exit')


def inject(a, name, build_id, project_id, node_fault=None, node_name=None):
    pods=json.loads(a.kubectl('get','pods','-l','artifactory.dev/project='+project_id,'-o','json'))['items']
    builds=json.loads(a.kubectl('get','imagebuilds','-o','json'))['items']
    for pod in pods:
        labels=pod['metadata'].get('labels',{})
        if labels.get('artifactory.dev/platform') not in ['amd64','arm64']:
            continue
        build=next((b for b in builds if b['spec']['projectId']==project_id and any(
            o.get('apiVersion')=='images.artifactory.dev/v1alpha1' and o.get('kind')=='ImageBuild' and o.get('controller') is True
            and o.get('uid')==b['metadata']['uid'] and o.get('name')==b['metadata']['name']
            for o in pod['metadata'].get('ownerReferences',[]))),None)
        if build is None:
            continue
        ledger=build.get('status',{}).get('podOwnership',{})
        if ledger.get('imageBuildUid')!=build['metadata']['uid'] or not any(
            record['uid']==pod['metadata']['uid'] and record['name']==pod['metadata']['name']
            for record in ledger.get('pods',[])):
            continue
        statuses={c['name']:c.get('state',{}) for c in pod.get('status',{}).get('containerStatuses',[])}
        if not all('running' in statuses.get(c,{}) for c in ['worker','buildkit']):
            continue
        if name=='node-unreachable':
            if node_fault is None or pod['spec']['nodeName']!=node_fault.node:
                raise ValueError('Node fault does not match the current build node')
            return node_fault.exercise(build,pod,build_id)
        if name == 'cancel':
            a.api('/api/v1/builds/'+build_id+'/cancel',{})
            return {'fault':name,'target':'current-build'}
        if name=='operator-restart':
            a.kubectl('rollout','restart','deploy/artifactory-image-operator')
            return {'fault':name,'target':'artifactory-image-operator','uid_ledger_saved':True}
        if name=='pod-deadline':
            return pod_deadline(a,pod)
        container={'runner-kill':'worker','buildkit-kill':'buildkit','supervisor-kill':'buildkit','runner-oom':'worker','supervisor-oom':'buildkit'}[name]
        observed=terminate(a,node_name,pod,container,name=='buildkit-kill',name in ['runner-oom','supervisor-oom'])
        if observed is None:
            continue
        return dict(observed,fault=name,pod=pod['metadata']['name'],pod_uid=pod['metadata']['uid'])
    return None
