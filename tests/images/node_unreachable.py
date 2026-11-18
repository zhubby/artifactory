"""[INPUT]: 专用单节点 kind、当前构建的 UID 链和已认证会话。
[OUTPUT]: 验证 kubelet 失联后的台账、残留 Pod、并发阻塞与恢复清理。
[POS]: 限定专属 kind；节点恢复后重建已退出的临时转发。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
import subprocess
import time
import uuid


def kind_target(context, node, cluster, nodes, pods, namespace):
    if not cluster or context!='kind-'+cluster or not node.startswith(cluster+'-'):
        raise ValueError('Node container does not belong to the explicitly selected kind')
    if len(nodes)!=1 or nodes[0]['metadata']['name']!=node:
        raise ValueError('Node fault requires a dedicated single-node kind')
    if any(p['metadata']['namespace'] not in [namespace,'kube-system','local-path-storage'] for p in pods):
        raise ValueError('Unrelated workloads found on the dedicated kind node')


class NodeUnreachable:
    def __init__(self,a,node):
        self.a=a;self.node=node;self.stopped=False;self.frozen_pid=None
        self.probe=None;self.pool=None;self.api=a.api
        self.evidence={'fault':'node-unreachable','node':node,'samples':[],'kubelet_restored':False}

    def docker(self,*args,data=None,check=True):
        result=subprocess.run(['docker','exec','-i',self.node,*args],input=data,capture_output=True,timeout=30)
        if check and result.returncode:
            raise RuntimeError('Dedicated kind node operation failed')
        return result

    def validate(self):
        cluster=self.a.command(['docker','inspect','--format','{{index .Config.Labels "io.x-k8s.kind.cluster"}}',self.node]).strip()
        nodes=json.loads(self.a.command(['kubectl','--context',self.a.context,'get','nodes','-o','json']))['items']
        pods=json.loads(self.a.command(['kubectl','--context',self.a.context,'get','pods','-A','-o','json']))['items']
        kind_target(self.a.context,self.node,cluster,nodes,pods,self.a.ns)
        if self.docker('systemctl','is-active','kubelet').stdout.strip()!=b'active':
            raise ValueError('Dedicated kubelet is not active before the fault')

    def request(self,path,body=None,method=None):
        method=method or ('POST' if body is not None else 'GET')
        cookie='; '.join(c.name+'='+c.value for c in self.a.jar)
        headers={'Cookie':cookie,'Origin':self.a.origin,'x-artifactory-csrf':'1','X-Artifactory-Tenant':self.a.tenant_id}
        config=['silent','show-error','fail','max-time = 15','noproxy = "*"','request = '+json.dumps(method),
            'url = '+json.dumps('http://'+self.control_ip+':8080'+path)]
        config+=['header = '+json.dumps(key+': '+value) for key,value in headers.items()]
        if body is not None:
            config+=['header = "Content-Type: application/json"','data = '+json.dumps(json.dumps(body))]
        response=self.docker('curl','--config','-',data=('\n'.join(config)+'\n').encode())
        return json.loads(response.stdout)

    def exercise(self,build,pod,job,build_id):
        self.validate()
        a=self.a
        self.build_name=build['metadata']['name']
        self.pool_name=build['spec']['pool']
        self.pool=json.loads(a.kubectl('get','builderpool',self.pool_name,'-o','json'))['spec']
        a.kubectl('patch','builderpool',self.pool_name,'--type=merge','-p',json.dumps({'spec':{'maxActive':1,'maxPerProject':1}}))
        control=next(p for p in json.loads(a.kubectl('get','pods','-l','app=control-plane','-o','json'))['items']
            if p.get('status',{}).get('phase')=='Running' and not p['metadata'].get('deletionTimestamp'))
        self.control_ip=control['status']['podIP']
        if self.request('/healthz')['status']!='ok':
            raise RuntimeError('Independent kind HTTP path is unavailable')
        status=next(c for c in pod['status']['containerStatuses'] if c['name']=='worker')
        self.container_id=status['containerID'].split('://',1)[1]
        pid=json.loads(self.docker('crictl','inspect',self.container_id).stdout)['info']['pid']
        if not isinstance(pid,int) or pid<=1:
            raise ValueError('Unable to prove Runner host PID')
        self.frozen_pid=pid
        self.docker('kill','-STOP',str(pid))
        self.stopped=True
        self.docker('systemctl','stop','kubelet')
        if self.docker('systemctl','is-active','kubelet',check=False).returncode==0:
            raise RuntimeError('Kubelet remained active')
        probe=subprocess.run(['kubectl','--context',a.context,'get','--raw',
            '/api/v1/nodes/'+self.node+'/proxy/healthz','--request-timeout=5s'],capture_output=True,timeout=10)
        if probe.returncode==0:
            raise RuntimeError('Kubelet proxy remained reachable')
        self.evidence['kubelet_proxy_unreachable']=True
        a.api=self.request
        self.request('/api/v1/builds/'+build_id+'/cancel',{})
        execution=uuid.uuid4()
        self.probe='capacity-probe-'+execution.hex[:12]
        probe_spec=dict(build['spec'],executionId=str(execution),cancelRequested=False,taskSecret=self.probe+'-absent')
        a.apply({'apiVersion':'images.artifactory.dev/v1alpha1','kind':'ImageBuild','metadata':{'name':self.probe},'spec':probe_spec})
        samples=self.evidence['samples']
        started=time.monotonic()
        for index in range(8):
            if index:
                time.sleep(5)
            def pending():
                current=json.loads(a.kubectl('get','imagebuild',self.build_name,'-o','json'))
                jobs=json.loads(a.kubectl('get','jobs','-l','artifactory.dev/image-build='+self.build_name,'-o','json'))['items']
                status=current.get('status',{})
                return current if not jobs and status.get('phase')=='Canceled' and status.get('message','').startswith('CleanupPending:') else None
            current=a.wait(pending,45)
            ledger=current['status']['jobOwnership']
            if ledger['imageBuildUid']!=build['metadata']['uid'] or not any(r['uid']==job['metadata']['uid'] for r in ledger['jobs']):
                raise AssertionError('Job UID ledger was lost after Job disappearance')
            physical=json.loads(a.kubectl('get','pod',pod['metadata']['name'],'-o','json'))
            if physical['metadata']['uid']!=pod['metadata']['uid'] or not physical['metadata'].get('deletionTimestamp'):
                raise AssertionError('Canceled Pod identity was lost during node failure')
            candidate=json.loads(a.kubectl('get','imagebuild',self.probe,'-o','json'))
            admitted=json.loads(a.kubectl('get','jobs,pods','-l','artifactory.dev/image-build='+self.probe,'-o','json'))['items']
            if admitted:
                raise AssertionError('Orphaned live Pod released execution capacity')
            state=self.docker('cat','/proc/'+str(pid)+'/stat').stdout.decode().split(') ',1)[1].split()[0]
            if state!='T':
                raise AssertionError('Runner is no longer physically stopped in the isolated node')
            samples.append({'seconds':round(time.monotonic()-started,3),'phase':current['status']['phase'],
                'cleanup_pending':True,'job_absent':True,'pod_uid':physical['metadata']['uid'],
                'ledger_job_uid':job['metadata']['uid'],'runner_state':state,'probe_phase':candidate.get('status',{}).get('phase'),
                'probe_jobs':0})
        if not any(s['probe_phase']=='Building' for s in samples):
            raise AssertionError('Capacity probe was not reconciled during node failure')
        self.restore()
        def cleaned():
            resources=json.loads(a.kubectl('get','jobs,pods','-l','artifactory.dev/image-build='+self.build_name,'-o','json'))['items']
            status=json.loads(a.kubectl('get','imagebuild',self.build_name,'-o','json')).get('status',{})
            return not resources and status.get('phase')=='Canceled' and not status.get('message')
        a.wait(cleaned,120)
        self.evidence.update(physical_cleanup_confirmed=True,capacity_retained=True)
        return self.evidence

    def restore(self):
        a=self.a
        try:
            if self.probe:
                a.kubectl('delete','imagebuild',self.probe,'--ignore-not-found=true','--wait=false')
                self.probe=None
        finally:
            try:
                if self.frozen_pid is not None:
                    self.docker('kill','-CONT',str(self.frozen_pid),check=False)
                    self.frozen_pid=None
                if self.stopped:
                    self.docker('systemctl','start','kubelet')
                    self.stopped=False
                    a.command(['kubectl','--context',a.context,'wait','node/'+self.node,'--for=condition=Ready','--timeout=120s'])
                    self.evidence['kubelet_restored']=True
            finally:
                a.api=self.api
                if self.pool is not None:
                    a.kubectl('patch','builderpool',self.pool_name,'--type=merge','-p',json.dumps({'spec':{
                        key:self.pool[key] for key in ['maxActive','maxPerProject']}}))
                    self.pool=None
                for process in a.processes:
                    process.terminate()
                for process in a.processes:
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill();process.wait(timeout=5)
                a.processes=[]
                def connected():
                    if not a.processes or any(process.poll() is not None for process in a.processes):
                        for process in a.processes:
                            process.terminate()
                        for process in a.processes:
                            try:
                                process.wait(timeout=5)
                            except subprocess.TimeoutExpired:
                                process.kill();process.wait(timeout=5)
                        a.processes=[]
                        a.forward('service/gateway','38081:8080')
                        a.forward('service/upstream','39443:9443')
                    return a.api('/api/v1/auth/config')
                a.wait(connected,60)
