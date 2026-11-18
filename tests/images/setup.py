#!/usr/bin/env python3
"""[INPUT]: 显式隔离 namespace、Acceptance 安装与统一镜像 tag。
[OUTPUT]: 配置独立私有 S3、内建仓库、外部仓库和监督型构建池。
[POS]: 复用公开夹具，等待存储后端端口就绪后再启动转发。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import base64
from datetime import datetime, timezone
import hashlib
import hmac
import importlib.util
import json
from pathlib import Path
import re
import secrets
import shutil
import socket
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('image_acceptance_base', ROOT/'tests/operator/acceptance/run.py')
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


def safe_namespace(value):
    if not re.fullmatch(r'artifact-image-[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?', value):
        raise ValueError('Namespace must be a new artifact-image-<name> namespace, at most 63 characters')
    return value


def clean(value):
    value.pop('status', None)
    for key in ['managedFields', 'resourceVersion', 'creationTimestamp', 'uid', 'generation']:
        value['metadata'].pop(key, None)
    return value


class ImageAcceptance(base.Acceptance):
    def __init__(self, context, namespace, tag, resume=False, registry='localhost:5001/artifactory'):
        safe_namespace(namespace)
        if not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}', tag):
            raise ValueError('Invalid image tag')
        super().__init__(context, namespace, 'IfNotPresent', resume)
        self.image_tag = tag
        self.registry = registry.rstrip('/')

    def command(self, args, data=None):
        result = subprocess.run(args, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if result.returncode:
            # 不打印命令参数、响应正文或 Secret，避免错误路径泄露认证。
            raise RuntimeError(f'{Path(args[0]).name} failed with exit {result.returncode}')
        return result.stdout.decode()

    def stop_forwards(self):
        for process in self.processes:
            process.terminate()
        for process in self.processes:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        self.processes = []

    def close(self):
        failed = False
        try:
            if len(self.jar):
                self.api('/api/v1/auth/logout',{})
        except Exception:
            failed = True
        finally:
            self.stop_forwards()
            self.jar.clear()
        if failed:
            raise RuntimeError('acceptance-session-logout-failed')


def create_bucket(a, access, secret, port=38333):
    a.forward('service/image-s3', f'{port}:8333')
    def ready():
        with socket.create_connection(('127.0.0.1', port), timeout=2):
            return True
    a.wait(ready, 30)
    now = datetime.now(timezone.utc)
    stamp, day = now.strftime('%Y%m%dT%H%M%SZ'), now.strftime('%Y%m%d')
    empty = hashlib.sha256(b'').hexdigest()
    host = f'127.0.0.1:{port}'
    names = 'host;x-amz-content-sha256;x-amz-date'
    canonical = f'PUT\n/artifactory-registry\n\nhost:{host}\nx-amz-content-sha256:{empty}\nx-amz-date:{stamp}\n\n{names}\n{empty}'
    scope = day+'/us-east-1/s3/aws4_request'
    value = 'AWS4-HMAC-SHA256\n'+stamp+'\n'+scope+'\n'+hashlib.sha256(canonical.encode()).hexdigest()
    key = ('AWS4'+secret).encode()
    for part in [day, 'us-east-1', 's3', 'aws4_request']:
        key = hmac.new(key, part.encode(), hashlib.sha256).digest()
    signature = hmac.new(key, value.encode(), hashlib.sha256).hexdigest()
    headers = {'Host':host, 'x-amz-date':stamp, 'x-amz-content-sha256':empty,
               'Authorization':f'AWS4-HMAC-SHA256 Credential={access}/{scope}, SignedHeaders={names}, Signature={signature}'}
    request = urllib.request.Request(f'http://{host}/artifactory-registry', data=b'', method='PUT', headers=headers)
    def create():
        try:
            with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=10) as response:
                return response.status == 200
        except urllib.error.HTTPError as error:
            if error.code == 409:
                return True
            raise
    a.wait(create, 30)


def install(a, platforms, allow_emulation=False, emulation_selector=None, external=False):
    material = a.run/'image-private'
    material.mkdir(mode=0o700, exist_ok=True)
    keys_path = material/'credentials.json'
    if not keys_path.exists():
        keys_path.write_text(json.dumps({key:secrets.token_hex(24) for key in ['admin','storage','http','external']}))
        keys_path.chmod(0o600)
    keys = json.loads(keys_path.read_text())
    key, cert = material/'registry.key', material/'registry.pem'
    if not key.exists():
        a.command(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(key),'-out',str(cert),'-days','2','-subj','/CN=Isolated registry signer','-addext','keyUsage=critical,digitalSignature'])
    fixture = clean(json.loads(a.kubectl('get','deploy','upstream','-o','json')))
    container = fixture['spec']['template']['spec']['containers'][0]
    environment = {item['name']:item['value'] for item in container.get('env',[]) if 'value' in item}
    environment.update(FIXTURE_EXECUTOR='buildkit', FIXTURE_ARCH=platforms[0].split('/')[1],
                       FIXTURE_IMAGE_PLATFORMS=','.join(platforms), FIXTURE_ALLOW_EMULATION=str(allow_emulation).lower())
    container['env'] = [{'name':name,'value':value} for name,value in environment.items()]
    container['image'] = a.image('operator-fixture')
    identities = {'identities':[
        {'name':'registry-bootstrap','credentials':[{'accessKey':'registry-bootstrap','secretKey':keys['admin']}], 'actions':['Admin','Read','Write','List','Tagging','WriteAcp']},
        {'name':'registry-storage','credentials':[{'accessKey':'registry-storage','secretKey':keys['storage']}], 'actions':[action+':artifactory-registry' for action in ['Read','Write','List','Tagging','WriteAcp']]}]}
    s3 = a.workload('image-s3',a.registry+'/seaweedfs:4.53',
        ['mini','-dir=/data','-s3.port=8333','-s3.config=/run/s3/s3.json','-s3.iam.readOnly=true','-admin.ui=false','-ip.bind=0.0.0.0'])
    pod = s3['spec']['template']['spec']
    pod['automountServiceAccountToken'] = False
    pod['volumes'] = [{'name':'config','secret':{'secretName':'image-s3-config'}},{'name':'data','emptyDir':{'sizeLimit':'2Gi'}}]
    pod['containers'][0]['volumeMounts'] = [{'name':'config','mountPath':'/run/s3','readOnly':True},{'name':'data','mountPath':'/data'}]
    pod['containers'][0]['readinessProbe'] = {'tcpSocket':{'port':8333},'periodSeconds':1,'failureThreshold':90}
    a.apply(fixture, a.secret('image-s3-config',{'s3.json':json.dumps(identities)}),s3,a.service('image-s3',[8333]),
            a.secret('image-registry-signing',{'tls.key':key.read_text(),'tls.crt':cert.read_text()}),
            a.secret('image-registry-signing-public',{'tls.crt':cert.read_text()}),
            a.secret('image-registry-storage',{'access-key':'registry-storage','secret-key':keys['storage'],'http-secret':keys['http']}))
    for name in ['image-s3','upstream']:
        a.kubectl('rollout','status','deploy/'+name,'--timeout=180s')
    create_bucket(a,'registry-bootstrap',keys['admin'])
    values = {'image':a.registry+'/distribution:3.0.0','replicas':2,'service':'image-registry',
        's3':{'bucket':'artifactory-registry','endpoint':'http://image-s3:8333','secretName':'image-registry-storage','secure':False},
        'auth':{'realm':'http://gateway:8080/api/v1/registry/token','service':'artifactory-registry','issuer':'artifactory','certificateSecret':'image-registry-signing-public'}}
    path = a.run/'registry-values.json'; path.write_text(json.dumps(values))
    a.command(['helm','--kube-context',a.context,'upgrade','--install','image',str(ROOT/'charts/artifactory-registry'),'-n',a.ns,'-f',str(path)])
    a.kubectl('rollout','status','deploy/image-registry','--timeout=180s')
    role = clean(json.loads(a.kubectl('get','role','artifactory-control','-o','json')))
    role['rules'] += [{'apiGroups':['images.artifactory.dev'],'resources':['imagebuilds'],'verbs':['get','create','patch']},
                      {'apiGroups':[''],'resources':['secrets'],'verbs':['get','create','delete']}]
    control = clean(json.loads(a.kubectl('get','deploy','control-plane','-o','json')))
    pod = control['spec']['template']['spec']
    pod['volumes'] = [v for v in pod.get('volumes',[]) if v['name']!='registry-signing'] + [{'name':'registry-signing','secret':{'secretName':'image-registry-signing','defaultMode':0o440}}]
    pod['containers'][0]['volumeMounts'] = [v for v in pod['containers'][0].get('volumeMounts',[]) if v['name']!='registry-signing'] + [{'name':'registry-signing','mountPath':'/run/registry-signing','readOnly':True}]
    environment = {name:base64.b64decode(value).decode() for name,value in json.loads(a.kubectl('get','secret','control-plane','-o','json'))['data'].items()}
    environment.update(ARTIFACTORY_IMAGE_NAMESPACE=a.ns,ARTIFACTORY_REGISTRY_URL='http://image-registry:5000/',
        ARTIFACTORY_IMAGE_CONTROL_URL='https://upstream:9443',ARTIFACTORY_REGISTRY_SIGNING_KEY='/run/registry-signing/tls.key',ARTIFACTORY_REGISTRY_SIGNING_CERT='/run/registry-signing/tls.crt')
    gateway = {name:base64.b64decode(value).decode() for name,value in json.loads(a.kubectl('get','secret','gateway','-o','json'))['data'].items()}
    gateway['ARTIFACTORY_GATEWAY_REGISTRY']='http://image-registry:5000'
    gateway_workload=clean(json.loads(a.kubectl('get','deploy','gateway','-o','json')))
    gateway_workload['spec']['replicas']=2
    a.apply(role,a.secret('control-plane',environment),a.secret('gateway',gateway),control,gateway_workload)
    for name in ['control-plane','gateway']:
        a.kubectl('rollout','restart','deploy/'+name)
        a.kubectl('rollout','status','deploy/'+name,'--timeout=180s')
    a.command(['kubectl','--context',a.context,'apply','-f',str(ROOT/'operators/image-operator/config/crd/resources.yaml')])
    config = '[worker.oci]\n  snapshotter = "native"\n[registry."image-registry:5000"]\n  http = true\n'
    a.apply({'apiVersion':'v1','kind':'ConfigMap','metadata':{'name':'image-buildkit'},'data':{'buildkitd.toml':config}})
    values = {'image':{'repository':a.registry+'/image-operator','tag':a.image_tag,'pullPolicy':'IfNotPresent'},
        'builderPool':{'workerImage':a.image('image-worker'),'buildkitImage':a.image('buildkit'),'sourcePreparerImage':a.image('source-preparer'),
            'imagePullPolicy':'IfNotPresent','controlUrl':'https://upstream:9443','maxActive':2,'maxPerProject':2,
            'cpu':'1','memory':'1Gi','ephemeralStorage':'2Gi','caSecret':'service-tls','buildkitConfigMap':'image-buildkit',
            'emulationNodeSelector':emulation_selector or {},'emulationPlatforms':platforms if allow_emulation else []}}
    path = a.run/'image-operator-values.json'; path.write_text(json.dumps(values))
    a.command(['helm','--kube-context',a.context,'upgrade','--install','image-build',str(ROOT/'charts/artifactory-image-operator'),'-n',a.ns,'-f',str(path)])
    a.kubectl('rollout','status','deploy/image-build-image-operator','--timeout=180s')
    if external:
        install_external(a,keys['external'])
    a.stop_forwards()
    a.forward('service/gateway','38081:8080')
    a.forward('service/upstream','39443:9443')
    a.wait(lambda:a.api('/api/v1/auth/config'))
    a.api('/api/v1/auth/logout',{})
    a.jar.clear()
    a.evidence['image_setup']={'platforms':platforms,'allow_emulation':allow_emulation,'s3_bucket':'artifactory-registry',
        'registry_replicas':2,'buildkit_image':a.image('buildkit'),'external_http':external}


def install_external(a, password):
    tool = shutil.which('htpasswd')
    if not tool:
        raise RuntimeError('External HTTP fixture requires htpasswd on PATH')
    hashed = a.command([tool,'-B','-i','-n','publisher'],data=(password+'\n').encode())
    config = 'version: 0.1\nstorage:\n  filesystem: {rootdirectory: /var/lib/registry}\n  redirect: {disable: true}\nhttp: {addr: ":5000", relativeurls: true}\nauth:\n  htpasswd: {realm: acceptance-external, path: /run/auth/htpasswd}\n'
    workload = a.workload('external-registry',a.registry+'/distribution:3.0.0',['serve','/run/config/config.yml'])
    pod = workload['spec']['template']['spec']
    pod.update(automountServiceAccountToken=False,securityContext={'runAsUser':10001,'runAsGroup':10001,'fsGroup':10001,'runAsNonRoot':True},
        volumes=[{'name':'data','emptyDir':{'sizeLimit':'2Gi'}},{'name':'config','configMap':{'name':'external-registry'}},{'name':'auth','secret':{'secretName':'external-registry-auth'}}])
    pod['containers'][0]['volumeMounts']=[{'name':'data','mountPath':'/var/lib/registry'},{'name':'config','mountPath':'/run/config','readOnly':True},{'name':'auth','mountPath':'/run/auth','readOnly':True}]
    a.apply({'apiVersion':'v1','kind':'ConfigMap','metadata':{'name':'external-registry'},'data':{'config.yml':config}},
        a.secret('external-registry-auth',{'htpasswd':hashed}),a.secret('external-registry-login',{'endpoint':'http://external-registry:5000/','username':'publisher','password':password}),
        workload,a.service('external-registry',[5000]))
    a.kubectl('rollout','status','deploy/external-registry','--timeout=180s')
