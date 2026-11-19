"""[INPUT]: 合成 OCI Blob、隔离身份与模拟 Kubernetes 读接口。
[OUTPUT]: 验证 namespace、真实 RUN、故障 UID、CAS 终态保护与 OOM 事件。
[POS]: 端到端脚本的离线回归，不连接任何集群。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0,str(Path(__file__).resolve().parent))
from faults import _confirmed_exit, container_cgroup, container_oom_observed, force_oom, inject, pod_deadline, terminate
from registry import registry_graph
from setup import ImageAcceptance, install, safe_namespace
from node_unreachable import NodeUnreachable, kind_target
from run import io_diagnostic


def graph(marker='executed-linux-amd64\n', machine=62, corrupt_layer=False):
    blobs={}
    def put(data,kind):
        digest='sha256:'+hashlib.sha256(data).hexdigest()
        blobs['/'+kind+'/'+digest]=data
        return {'digest':digest,'size':len(data)}
    elf=bytearray(64)
    elf[:4]=b'\x7fELF'; elf[5]=1; elf[18:20]=machine.to_bytes(2,'little')
    content=io.BytesIO()
    with tarfile.open(fileobj=content,mode='w:gz') as archive:
        for name,value in {'arch-check':bytes(elf),'arch-result':marker.encode(),'hello.txt':b'image-source-init-ok\n'}.items():
            entry=tarfile.TarInfo(name);entry.size=len(value)
            archive.addfile(entry,io.BytesIO(value))
    layer=put(content.getvalue(),'blobs')
    config=put(json.dumps({'architecture':'amd64','os':'linux'}).encode(),'blobs')
    manifest=put(json.dumps({'config':config,'layers':[layer]}).encode(),'manifests')
    root=put(json.dumps({'mediaType':'application/vnd.oci.image.index.v1+json','manifests':[
        dict(manifest,platform={'architecture':'amd64','os':'linux'})]}).encode(),'manifests')
    if corrupt_layer:
        blobs['/blobs/'+layer['digest']]+=b'corruption'
    def read(path,method='GET',headers=None):
        data=blobs[path.removeprefix('/v2/repository')]
        outgoing={'Content-Length':str(len(data))}
        if method=='HEAD':
            return 200,outgoing,b''
        if headers and 'Range' in headers:
            return 206,outgoing,data[:16]
        return 200,outgoing,data
    return read,root['digest']


class DeadlineClient:
    """终态立即触发 Operator 删除；只有测试保护能让观察者读取终态。"""
    def __init__(self, *, terminal=True, conflicts=None, foreign=False, disappear=False, replace=False):
        self.pod={'metadata':{'name':'pod','uid':'pod-uid','resourceVersion':'1','finalizers':['operator']},
            'spec':{'activeDeadlineSeconds':900},'status':{'phase':'Running'}}
        self.terminal=terminal;self.conflicts=dict(conflicts or {});self.foreign=foreign
        self.disappear=disappear;self.replace=replace;self.patches=[];self.protected=False

    def kubectl(self,*args):
        if args[0]=='get':
            return json.dumps(self.pod) if self.pod else ''
        patch=json.loads(args[args.index('-p')+1]);self.patches.append(patch)
        assert self.pod is not None
        assert patch['metadata']['uid']==self.pod['metadata']['uid']
        assert patch['metadata']['resourceVersion']==self.pod['metadata']['resourceVersion']
        operation='install' if 'spec' in patch else 'remove'
        if self.conflicts.get(operation,0):
            self.conflicts[operation]-=1
            if self.foreign:
                self.pod['metadata']['finalizers'].append('external/'+operation)
            self.pod['metadata']['resourceVersion']=str(int(self.pod['metadata']['resourceVersion'])+1)
            raise RuntimeError('synthetic CAS conflict')
        self.pod['metadata']['finalizers']=patch['metadata']['finalizers']
        self.pod['metadata']['resourceVersion']=str(int(self.pod['metadata']['resourceVersion'])+1)
        if operation=='install':
            self.pod['spec'].update(patch['spec'])
            if self.terminal:
                self.pod['status']={'phase':'Failed','reason':'DeadlineExceeded'}
                self.pod['metadata']['deletionTimestamp']='2026-11-19T00:00:00Z'
                self.pod['metadata']['finalizers'].remove('operator')
                self.protected=any(value.startswith('tests.artifactory.dev/deadline-')
                    for value in self.pod['metadata']['finalizers'])
        response=json.dumps(self.pod)
        if operation=='install' and self.replace:
            self.pod={'metadata':{'name':'pod','uid':'replacement','resourceVersion':'1','finalizers':['foreign']}}
        elif (operation=='install' and self.disappear) or (
            self.pod['metadata'].get('deletionTimestamp') and not self.pod['metadata']['finalizers']
        ):
            self.pod=None
        return response


class HarnessTests(unittest.TestCase):
    def test_deadline_retries_cas_conflicts_and_preserves_terminal_before_fast_gc(self):
        client=DeadlineClient(conflicts={'install':2,'remove':2})
        original=json.loads(json.dumps(client.pod))
        with patch('faults.time.sleep'):
            result=pod_deadline(client,original)
        self.assertEqual(result['observed_reason'],'DeadlineExceeded')
        self.assertEqual(result['pod_uid'],'pod-uid')
        self.assertTrue(client.protected)
        self.assertIsNone(client.pod)
        self.assertEqual(len(client.patches),6)
        self.assertTrue(all(value['metadata']['uid']=='pod-uid' for value in client.patches))

    def test_deadline_cleanup_preserves_finalizers_added_during_cas_conflicts(self):
        client=DeadlineClient(conflicts={'install':1,'remove':1},foreign=True)
        with patch('faults.time.sleep'):
            pod_deadline(client,json.loads(json.dumps(client.pod)))
        self.assertEqual(client.pod['metadata']['finalizers'],['external/install','external/remove'])

    def test_deadline_missing_pod_never_counts_as_terminal_evidence(self):
        client=DeadlineClient(disappear=True)
        with self.assertRaisesRegex(ValueError,'deadline-pod-disappeared-before-proof'):
            pod_deadline(client,json.loads(json.dumps(client.pod)))
        self.assertEqual(len(client.patches),1)

    def test_deadline_uid_replacement_is_never_modified_during_cleanup(self):
        client=DeadlineClient(replace=True)
        with self.assertRaisesRegex(ValueError,'deadline-pod-uid-changed'):
            pod_deadline(client,json.loads(json.dumps(client.pod)))
        self.assertEqual(client.pod['metadata']['finalizers'],['foreign'])
        self.assertEqual(len(client.patches),1)

    def test_deadline_timeout_still_removes_only_its_test_finalizer(self):
        client=DeadlineClient(terminal=False)
        with patch('faults.time.monotonic',side_effect=[0,0,31]),patch('faults.time.sleep'):
            with self.assertRaises(TimeoutError):
                pod_deadline(client,json.loads(json.dumps(client.pod)))
        self.assertEqual(client.pod['metadata']['finalizers'],['operator'])

    def test_oom_exit_requires_matching_kernel_event_even_when_cri_reason_lags(self):
        identifier='b'*64
        path='/kubepods/pod1234/'+identifier
        output=('oom-kill:constraint=CONSTRAINT_MEMCG,oom_memcg=/host'+path+',task=supervisor\n'
            'Tasks in /host'+path+' are going to be killed due to memory.oom.group set\n').encode()
        self.assertTrue(container_oom_observed(output,path))
        self.assertFalse(container_oom_observed(output,path+'-suffix'))
        self.assertFalse(container_oom_observed(output,path.replace(identifier,'a'*64)))
        self.assertFalse(container_oom_observed(output.splitlines()[0],path))
        self.assertFalse(container_oom_observed(output.splitlines()[1],path))
        node=Mock()
        node.docker.return_value.stdout=output
        with patch('faults.container_identity',return_value={'state':'CONTAINER_EXITED','exitCode':137,'reason':'Error'}):
            observed=_confirmed_exit(node,None,{},'buildkit',identifier,None,path)
            self.assertEqual(observed['reason'],'Error')
            self.assertTrue(observed['kernel_cgroup_oom_observed'])
            node.docker.return_value.stdout=b''
            with self.assertRaises(ValueError):
                _confirmed_exit(node,None,{},'buildkit',identifier,None,path)

    def test_supervisor_diagnostics_preserve_only_known_io_enum_values(self):
        self.assertEqual(io_diagnostic('BuildKit supervisor failed; io_kind=None'),'None')
        self.assertEqual(io_diagnostic('BuildKit supervisor failed; io_kind=Some(PermissionDenied)\n'),'PermissionDenied')
        for message in [None,'BuildKit supervisor failed; io_kind=Some(secret-value)',
            'BuildKit supervisor failed; io_kind=Some(PermissionDenied)\nsynthetic-private-diagnostic',
            'raw daemon diagnostic with synthetic registry credential']:
            self.assertIsNone(io_diagnostic(message))

    def test_session_revocation_precedes_forward_shutdown_even_on_logout_failure(self):
        for fail in [False,True]:
            events=[]
            class Jar(list):
                def clear(self): events.append('clear-cookie');super().clear()
            class Process:
                def terminate(self): events.append('stop-forward')
                def wait(self,timeout): events.append('wait-forward')
            client=ImageAcceptance.__new__(ImageAcceptance)
            client.jar=Jar(['synthetic-session']);client.processes=[Process()]
            def api(path,body):
                self.assertEqual(path,'/api/v1/auth/logout');events.append('logout')
                if fail: raise RuntimeError('synthetic-connection-failure')
            client.api=api
            if fail:
                with self.assertRaises(RuntimeError): client.close()
            else:
                client.close()
            self.assertEqual(events,['logout','stop-forward','wait-forward','clear-cookie'])
            self.assertFalse(client.processes)
            self.assertFalse(client.jar)

    def test_image_storage_waits_for_backend_port_before_starting_a_forward(self):
        class ReadyChecked(Exception): pass
        with tempfile.TemporaryDirectory() as directory:
            client=ImageAcceptance.__new__(ImageAcceptance)
            client.run=Path(directory);client.image_tag='fixture';client.registry='example.invalid/fixture';client.pull_policy='IfNotPresent'
            private=client.run/'image-private';private.mkdir()
            (private/'credentials.json').write_text(json.dumps({key:'synthetic-fixture' for key in ['admin','storage','http','external']}))
            for name in ['registry.key','registry.pem']: (private/name).write_text('synthetic-fixture')
            applied=[]
            client.apply=lambda *items:applied.extend(items)
            def kubectl(*args):
                if args[:3]==('get','deploy','upstream'):
                    return json.dumps({'kind':'Deployment','metadata':{'name':'upstream'},'spec':{'template':{'spec':{'containers':[{'name':'upstream'}]}}}})
                self.assertEqual(args[:3],('rollout','status','deploy/image-s3'))
                storage=next(item for item in applied if item['kind']=='Deployment' and item['metadata']['name']=='image-s3')
                probe=storage['spec']['template']['spec']['containers'][0].get('readinessProbe')
                self.assertEqual(probe,{'tcpSocket':{'port':8333},'periodSeconds':1,'failureThreshold':90})
                raise ReadyChecked()
            client.kubectl=kubectl
            with self.assertRaises(ReadyChecked): install(client,['linux/arm64'])

    def test_node_fault_requires_matching_docker_label_single_node_and_dedicated_workloads(self):
        node={'metadata':{'name':'isolated-control-plane'}}
        pod={'metadata':{'namespace':'artifact-image-isolated'}}
        kind_target('kind-isolated','isolated-control-plane','isolated',[node],[pod],'artifact-image-isolated')
        for context,container,cluster,nodes,pods in [
            ('production','isolated-control-plane','isolated',[node],[pod]),
            ('kind-other','isolated-control-plane','isolated',[node],[pod]),
            ('kind-isolated','unrelated-control-plane','isolated',[node],[pod]),
            ('kind-isolated','isolated-control-plane','isolated',[node,node],[pod]),
            ('kind-isolated','isolated-control-plane','isolated',[node],[{'metadata':{'namespace':'unrelated'}}]),
        ]:
            with self.subTest(context=context,container=container),self.assertRaises(ValueError):
                kind_target(context,container,cluster,nodes,pods,'artifact-image-isolated')

    def test_node_restore_starts_kubelet_even_if_probe_cleanup_fails(self):
        class Client:
            def __init__(self):
                self.context='kind-isolated';self.processes=[];self.calls=[]
            def api(self,*args): return {'ok':True}
            def kubectl(self,*args): raise RuntimeError('injected probe delete failure')
            def command(self,*args): self.calls.append(args)
            def forward(self,*args): self.calls.append(args)
            def wait(self,callback,*args): return callback()
        client=Client()
        fault=NodeUnreachable(client,'isolated-control-plane')
        docker=[]
        fault.docker=lambda *args,**kwargs:docker.append(args)
        fault.stopped=True;fault.probe='capacity-probe-owned'
        with self.assertRaises(RuntimeError): fault.restore()
        self.assertIn(('systemctl','start','kubelet'),docker)
        self.assertFalse(fault.stopped)
        self.assertIn(('service/gateway','38081:8080'),client.calls)

    def test_namespace_must_be_an_explicit_image_acceptance_namespace(self):
        self.assertEqual(safe_namespace('artifact-image-k128-arm64'),'artifact-image-k128-arm64')
        for namespace in ['artifactory','default','kube-system','artifact-image-','artifact-image-a/../../x','artifact-image-'+('x'*64)]:
            with self.subTest(namespace=namespace),self.assertRaises(ValueError):
                safe_namespace(namespace)

    def test_node_restore_retries_forwards_that_exit_before_kubelet_has_its_pods(self):
        class Process:
            def __init__(self,exited): self.exited=exited;self.terminated=False
            def poll(self): return 1 if self.exited else None
            def terminate(self): self.terminated=True
            def wait(self,timeout): return 1
        class Client:
            def __init__(self): self.processes=[];self.created=[]
            def forward(self,*args):
                process=Process(len(self.created)<2)
                self.processes.append(process);self.created.append(process)
            def api(self,*args):
                if any(process.poll() is not None for process in self.processes):
                    raise RuntimeError('kubelet has not restored the Pod cache')
                return {'ok':True}
            def wait(self,callback,*args):
                for _ in range(3):
                    try:
                        if callback(): return True
                    except RuntimeError:
                        pass
                raise TimeoutError('forward did not reconnect')
        client=Client()
        NodeUnreachable(client,'isolated-control-plane').restore()
        self.assertEqual(len(client.created),4)
        self.assertTrue(all(process.terminated for process in client.created[:2]))
        self.assertTrue(all(process.poll() is None for process in client.processes))

    def test_registry_graph_verifies_real_run_and_every_blob_digest(self):
        read,digest=graph()
        result=registry_graph(read,'repository',digest,['linux/amd64'])
        self.assertEqual(result['verified'][0]['run_marker'],'executed-linux-amd64')
        self.assertTrue(result['verified'][0]['blobs'][0]['range'])
        for args in [{'marker':'executed-linux-arm64\n'},{'machine':183},{'corrupt_layer':True}]:
            with self.subTest(args=args),self.assertRaises(AssertionError):
                read,digest=graph(**args)
                registry_graph(read,'repository',digest,['linux/amd64'])

    def test_fault_rejects_a_foreign_cr_uid_despite_matching_name_and_labels(self):
        class Client:
            def __init__(self,owner):
                self.owner=owner;self.mutations=[]
            def kubectl(self,*args):
                if args[0]!='get':
                    self.mutations.append(args)
                    return ''
                kind=args[1]
                if kind=='pods':
                    items=[{'metadata':{'name':'pod','uid':'pod-uid','labels':{'artifactory.dev/platform':'amd64'},
                        'ownerReferences':[{'apiVersion':'images.artifactory.dev/v1alpha1','kind':'ImageBuild','controller':True,'uid':self.owner,'name':'build'}]},
                        'status':{'containerStatuses':[{'name':name,'state':{'running':{}}} for name in ['worker','buildkit']]}}]
                else:
                    items=[{'metadata':{'name':'build','uid':'build-uid'},'spec':{'projectId':'project'},
                        'status':{'podOwnership':{'imageBuildUid':'build-uid','pods':[{'name':'pod','uid':'pod-uid','stopped':False}]}}}]
                return json.dumps({'items':items})
        foreign=Client('foreign-cr')
        self.assertIsNone(inject(foreign,'runner-kill','build','project'))
        self.assertFalse(foreign.mutations)
        restart=Client('build-uid')
        self.assertTrue(inject(restart,'operator-restart','build','project')['uid_ledger_saved'])

    def test_runtime_fault_proves_sandbox_uid_and_actual_nonzero_exit(self):
        class Client:
            ns='artifact-image-isolated'
        identifier='a'*64
        relative='/kubepods/pod1234/'+identifier
        kernel=('oom-kill:constraint=CONSTRAINT_MEMCG,oom_memcg=/host'+relative+',task=supervisor\n'
            'Tasks in /host'+relative+' are going to be killed due to memory.oom.group set\n').encode()
        pod={'metadata':{'name':'pod','uid':'pod-uid'},'spec':{'nodeName':'isolated-control-plane'},
            'status':{'containerStatuses':[{'name':'worker','containerID':'containerd://'+identifier}]}}
        class Result:
            def __init__(self,value): self.stdout=json.dumps(value).encode()
        class Node:
            last=None
            sandbox_uid='pod-uid'
            exit_code=137
            reason='Error'
            oom_output=kernel
            def __init__(self,*args): self.stopped=False;self.node=args[1];Node.last=self
            def validate(self): pass
            def docker(self,*args):
                if args[0]=='dmesg': return Mock(stdout=self.oom_output)
                if args[1]=='stop':
                    self.stopped=True
                    return Result({})
                if args[1]=='inspectp':
                    return Result({'status':{'metadata':{'uid':self.sandbox_uid,'namespace':'artifact-image-isolated','name':'pod'}}})
                return Result({'status':{'id':identifier,'metadata':{'name':'worker'},'state':'CONTAINER_EXITED' if self.stopped else 'CONTAINER_RUNNING',
                    'exitCode':self.exit_code,'reason':self.reason,'labels':{'io.kubernetes.pod.uid':'pod-uid','io.kubernetes.pod.namespace':'artifact-image-isolated','io.kubernetes.pod.name':'pod'}},
                    'info':{'sandboxID':'sandbox-id'}})
        with patch('faults.NodeUnreachable',Node):
            result=terminate(Client(),'isolated-control-plane',pod,'worker')
            self.assertEqual(result['exit_code'],137)
            self.assertTrue(result['runtime_exit_observed'])
            Node.sandbox_uid='foreign-pod'
            with self.assertRaises(ValueError): terminate(Client(),'isolated-control-plane',pod,'worker')
            self.assertFalse(Node.last.stopped)
            Node.sandbox_uid='pod-uid';Node.exit_code=0
            with self.assertRaises(AssertionError): terminate(Client(),'isolated-control-plane',pod,'worker')
            Node.exit_code=137;Node.reason='OOMKilled'
            with patch('faults.force_oom',side_effect=lambda node,*args:(setattr(node,'stopped',True),relative)[1]):
                result=terminate(Client(),'isolated-control-plane',pod,'worker',oom=True)
                self.assertEqual((result['reason'],result['exit_code']),('OOMKilled',137))
                Node.reason='Error'
                result=terminate(Client(),'isolated-control-plane',pod,'worker',oom=True)
                self.assertTrue(result['kernel_cgroup_oom_observed'])
                Node.oom_output=b''
                with self.assertRaises(ValueError):
                    terminate(Client(),'isolated-control-plane',pod,'worker',oom=True)

    def test_oom_only_writes_a_proven_pod_container_cgroup_v2(self):
        identifier='b'*64
        class Result:
            def __init__(self,value): self.stdout=value
        class Node:
            def __init__(self):
                self.path='/kubelet.slice/kubelet-kubepods.slice/kubelet-kubepods-burstable.slice/kubelet-kubepods-burstable-pod1234_5678.slice/cri-containerd-'+identifier+'.scope'
                self.filesystem=b'cgroup2fs\n';self.writes=[]
            def docker(self,*args,data=None,**kwargs):
                if args[0]=='stat': return Result(self.filesystem)
                if args[0]=='crictl': return Result(json.dumps({'info':{'pid':123}}).encode())
                if args[0]=='tee': self.writes.append((args[1],data));return Result(data)
                return Result(('0::'+self.path+'\n').encode() if args[1].startswith('/proc/') else b'0\n')
        pod={'metadata':{'uid':'1234-5678'}}
        node=Node()
        force_oom(node,pod,identifier)
        self.assertEqual(node.writes,[('/sys/fs/cgroup'+node.path+'/memory.oom.group',b'1\n'),('/sys/fs/cgroup'+node.path+'/memory.max',b'1\n')])
        for bad_path in ['/docker/foreign/'+identifier,'/kubepods/../pod1234_5678/'+identifier,'/kubepods/pod1234_5678/foreign']:
            node=Node();node.path=bad_path
            with self.assertRaises(ValueError): force_oom(node,pod,identifier)
            self.assertFalse(node.writes)
        node=Node();node.filesystem=b'tmpfs\n'
        with self.assertRaises(ValueError): force_oom(node,pod,identifier)
        self.assertFalse(node.writes)

    def test_cgroup_target_requires_exact_pod_and_container_components(self):
        identifier='b'*64
        uid='1234-5678'
        for root in ['/kubepods/pod'+uid+'/'+identifier,
                     '/kubepods/burstable/pod'+uid+'/'+identifier,
                     '/kubepods.slice/kubepods-pod1234_5678.slice/cri-containerd-'+identifier+'.scope',
                     '/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod1234_5678.slice/cri-containerd-'+identifier+'.scope',
                     '/kubelet.slice/kubelet-kubepods.slice/kubelet-kubepods-burstable.slice/kubelet-kubepods-burstable-pod1234_5678.slice/cri-containerd-'+identifier+'.scope']:
            with self.subTest(root=root):
                self.assertEqual(container_cgroup(root,uid,identifier),'/sys/fs/cgroup'+root)
                for invalid in [root+'/child',root.replace(uid,'prefix-'+uid),root.replace('1234_5678','x1234_5678'),
                                root.replace(identifier,'a'+identifier),'/foreign'+root,root+'/../'+identifier]:
                    if invalid!=root:
                        with self.assertRaises(ValueError): container_cgroup(invalid,uid,identifier)

    def test_buildkit_oom_waits_for_daemon_readiness_before_pressure(self):
        identifier='a'*64
        pod={'metadata':{'name':'pod','uid':'pod-uid'},'spec':{'nodeName':'isolated-control-plane'},
             'status':{'containerStatuses':[{'name':'buildkit','containerID':'containerd://'+identifier}]}}
        client=Mock()
        client.kubectl.side_effect=RuntimeError('daemon is not ready')
        with patch('faults.NodeUnreachable'),patch('faults.container_identity',return_value={'state':'CONTAINER_RUNNING'}), \
             patch('faults.force_oom') as pressure:
            self.assertIsNone(terminate(client,'isolated-control-plane',pod,'buildkit',oom=True))
            pressure.assert_not_called()


if __name__=='__main__':
    unittest.main()
