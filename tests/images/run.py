#!/usr/bin/env python3
"""[INPUT]: 显式 Kubernetes context、隔离 namespace、镜像 tag 和平台。
[OUTPUT]: 验证真实源码 init、BuildKit RUN、OCI 推送与网关摘要拉取。
[POS]: 可恢复的端到端验收入口；只保存白名单证据并回收临时身份。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from registry import registry_graph
from setup import ImageAcceptance, install
from faults import inject
from node_unreachable import NodeUnreachable

IO_KINDS=set('NotFound PermissionDenied ConnectionRefused ConnectionReset HostUnreachable NetworkUnreachable ConnectionAborted NotConnected AddrInUse AddrNotAvailable NetworkDown BrokenPipe AlreadyExists WouldBlock NotADirectory IsADirectory DirectoryNotEmpty ReadOnlyFilesystem FilesystemLoop StaleNetworkFileHandle InvalidInput InvalidData TimedOut WriteZero StorageFull NotSeekable QuotaExceeded FileTooLarge ResourceBusy ExecutableFileBusy Deadlock CrossesDevices TooManyLinks InvalidFilename ArgumentListTooLong Interrupted Unsupported UnexpectedEof OutOfMemory InProgress Other'.split())


def io_diagnostic(message):
    if not isinstance(message,str):
        return None
    if message.strip()=='BuildKit supervisor failed; io_kind=None':
        return 'None'
    matched=re.fullmatch(r'BuildKit supervisor failed; io_kind=Some\(([A-Za-z]+)\)',message.strip())
    return matched[1] if matched and matched[1] in IO_KINDS else None


def raw(a, path, method='GET', headers=None):
    outgoing = {'Origin':a.origin,'x-artifactory-csrf':'1','X-Artifactory-Tenant':a.tenant_id}
    outgoing.update(headers or {})
    request = urllib.request.Request(a.base+path,method=method,headers=outgoing)
    try:
        with a.http.open(request,timeout=30) as response:
            return response.status,response.headers,response.read()
    except urllib.error.HTTPError as error:
        return error.code,error.headers,error.read()


def observe(a, project, evidence):
    pods = json.loads(a.kubectl('get','pods','-l','artifactory.dev/project='+project,'-o','json'))['items']
    for pod in pods:
        platform = pod['metadata'].get('labels',{}).get('artifactory.dev/platform')
        if platform not in ['amd64','arm64']:
            continue
        name = pod['metadata']['name']
        spec,status = pod['spec'],pod.get('status',{})
        assert [c['name'] for c in spec.get('initContainers',[])] == ['source']
        assert [c['name'] for c in spec['containers']] == ['worker','buildkit']
        assert not spec.get('shareProcessNamespace',False)
        assert all('restartPolicy' not in c for c in spec['initContainers'])
        assert all('appArmorProfile' not in c.get('securityContext',{}) for c in spec['containers'])
        assert pod['metadata']['annotations']['container.apparmor.security.beta.kubernetes.io/buildkit'] == 'unconfined'
        mounts = {c['name']:{m['name']:m for m in c.get('volumeMounts',[])} for c in spec['containers']}
        assert mounts['worker']['workspace']['readOnly']
        assert not {'task','workspace','work','source-auth','source-identity'} & set(mounts['buildkit'])
        volumes = {v['name']:v for v in spec['volumes']}
        assert volumes['source-auth']['emptyDir']['medium'] == 'Memory'
        assert 'emptyDir' in volumes['workspace']
        record = evidence['pods'].setdefault(name,{'uid':pod['metadata']['uid'],'platform':'linux/'+platform,'node':spec.get('nodeName'),
            'ordinary_init':True,'separate_containers':True,'readonly_source':True,'ephemeral_source':True,'buildkit_credential_isolation':True})
        record['containers'] = [{'name':c['name'],'image':c['image'],'image_id':c.get('imageID'),
            'exit_code':c.get('state',{}).get('terminated',{}).get('exitCode'),'reason':c.get('state',{}).get('terminated',{}).get('reason')}
            for c in status.get('initContainerStatuses',[])+status.get('containerStatuses',[])]
        buildkit=next((c for c in status.get('containerStatuses',[]) if c['name']=='buildkit'),{})
        termination=buildkit.get('state',{}).get('terminated',{})
        diagnostic=io_diagnostic(termination.get('message'))
        if termination.get('exitCode') and diagnostic is None:
            try:
                lines=a.kubectl('logs',name,'-c','buildkit','--tail=20').splitlines()
                diagnostic=next((value for line in lines if (value:=io_diagnostic(line)) is not None),None)
            except RuntimeError:
                pass
        if diagnostic is not None:
            record['buildkit_io_kind']=diagnostic
        source = next((c for c in status.get('initContainerStatuses',[]) if c['name']=='source'),{})
        record['source_exit_code'] = source.get('state',{}).get('terminated',{}).get('exitCode')
        if 'envelope' not in record:
            try:
                materials = json.loads(a.kubectl('get','secret',volumes['task']['secret']['secretName'],'-o','json'))['data']
                task = json.loads(base64.b64decode(materials['build.json']))
                assert 'source' not in task and 'repositories' not in task
                record['envelope'] = {'source_manifest_id':task['source_manifest_id'],'source_digest':task['source_digest'],
                    'full_manifest_absent':True,'material_bytes':sum(len(base64.b64decode(v)) for v in materials.values())}
            except RuntimeError:
                pass
        if record['source_exit_code']==0 and 'manifest' not in record:
            try:
                manifest = json.loads(a.kubectl('exec',name,'-c','worker','--','cat','/workspace/source-manifest.json'))
                record['manifest'] = {'id':manifest['id'],'digest':manifest['digest']}
            except RuntimeError:
                pass


def save(a, evidence):
    path = a.run/'image-evidence.json'
    path.write_text(json.dumps(evidence,ensure_ascii=False,indent=2)+'\n')
    path.chmod(0o600)


def verify(a, args, platforms):
    evidence = {'context':a.context,'namespace':a.ns,'platforms':platforms,'image_tag':a.image_tag,'pods':{},'result':'running','cleanup_errors':[]}
    token_id = credential_id = build_id = project_id = None
    logged_in = False
    node_fault=NodeUnreachable(a,args.node_container) if args.fault=='node-unreachable' else None
    source_control=None
    def detail():
        value = a.api('/api/v1/builds/'+build_id)
        evidence['build'] = {key:value.get(key) for key in ['id','status','commit_sha','source_manifest_id']}
        evidence['executions'] = [{key:item.get(key) for key in ['id','operation','status','root_digest','destination_repository','tags']}
            for item in value.get('image_executions',[])]
        save(a,evidence)
        return value
    try:
        secret = json.loads(a.kubectl('get','secret','control-plane','-o','json'))['data']
        a.api('/api/v1/auth/login',{'username':base64.b64decode(secret['ARTIFACTORY_BOOTSTRAP_USERNAME']).decode(),
            'password':base64.b64decode(secret['ARTIFACTORY_BOOTSTRAP_PASSWORD']).decode()})
        logged_in=True
        del secret
        profile = next(p for p in a.api('/api/v1/model-profiles') if p['name']=='Operator fixture')
        project = a.api('/api/v1/projects',{'name':'Image lifecycle '+str(int(time.time())),
            'repository_url':'https://upstream:9443/repo.git','model_profile_id':profile['id']})
        project_id=project['id']
        if args.fault=='source-init':
            source_control=json.loads(a.kubectl('get','builderpool','default','-o','json'))['spec']['controlUrl']
            a.kubectl('patch','builderpool','default','--type=merge','-p',json.dumps({'spec':{'controlUrl':'https://upstream:1'}}))
        commit = a.kubectl('exec','deploy/upstream','--','git','-C','/tmp/upstream/repo.git','rev-parse','HEAD').strip()
        evidence.update(project_id=project_id,source_commit=commit)
        a.service_request(39443,'/test/mode',{'build_seconds':args.build_delay})
        build_id=a.api('/api/v1/projects/'+project_id+'/builds',{'ref':commit,'parameters':{},'targets':['image'],
            'goal':'Read Dockerfile and build the immutable source with BuildKit for '+','.join(platforms),
            'target_requirements':[{'os':'linux','arch':p.split('/')[1]} for p in platforms]})['id']
        deadline=time.monotonic()+args.timeout
        while time.monotonic()<deadline:
            value=detail()
            observe(a,project_id,evidence)
            if args.fault and not evidence.get('fault'):
                if args.fault=='source-init':
                    failed=next((p for p in evidence['pods'].values() if isinstance(p.get('source_exit_code'),int) and p['source_exit_code']!=0),None)
                    injected={'fault':'source-init','source_exit_code':failed['source_exit_code'],'prepare_endpoint_fault':True} if failed else None
                else:
                    injected=inject(a,args.fault,build_id,project_id,node_fault,args.node_container)
                if injected:
                    evidence['fault']=injected
            if value['status'] in ['succeeded','failed','canceled','lost','needs_input']:
                break
            time.sleep(1)
        expected='canceled' if args.fault in ['cancel','node-unreachable'] else ('failed' if args.fault in ['runner-kill','buildkit-kill','supervisor-kill','runner-oom','supervisor-oom','source-init','pod-deadline'] else 'succeeded')
        assert value['status']==expected,'unexpected terminal build status'
        if args.fault:
            assert evidence.get('fault'),'fault was not injected'
        assert value['commit_sha']==commit
        if expected!='succeeded':
            assert all(e['status']!='succeeded' for e in value.get('image_executions',[]))
            deadline=time.monotonic()+90
            while time.monotonic()<deadline:
                remaining=a.kubectl('get','pods','-l','artifactory.dev/project='+project_id,'-o','name').splitlines()
                for item in value.get('image_executions',[]):
                    remaining+=a.kubectl('get','secrets','-l','artifactory.dev/execution='+item['id'],'-o','name').splitlines()
                if not remaining:
                    break
                time.sleep(1)
            assert not remaining,'failed execution retained running resources'
            evidence.update(result='passed',task_resources_remaining=remaining)
            print('PASS '+args.fault+' -> '+expected+' and resource cleanup',flush=True)
            return
        execution=next(e for e in value['image_executions'] if e['operation']=='build' and e['status']=='succeeded')
        assert execution['spec']['platforms']==platforms
        assert {p['platform'] for p in evidence['pods'].values()}==set(platforms)
        for pod in evidence['pods'].values():
            assert pod['source_exit_code']==0
            assert pod['manifest']['id']==value['source_manifest_id']
            assert pod['envelope']['source_manifest_id']==pod['manifest']['id']
            assert pod['envelope']['source_digest']==pod['manifest']['digest']
            assert pod['envelope']['material_bytes']<=900*1024
        issued=a.api('/api/v1/tokens',{'name':'Image digest verification','project_ids':[project_id],'scopes':['read'],'days':1})
        token_id=issued['id']
        query=urllib.parse.urlencode({'service':'artifactory-registry','scope':'repository:'+execution['destination_repository']+':pull'})
        basic=base64.b64encode(('_token:'+issued['token']).encode()).decode()
        status,_,payload=raw(a,'/api/v1/registry/token?'+query,headers={'Authorization':'Basic '+basic})
        assert status==200
        authorization={'Authorization':'Bearer '+json.loads(payload)['token'],
            'Accept':'application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json'}
        def internal(path,method='GET',headers=None):
            return raw(a,path,method,dict(authorization,**(headers or {})))
        evidence['internal_registry']=registry_graph(internal,execution['destination_repository'],execution['root_digest'],platforms)
        if args.external_http:
            login={key:base64.b64decode(value).decode() for key,value in json.loads(a.kubectl('get','secret','external-registry-login','-o','json'))['data'].items()}
            credential_id=a.api('/api/v1/projects/'+project_id+'/image-credentials',{'kind':'registry','name':'Isolated external HTTP registry','login':login})['id']
            publication=a.api('/api/v1/image-executions/'+execution['id']+'/publish',{'credential_id':credential_id,'repository':'acceptance/source-init','tags':['verified']})['id']
            deadline=time.monotonic()+180
            while time.monotonic()<deadline:
                published=next(e for e in detail()['image_executions'] if e['id']==publication)
                if published['status'] in ['succeeded','failed','canceled']:
                    break
                time.sleep(1)
            assert published['status']=='succeeded' and published['root_digest']==execution['root_digest']
            a.forward('service/external-registry','38500:5000')
            opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
            external_auth='Basic '+base64.b64encode((login['username']+':'+login['password']).encode()).decode()
            def external(path,method='GET',headers=None):
                outgoing={'Authorization':external_auth,'Accept':authorization['Accept']}
                outgoing.update(headers or {})
                with opener.open(urllib.request.Request('http://127.0.0.1:38500'+path,method=method,headers=outgoing),timeout=30) as response:
                    return response.status,response.headers,response.read()
            a.wait(lambda:external('/v2/')[0]==200,30)
            evidence['external_registry']=registry_graph(external,'acceptance/source-init',execution['root_digest'],platforms)
        deadline=time.monotonic()+90
        remaining=[]
        while time.monotonic()<deadline:
            remaining=[]
            for item in detail()['image_executions']:
                selector='artifactory.dev/image-build=image-'+item['id'].replace('-','')
                remaining+=a.kubectl('get','pods','-l',selector,'-o','name').splitlines()
                remaining+=a.kubectl('get','secrets','-l','artifactory.dev/execution='+item['id'],'-o','name').splitlines()
            if not remaining:
                break
            time.sleep(1)
        assert not remaining,'completed execution retained task resources'
        evidence.update(result='passed',task_resources_remaining=remaining)
        print('PASS source init, real '+','.join(platforms)+' RUN, registry graph and cleanup',flush=True)
    except Exception as error:
        evidence.update(result='failed',failure_class=type(error).__name__)
        if getattr(error,'reason_code',None):
            evidence['failure_code']=error.reason_code
        raise
    finally:
        if node_fault is not None and (node_fault.stopped or node_fault.frozen_pid is not None or node_fault.pool is not None):
            try:
                node_fault.restore()
            except Exception:
                evidence['cleanup_errors'].append('node_restore')
        if node_fault is not None:
            evidence['fault']=node_fault.evidence
        if source_control is not None:
            try:
                a.kubectl('patch','builderpool','default','--type=merge','-p',json.dumps({'spec':{'controlUrl':source_control}}))
            except Exception:
                evidence['cleanup_errors'].append('source_control_restore')
        cleanup=[]
        if build_id and evidence['result']!='passed' and evidence.get('build',{}).get('status') not in ['succeeded','failed','canceled','lost']:
            cleanup.append(('cancel','/api/v1/builds/'+build_id+'/cancel',{},'POST'))
        if token_id:
            cleanup.append(('token','/api/v1/tokens/'+token_id,None,'DELETE'))
        if credential_id:
            cleanup.append(('credential','/api/v1/projects/'+project_id+'/image-credentials/'+credential_id,None,'DELETE'))
        if logged_in:
            cleanup.append(('logout','/api/v1/auth/logout',{},'POST'))
        for name,path,body,method in cleanup:
            try:
                a.api(path,body,method)
            except Exception:
                evidence['cleanup_errors'].append(name)
        save(a,evidence)
        if evidence['cleanup_errors']:
            raise RuntimeError('temporary API identity cleanup failed')


def main():
    parser=argparse.ArgumentParser(description='Run real image builds in a new isolated namespace')
    parser.add_argument('--context',required=True)
    parser.add_argument('--namespace',required=True)
    parser.add_argument('--tag',required=True)
    parser.add_argument('--registry',default='localhost:5001/artifactory')
    parser.add_argument('--resume',action='store_true')
    parser.add_argument('--mode',choices=['amd64','arm64','multiarch'],default='amd64')
    parser.add_argument('--allow-emulation',action='store_true')
    parser.add_argument('--emulation-selector',action='append',default=[],metavar='KEY=VALUE')
    parser.add_argument('--external-http',action='store_true',help='Explicitly enable a separate HTTP registry fixture')
    parser.add_argument('--fault',choices=['runner-kill','buildkit-kill','supervisor-kill','runner-oom','supervisor-oom','source-init','pod-deadline','cancel','operator-restart','node-unreachable'])
    parser.add_argument('--node-container',help='Explicit dedicated single-node kind container for runtime/node faults')
    parser.add_argument('--timeout',type=int,default=900)
    parser.add_argument('--build-delay',type=int,choices=range(0,31),default=5)
    args=parser.parse_args()
    if args.fault:
        args.build_delay=30
    if args.fault=='node-unreachable' and (not args.node_container or args.mode=='multiarch'):
        parser.error('node-unreachable requires --node-container and a single native platform')
    if args.fault in ['runner-kill','buildkit-kill','supervisor-kill','runner-oom','supervisor-oom'] and not args.node_container:
        parser.error('container faults require --node-container for actual runtime exit verification')
    platforms=['linux/amd64','linux/arm64'] if args.mode=='multiarch' else ['linux/'+args.mode]
    selector={}
    for value in args.emulation_selector:
        if '=' not in value:
            parser.error('--emulation-selector must use KEY=VALUE')
        key,value=value.split('=',1)
        if not key or not value:
            parser.error('empty emulation selector')
        selector[key]=value
    if args.allow_emulation and not selector:
        parser.error('--allow-emulation requires an explicitly verified node selector')
    os.umask(0o077)
    a=ImageAcceptance(args.context,args.namespace,args.tag,args.resume,args.registry)
    try:
        if args.resume:
            a.resume()
        else:
            a.setup()
        install(a,platforms,args.allow_emulation,selector,args.external_http)
        verify(a,args,platforms)
        print('Evidence '+str(a.run/'image-evidence.json'),flush=True)
    finally:
        a.close()


if __name__=='__main__':
    try:
        main()
    except Exception as error:
        print('FAIL '+type(error).__name__+'; inspect sanitized image-evidence.json',file=sys.stderr)
        sys.exit(1)
