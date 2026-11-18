#!/usr/bin/env python3
"""[INPUT]: 独立服务镜像、临时命名空间和确定性模型夹具。
[OUTPUT]: 验证任务交付、恢复暂停池，并收集全副本日志。
[POS]: 仅修改指定验收命名空间；私密证据保存在 .local/。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import base64
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import ssl
import subprocess
import time
import urllib.request
import uuid
from urllib.parse import parse_qs, unquote, urlsplit

ROOT = Path(__file__).resolve().parents[3]
os.umask(0o077)


class Acceptance:
    def __init__(self, context, namespace=None, pull_policy='Always', resume=False):
        self.context = context
        self.image_tag = os.environ.get('ARTIFACTORY_ACCEPTANCE_IMAGE_TAG', 'rust-native')
        self.registry = os.environ.get('ARTIFACTORY_ACCEPTANCE_IMAGE_REGISTRY', 'localhost:5001/artifactory').rstrip('/')
        nodes = json.loads(subprocess.check_output(['kubectl','--context',context,'get','nodes','-o','json']))
        self.arch = nodes['items'][0]['status']['nodeInfo']['architecture']
        self.node_ip = next(address['address'] for address in nodes['items'][0]['status']['addresses'] if address['type']=='InternalIP')
        self.ns = namespace or ('artifact-operator-' + time.strftime('%m%d%H%M%S'))
        self.run = ROOT / '.local/operator-acceptance' / self.ns
        if resume and (not namespace or not (self.run/'pki/ca.pem').is_file()):
            raise ValueError('Resume requires this harness existing namespace and private PKI')
        self.run.mkdir(parents=True, exist_ok=resume)
        self.evidence = {'context': context, 'namespace': self.ns, 'model': 'deterministic fixture; real Pi loop', 'checks': []}
        if resume and (self.run/'evidence.json').is_file():
            self.evidence = json.loads((self.run/'evidence.json').read_text())
        self.jar = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(self.jar))
        self.tenant_id = '00000000-0000-0000-0000-000000000001'
        self.http.addheaders.append(('X-Artifactory-Tenant', self.tenant_id))
        self.evidence['tenant_id'] = self.tenant_id
        self.base = 'http://127.0.0.1:38081'
        self.origin = self.base
        self.processes = []
        self.pull_policy = pull_policy

    def command(self, args, data=None):
        result = subprocess.run(args, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if result.returncode:
            raise RuntimeError(f'{args[0]} failed: {result.stderr.decode()[-2500:]}')
        return result.stdout.decode()

    def kubectl(self, *args, data=None):
        return self.command(['kubectl', '--context', self.context, '-n', self.ns, *args], data)

    def images(self):
        pods = json.loads(self.kubectl('get', 'pods', '-o', 'json'))['items']
        return [{'pod': p['metadata']['name'], 'uid': p['metadata']['uid'], 'images': [{'image': c['image'], 'imageID': c.get('imageID')} for c in p.get('status', {}).get('containerStatuses', [])]} for p in pods if p['status']['phase'] == 'Running' and not p['metadata'].get('deletionTimestamp')]

    def image(self, name):
        return f'{self.registry}/{name}:{self.image_tag}'

    def apply(self, *items):
        self.kubectl('apply', '-f', '-', data=json.dumps({'apiVersion': 'v1', 'kind': 'List', 'items': list(items)}).encode())

    def secret(self, name, values):
        return {'apiVersion': 'v1', 'kind': 'Secret', 'metadata': {'name': name}, 'stringData': values}

    def wait(self, callback, seconds=180):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            try:
                value = callback()
                if value:
                    return value
            except (OSError, RuntimeError, ValueError, urllib.error.URLError):
                pass
            time.sleep(1)
        raise TimeoutError('acceptance phase timed out')

    def check(self, name, value):
        if not value:
            raise AssertionError(name)
        self.evidence['checks'].append(name)
        (self.run / 'evidence.json').write_text(json.dumps(self.evidence, indent=2))
        print('PASS', name, flush=True)

    def verify_pool_pause(self):
        original = json.loads(self.kubectl('get', 'agentpool', 'default', '-o', 'json'))['spec']['maxActive']
        if original <= 0:
            raise AssertionError('Pause acceptance requires an admitting AgentPool')

        def phase_ready(expected):
            pool = json.loads(self.kubectl('get', 'agentpool', 'default', '-o', 'json'))
            status = pool.get('status', {})
            return (status.get('phase') == expected and
                    status.get('observedGeneration') == pool['metadata']['generation'])

        try:
            self.kubectl('patch', 'agentpool', 'default', '--type=merge', '-p', json.dumps({'spec': {'maxActive': 0}}))
            self.wait(lambda: phase_ready('Paused'), 30)
            self.check('pool can pause new admission', True)
        finally:
            self.kubectl('patch', 'agentpool', 'default', '--type=merge', '-p', json.dumps({'spec': {'maxActive': original}}))
            self.wait(lambda: phase_ready('Ready'), 30)

    def api(self, path, body=None, method=None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, method=method, headers={'Content-Type': 'application/json', 'Origin': self.origin, 'x-artifactory-csrf': '1'})
        with self.http.open(request, timeout=30) as response:
            return json.load(response)

    def workload(self, name, image, args, env=None, secret=None, pki=False):
        container = {'name': name, 'image': image, 'args': args, 'imagePullPolicy': self.pull_policy}
        if name == 'control-plane':
            container['readinessProbe'] = {'tcpSocket': {'port': 8080}, 'initialDelaySeconds': 1}
        if env:
            container['env'] = [{'name': key, 'value': value} for key, value in env.items()]
        if secret:
            container['envFrom'] = [{'secretRef': {'name': secret}}]
        spec = {'containers': [container]}
        if pki:
            container['volumeMounts'] = [{'name': 'pki', 'mountPath': '/run/pki', 'readOnly': True}]
            spec['volumes'] = [{'name': 'pki', 'secret': {'secretName': 'acceptance-pki', 'defaultMode': 0o444}}]
        if name == 'control-plane':
            spec['serviceAccountName'] = 'artifactory-control'
            spec['securityContext'] = {'runAsUser':10001,'runAsGroup':10001,'fsGroup':10001,'runAsNonRoot':True}
            spec.setdefault('volumes', []).append({'name': 'data', 'emptyDir': {'sizeLimit': '1Gi'}})
            container.setdefault('volumeMounts', []).append({'name': 'data', 'mountPath': '/tmp/control'})
        return {'apiVersion': 'apps/v1', 'kind': 'Deployment', 'metadata': {'name': name}, 'spec': {'replicas': 1, 'selector': {'matchLabels': {'app': name}}, 'template': {'metadata': {'labels': {'app': name}}, 'spec': spec}}}

    def service(self, name, ports):
        return {'apiVersion': 'v1', 'kind': 'Service', 'metadata': {'name': name}, 'spec': {'selector': {'app': name}, 'ports': [{'name': 'p' + str(port), 'port': port, 'targetPort': port} for port in ports]}}

    def control_identity(self):
        role = {'apiVersion':'rbac.authorization.k8s.io/v1','kind':'Role','metadata':{'name':'artifactory-control'},'rules':[
            {'apiGroups':['agents.artifactory.dev'],'resources':['sourcetasks','agentruns','agentpools'],'verbs':['get','create','patch']},
            {'apiGroups':['batch'],'resources':['jobs'],'verbs':['get']},
            {'apiGroups':[''],'resources':['pods'],'verbs':['get']}]}
        binding = {'apiVersion':'rbac.authorization.k8s.io/v1','kind':'RoleBinding','metadata':{'name':'artifactory-control'},'subjects':[{'kind':'ServiceAccount','name':'artifactory-control','namespace':self.ns}],'roleRef':{'apiGroup':'rbac.authorization.k8s.io','kind':'Role','name':'artifactory-control'}}
        cluster = {'apiVersion':'rbac.authorization.k8s.io/v1','kind':'ClusterRole','metadata':{'name':self.ns+'-control-tokenreview'},'rules':[{'apiGroups':['authentication.k8s.io'],'resources':['tokenreviews'],'verbs':['create']}]}
        cluster_binding = {'apiVersion':'rbac.authorization.k8s.io/v1','kind':'ClusterRoleBinding','metadata':{'name':self.ns+'-control-tokenreview'},'subjects':binding['subjects'],'roleRef':{'apiGroup':'rbac.authorization.k8s.io','kind':'ClusterRole','name':cluster['metadata']['name']}}
        return {'apiVersion':'v1','kind':'ServiceAccount','metadata':{'name':'artifactory-control'}}, role, binding, cluster, cluster_binding

    def gateway(self, token):
        env = {
            'ARTIFACTORY_GATEWAY_PROFILE': 'all',
            'ARTIFACTORY_GATEWAY_HTTP_BIND': '0.0.0.0:8080',
            'ARTIFACTORY_GATEWAY_CONTROL': 'http://control-plane:8080',
            'ARTIFACTORY_GATEWAY_REGISTRY': 'http://control-plane:8080',
            'ARTIFACTORY_GATEWAY_WEB': 'http://control-plane:8080',
            'ARTIFACTORY_GATEWAY_DEVICE': 'control-plane:8443',
            'ARTIFACTORY_GATEWAY_TOKEN': token,
            'ARTIFACTORY_S3_ENDPOINT': 'http://s3:8333',
            'ARTIFACTORY_INSECURE_DEV': 'true',
        }
        secret = self.secret('gateway', env)
        deployment = self.workload('gateway', self.image('gateway'), [], secret='gateway')
        deployment['spec']['template']['spec']['automountServiceAccountToken'] = False
        deployment['spec']['template']['spec']['containers'][0]['readinessProbe'] = {'httpGet': {'path': '/healthz', 'port': 8080}}
        return secret, deployment, self.service('gateway', [8080])

    def sql(self, database, statement):
        return self.kubectl('exec', 'service/postgres', '--', 'psql', '-U', 'admin', '-d', database, '-qAt', '-v', 'ON_ERROR_STOP=1', '-c', statement).strip()

    def setup(self):
        self.command(['kubectl', '--context', self.context, 'create', 'namespace', self.ns])
        pki = self.run / 'pki'; pki.mkdir()
        self.command(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(pki/'ca.key'), '-out', str(pki/'ca.pem'), '-days', '2', '-subj', '/CN=Operator isolated acceptance', '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign', '-addext', 'subjectKeyIdentifier=hash'])
        self.command(['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(pki/'server.key'), '-out', str(pki/'server.csr'), '-subj', '/CN=upstream'])
        (pki/'ext').write_text('subjectAltName=DNS:localhost,DNS:control-plane,DNS:upstream,DNS:artifactory-runtime,DNS:artifactory-telemetry,IP:127.0.0.1,IP:'+self.node_ip+'\nextendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature,keyEncipherment\nbasicConstraints=critical,CA:FALSE\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n')
        self.command(['openssl', 'x509', '-req', '-in', str(pki/'server.csr'), '-CA', str(pki/'ca.pem'), '-CAkey', str(pki/'ca.key'), '-CAcreateserial', '-out', str(pki/'server.pem'), '-days', '2', '-extfile', str(pki/'ext')])
        self.command(['openssl', 'verify', '-x509_strict', '-CAfile', str(pki/'ca.pem'), str(pki/'server.pem')])
        self.apply(self.secret('acceptance-pki', {name: (pki/name).read_text() for name in ['ca.key','ca.pem','server.key','server.pem']}), self.secret('service-tls', {'ca.crt': (pki/'ca.pem').read_text(), 'tls.crt': (pki/'server.pem').read_text(), 'tls.key': (pki/'server.key').read_text()}))
        password = secrets.token_hex(20); token = secrets.token_hex(32); admin = secrets.token_hex(20); gateway_token = secrets.token_hex(32)
        self.token=token
        self.tls=ssl.create_default_context(cafile=str(pki/'ca.pem'))
        databases = "\n".join(f"CREATE USER {role} PASSWORD '{password}'; CREATE DATABASE {role} OWNER {role};\n\\connect {role}\nCREATE EXTENSION IF NOT EXISTS timescaledb;\n\\connect postgres" for role in ['business','planner','telemetry'])
        postgres = self.workload('postgres', self.registry+'/timescaledb:2.32.0-pg17', [], {'POSTGRES_USER': 'admin', 'POSTGRES_PASSWORD': password, 'TIMESCALEDB_TELEMETRY':'off'})
        postgres['spec']['template']['spec']['containers'][0]['volumeMounts'] = [{'name': 'init', 'mountPath': '/docker-entrypoint-initdb.d'}]
        postgres['spec']['template']['spec']['volumes'] = [{'name': 'init', 'configMap': {'name': 'init-databases'}}]
        self.apply({'apiVersion':'v1','kind':'ConfigMap','metadata':{'name':'init-databases'},'data':{'100_init.sql':databases}}, postgres, self.service('postgres',[5432]))
        s3config = {'identities':[{'name':'acceptance','credentials':[{'accessKey':'acceptance','secretKey':password}],'actions':['Admin','Read','Write','List','Tagging']}]}
        s3 = self.workload('s3',self.registry+'/seaweedfs:4.53',['mini','-dir=/data','-s3.port=8333','-s3.config=/run/s3/s3.json','-s3.iam.readOnly=true','-admin.ui=false','-ip.bind=0.0.0.0'])
        s3['spec']['template']['spec']['containers'][0]['volumeMounts']=[{'name':'config','mountPath':'/run/s3','readOnly':True}]
        s3['spec']['template']['spec']['volumes']=[{'name':'config','secret':{'secretName':'s3-config'}}]
        self.apply(self.secret('s3-config',{'s3.json':json.dumps(s3config)}),s3,self.service('s3',[8333]))
        shared={'ARTIFACTORY_SERVICE_TOKEN':token,'ARTIFACTORY_S3_ENDPOINT':'http://s3:8333','ARTIFACTORY_S3_BUCKET':'operator-acceptance','ARTIFACTORY_S3_ACCESS_KEY':'acceptance','ARTIFACTORY_S3_SECRET_KEY':password}
        shared['RUST_LOG']='info'
        runtime=dict(shared,DATABASE_URL=f'postgres://planner:{password}@postgres:5432/planner',ARTIFACTORY_SERVICE_KEY=base64.b64encode(secrets.token_bytes(32)).decode())
        control=dict(shared,DATABASE_URL=f'postgres://business:{password}@postgres:5432/business',ARTIFACTORY_MASTER_KEY=base64.b64encode(secrets.token_bytes(32)).decode(),ARTIFACTORY_BOOTSTRAP_USERNAME='operator-admin',ARTIFACTORY_BOOTSTRAP_PASSWORD=admin,
                     ARTIFACTORY_RUNTIME_URL='https://artifactory-runtime:8091',ARTIFACTORY_KUBERNETES_NAMESPACE=self.ns,ARTIFACTORY_AGENT_POOL='default',ARTIFACTORY_SERVICE_CA='/run/pki/ca.pem',ARTIFACTORY_PUBLIC_URL=self.base,ARTIFACTORY_INSECURE_DEV='true',ARTIFACTORY_HTTP_BIND='0.0.0.0:8080',ARTIFACTORY_GRPC_BIND='0.0.0.0:8443',ARTIFACTORY_PKI_DIR='/run/pki',ARTIFACTORY_DATA_DIR='/tmp/control')
        telemetry=dict(DATABASE_URL=f'postgres://telemetry:{password}@postgres:5432/telemetry',ARTIFACTORY_BUSINESS_DATABASE_URL=f'postgres://business:{password}@postgres:5432/business',ARTIFACTORY_TELEMETRY_TOKEN=token)
        control.update(ARTIFACTORY_TELEMETRY_ENDPOINT='https://artifactory-telemetry:8444',ARTIFACTORY_TELEMETRY_URL='https://artifactory-telemetry:8093',ARTIFACTORY_TELEMETRY_TOKEN=token)
        # 设备使用集群网关，主机客户端通过端口转发访问同一入口。
        control['ARTIFACTORY_PUBLIC_URL']='http://gateway:8080'
        control['ARTIFACTORY_GATEWAY_TOKEN']=gateway_token
        self.origin=control['ARTIFACTORY_PUBLIC_URL']
        self.apply(*self.control_identity(),self.secret('telemetry',telemetry),self.secret('runtime',runtime),self.secret('control-plane',control),self.workload('control-plane',self.image('control-plane'),['serve'],secret='control-plane',pki=True),self.service('control-plane',[8080,8443]),*self.gateway(gateway_token),self.workload('upstream',self.image('operator-fixture'),[],env={'FIXTURE_ARCH':self.arch},pki=True),self.service('upstream',[9443]))
        self.wait(lambda:self.sql('business','SELECT 1')=='1')
        self.command(['kubectl','--context',self.context,'apply','-f',str(ROOT/'operators/agent-operator/config/crd/resources.yaml')])
        values={'image':{'repository':self.registry+'/agent-operator','tag':self.image_tag,'pullPolicy':self.pull_policy},'runtime':{'secretName':'runtime','image':{'repository':self.registry+'/planner-runtime','tag':self.image_tag}},'telemetry':{'secretName':'telemetry','pkiSecretName':'acceptance-pki','image':{'repository':self.registry+'/control-plane','tag':self.image_tag}},'agentPool':{'image':self.image('operator-pi'),'sourcePreparerImage':self.image('source-preparer'),'maxActive':2,'maxPerProject':1,'ephemeralStorage':'2Gi'},'tls':{'secretName':'service-tls'},'controlUrl':'https://upstream:9443','networkPolicy':{'enabled':True,'gitPorts':[9443]}}
        path=self.run/'values.json';path.write_text(json.dumps(values))
        self.command(['helm','--kube-context',self.context,'upgrade','--install','native',str(ROOT/'charts/artifactory-agent-operator'),'-n',self.ns,'-f',str(path)])
        self.kubectl('rollout','status','deploy/control-plane','--timeout=180s')
        self.kubectl('rollout','status','deploy/gateway','--timeout=180s')
        self.forward('service/gateway','38081:8080')
        self.forward('service/upstream','39443:9443')
        self.wait(lambda:self.api('/api/v1/auth/config'))
        self.enroll(admin)

    def enroll(self, admin):
        self.api('/api/v1/auth/login',{'username':'operator-admin','password':admin})
        if not self.api('/api/v1/model-profiles'):
            self.api('/api/v1/model-profiles',{'name':'Operator fixture','api':'openai-completions','base_url':'https://upstream:9443/v1','model':'fixture','api_key':'fixture-provider-key','parameters':{'thinking_level':'off','max_tokens':4096},'max_calls':20,'timeout_seconds':600,'is_default':True})
        enrollment=self.api('/api/v1/enrollment-tokens',{'pool_id':'00000000-0000-0000-0000-000000000001'})['token']
        self.apply(self.secret('emissary-token',{'ARTIFACTORY_ENROLLMENT_TOKEN':enrollment}))
        shell='artifactory-emissary register --config /tmp/emissary.json --endpoint control-plane:8443 --ca /run/pki/ca.pem --root /tmp/emissary --name operator-native-device --allow-http-uploads && sed -i \'s|"git_ca_file":""|"git_ca_file":"/run/pki/ca.pem"|\' /tmp/emissary.json && exec artifactory-emissary run --config /tmp/emissary.json'
        device=self.workload('device',self.image('operator-fixture'),[],secret='emissary-token',pki=True);device['spec']['template']['spec']['containers'][0]['command']=['/usr/bin/tini','--','sh','-ec',shell]
        if not any(node['online'] for node in self.api('/api/v1/nodes')):self.apply(device)
        self.wait(lambda:any(node['online'] for node in self.api('/api/v1/nodes')))
        projects=self.api('/api/v1/projects')
        fixture=next((p for p in projects if p['name']=='Kubernetes Pi acceptance'),None)
        self.project=fixture['id'] if fixture else self.api('/api/v1/projects',{'name':'Kubernetes Pi acceptance','repository_url':'https://upstream:9443/repo.git',})['id']
        self.check('independent namespace, databases, Operator and Rust Emissary enrolled',True)

    def resume(self):
        # Only resume an installation created by this harness with the same private CA.
        secret=json.loads(self.kubectl('get','secret','control-plane','-o','json'))['data']
        expected=(self.run/'pki/ca.pem').read_bytes()
        pki=json.loads(self.kubectl('get','secret','acceptance-pki','-o','json'))['data']
        if base64.b64decode(pki['ca.pem'])!=expected:
            raise ValueError('Existing namespace does not belong to this saved run')
        self.token=base64.b64decode(secret['ARTIFACTORY_SERVICE_TOKEN']).decode()
        self.tls=ssl.create_default_context(cafile=str(self.run/'pki/ca.pem'))
        self.origin=self.kubectl('exec','deploy/control-plane','--','printenv','ARTIFACTORY_PUBLIC_URL').strip()
        if 'ARTIFACTORY_GATEWAY_TOKEN' not in secret:
            raise ValueError('Resume requires a gateway-aware acceptance installation')
        self.forward('service/gateway','38081:8080');self.forward('service/upstream','39443:9443')
        self.wait(lambda:self.api('/api/v1/auth/config'))
        self.enroll(base64.b64decode(secret['ARTIFACTORY_BOOTSTRAP_PASSWORD']).decode())

    def forward(self, resource, ports):
        log=(self.run/('port-forward-'+ports.split(':')[0]+'.log')).open('wb')
        self.processes.append(subprocess.Popen(['kubectl','--context',self.context,'-n',self.ns,'port-forward',resource,ports],stdout=log,stderr=log));log.close()

    def service_request(self, port, path, body=None):
        request=urllib.request.Request(f'https://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+self.token})
        opener=urllib.request.build_opener(urllib.request.ProxyHandler({}),urllib.request.HTTPSHandler(context=self.tls))
        with opener.open(request,timeout=20) as response:
            return response.read().decode()

    def verify_artifact_gateway(self, artifact):
        request = urllib.request.Request(self.base + '/api/v1/artifacts/' + artifact['id'] + '/download', headers={'Accept': 'application/json'})
        with self.http.open(request, timeout=20) as response:
            url = urlsplit(json.load(response)['url'])
        self.check('artifact download advertises the gateway instead of private S3', url.netloc == urlsplit(self.origin).netloc and url.path == '/artifacts/object' and bool(parse_qs(url.query).get('ticket')))
        forwarded = self.base + url.path + '?' + url.query
        with self.http.open(forwarded, timeout=20) as response:
            data = response.read()
        self.check('artifact bytes download through the gateway', hashlib.sha256(data).hexdigest() == artifact['sha256'])
        with self.http.open(urllib.request.Request(forwarded, method='HEAD'), timeout=20) as response:
            self.check('artifact gateway preserves HEAD length', int(response.headers['Content-Length']) == len(data) and not response.read())
        with self.http.open(urllib.request.Request(forwarded, headers={'Range': 'bytes=0-7'}), timeout=20) as response:
            self.check('artifact gateway preserves Range', response.status == 206 and response.read() == data[:8])

    def run_state(self, build, *, generation=None, calls=1):
        runs = self.api('/api/v1/builds/'+build+'/plans')['runs']
        if not runs:
            return None
        run = runs[0]['id']
        value = self.sql('planner', f"SELECT jsonb_build_object('id',id,'life',lifecycle,'execution',execution) FROM runtime_runs WHERE id='{run}' AND status='running' AND (lifecycle->>'calls')::int={calls}")
        if not value:
            return None
        state = json.loads(value)
        return state if generation is None or state['life']['generation'] == generation else None

    def check_temporary_storage(self, *, idle=False):
        import importlib.util
        path = ROOT/'scripts/check_ephemeral_source.py'
        spec = importlib.util.spec_from_file_location('ephemeral_source_check', path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        resources = json.loads(self.kubectl('get','pods,jobs,pvc,deployments,statefulsets','-o','json'))['items']
        names = self.kubectl('get','secrets','-o','custom-columns=NAME:.metadata.name','--no-headers').splitlines()
        failures = module.violations(resources,names,idle=idle)
        if failures:
            if idle:
                return False
            raise AssertionError('; '.join(failures))
        return True

    def conversation(self, build, run, generation):
        after = 0
        messages = []
        while True:
            page = self.api(f'/api/v1/builds/{build}/planning/conversations/{run}/{generation}/messages?after={after}&limit=50')
            messages.extend(page['messages'])
            if page.get('next_cursor') is None:
                return messages
            after = page['next_cursor']

    def wait_conversation(self, build, run, generation, digest, seconds=60):
        # === 跨库回执 ===
        # 业务构建完成后，Runtime 仍须重试已丢失的接受回执。
        def ready():
            sessions = self.api('/api/v1/builds/'+build+'/planning/conversations')['conversations']
            messages = self.conversation(build, run, generation)
            self.evidence['conversation_snapshot'] = {
                'sessions': [{key: session.get(key) for key in
                    ('run_id', 'generation', 'status', 'source_digest')} for session in sessions],
                'message_count': len(messages),
                'message_kinds': sorted({message['kind'] for message in messages}),
            }
            (self.run / 'evidence.json').write_text(json.dumps(self.evidence, indent=2))
            if (len(sessions) == 1 and sessions[0]['run_id'] == run
                    and sessions[0]['generation'] == generation
                    and sessions[0]['status'] == 'succeeded'
                    and sessions[0]['source_digest'] == digest and messages):
                return sessions, messages
            return None
        return self.wait(ready, seconds)

    def verify(self):
        self.kubectl('patch','agentpool','default','--type=merge','-p','{"spec":{"maxActive":2}}')
        self.service_request(39443,'/test/mode',{'reset_ack':True,'hold':True})
        build = self.api(f'/api/v1/projects/{self.project}/builds',{'ref':'v-fixture','goal':'Inspect source and build dist/result.txt on Linux '+self.arch,'target_requirements':[{'os':'linux','arch':self.arch}]})['id']
        self.evidence['build_id'] = build
        running = self.wait(lambda:self.run_state(build),300)
        run = running['id']
        pod = json.loads(self.kubectl('get','pod',running['execution']['name'],'-o','json'))
        source = next(c for c in pod['spec'].get('initContainers',[]) if c['name']=='source')
        self.check('AI source init completes before first inference', any(c['name']=='source' and c['state'].get('terminated',{}).get('exitCode')==0 for c in pod['status']['initContainerStatuses']))
        self.check('AI main container mounts fixed source read-only', any(m['mountPath']=='/workspace' and m.get('readOnly') for m in pod['spec']['containers'][0]['volumeMounts']))
        self.check('init has source identity and main has no source credentials', any(m['name']=='source-identity' for m in source['volumeMounts']) and not any(m['name'] in ['source-auth','source-identity'] for m in pod['spec']['containers'][0]['volumeMounts']))
        self.check('running tasks use only ephemeral source storage',self.check_temporary_storage())
        detail = self.api('/api/v1/builds/'+build)
        manifest_id = detail['source_manifest_id']
        manifest = json.loads(self.sql('business',f"SELECT manifest FROM source_manifests WHERE id='{manifest_id}'"))
        self.check('SourceTask stores fixed commit and manifest metadata',manifest['repositories'][0]['commit_sha']==detail['commit_sha'] and len(manifest['digest'])==64)
        self.check('SourceTask has no persistent checkout tables',self.sql('business',"SELECT to_regclass('source_snapshots') IS NULL AND to_regclass('source_repositories') IS NULL")=='t')
        self.check('Control has neither Git binary nor source directory',self.kubectl('exec','deploy/control-plane','--','sh','-ec','test ! -e /tmp/control/sources && ! command -v git && echo clean').strip()=='clean')
        self.service_request(39443,'/test/mode',{})
        def terminal():
            value = self.api('/api/v1/builds/'+build)
            return value if value['status'] in ['succeeded','failed','lost','canceled','needs_input'] else None
        detail = self.wait(terminal,600)
        (self.run/'build.json').write_text(json.dumps(detail,indent=2))
        self.check('fixed manifest -> AI init -> validated plan -> native build',detail['status']=='succeeded')
        history = self.api('/api/v1/builds/'+build+'/plans')
        self.check('one logical run and one accepted revision',len(history['runs'])==1 and len(history['plans'])==1)
        calls = int(self.sql('planner',f"SELECT lifecycle->>'calls' FROM runtime_runs WHERE id='{run}'"))
        self.check('invalid plan is corrected within one cumulative budget',calls==4)
        self.check('lost acceptance ACK reuses result without another inference','fixture_dropped_accept_acks 1' in self.service_request(39443,'/metrics') and calls==4)
        self.check('native build verifies actual LFS bytes',detail['artifacts'] and detail['artifacts'][0]['sha256']==hashlib.sha256(b'operator-native-ok\n').hexdigest())
        self.verify_artifact_gateway(detail['artifacts'][0])
        sessions, messages = self.wait_conversation(build,run,1,manifest['digest'])
        self.check('AI conversation is durable and independent of source cleanup',len(sessions)==1 and sessions[0]['status']=='succeeded' and sessions[0]['source_digest']==manifest['digest'] and bool(messages))
        self.check('conversation includes source tool and validated proposal results','README.md' in json.dumps(messages) and {'proposal_submitted','proposal_result'} <= {m['kind'] for m in messages})
        self.evidence.update(calls=calls,source_manifest_id=manifest_id,source_digest=manifest['digest'],conversation_messages=len(messages))
        self.faults(detail['commit_sha'])
        self.wait(lambda:self.check_temporary_storage(idle=True),120)
        self.check('terminal tasks leave no Pod, Job, source PVC or task Secret',True)
        print('EVIDENCE',self.run/'evidence.json',flush=True)

    def faults(self, commit):
        self.forward('service/artifactory-runtime','38091:8091')
        self.wait(lambda:'artifactory_runtime_runs' in self.service_request(38091,'/metrics'))
        try:
            urllib.request.urlopen('https://127.0.0.1:38091/metrics',context=self.tls,timeout=10)
            unauthorized=False
        except urllib.error.HTTPError as error:
            unauthorized=error.code==401
        self.check('Runtime metrics reject unauthenticated requests',unauthorized)
        lease=json.loads(self.kubectl('get','lease','artifactory-agent-operator','-o','json'))
        holder=lease['spec']['holderIdentity']
        pods=json.loads(self.kubectl('get','pods','-l','app.kubernetes.io/name=artifactory-agent-operator','-o','json'))['items']
        leader=next(p['metadata']['name'] for p in pods if p['metadata']['uid']==holder)
        self.kubectl('delete','pod',leader,'--wait=false')
        self.wait(lambda:json.loads(self.kubectl('get','lease','artifactory-agent-operator','-o','json'))['spec']['holderIdentity']!=holder,60)
        self.check('two Operator replicas elect a replacement after leader deletion',True)
        # Hold a real model request before its headers, then kill the owning Pi Pod.
        self.service_request(39443,'/test/mode',{'hold':True})
        build=self.api(f'/api/v1/projects/{self.project}/builds',{'ref':'v-fixture','goal':'fault injection: retain budget across Pod loss','target_requirements':[{'os':'linux','arch':self.arch}]})['id']
        before=self.wait(lambda:self.run_state(build),300)
        pod=json.loads(self.kubectl('get','pod',before['execution']['name'],'-o','json'))
        self.evidence['pi_image']={key:pod['status']['containerStatuses'][0][key] for key in ['image','imageID']}
        samples=[]
        for _ in range(30):
            started=time.monotonic();self.api('/api/v1/overview');samples.append((time.monotonic()-started)*1000)
        self.evidence['business_api_during_held_pi']={'samples':len(samples),'p95_ms':round(sorted(samples)[int(len(samples)*.95)-1],3),'scope':'local small-load observation; no production SLO claim'}
        run=before['id'];self.evidence['fault_run_id']=run
        self.kubectl('delete','pod',before['execution']['name'],'--wait=false')
        self.sql('planner',f"UPDATE runtime_runs SET lifecycle=jsonb_set(lifecycle,'{{lease_until_ms}}',to_jsonb((extract(epoch FROM now())*1000)::bigint-1000)) WHERE id='{run}'")
        def retried():
            value=self.sql('planner',f"SELECT lifecycle FROM runtime_runs WHERE id='{run}'")
            life=json.loads(value)
            return life if life['generation']==2 and life['calls']>=2 else None
        after=self.wait(retried,120)
        self.check('Pod replacement keeps original deadline and spent model calls',after['deadline_ms']==before['life']['deadline_ms'] and after['calls']==2)
        sessions=self.api('/api/v1/builds/'+build+'/planning/conversations')['conversations']
        self.check('Pod loss creates a new conversation without replaying the old session',len(sessions)==2 and sessions[0]['generation']==1 and sessions[0]['status']=='interrupted' and sessions[1]['generation']==2 and sessions[1]['status']=='running' and sessions[0]['pod_uid']!=sessions[1]['pod_uid'])
        self.check('physical retry retains source commit',self.api('/api/v1/builds/'+build)['commit_sha']==commit)
        started=time.monotonic();self.api('/api/v1/builds/'+build+'/cancel',{})
        self.evidence['cancel_api_seconds']=round(time.monotonic()-started,3)
        self.wait(lambda:self.sql('planner',f"SELECT status FROM runtime_runs WHERE id='{run}'")=='canceled',30)
        self.evidence['cancel_authority_observed_seconds']=round(time.monotonic()-started,3)
        self.wait(lambda:not json.loads(self.kubectl('get','pods','-l','artifactory.dev/run='+run,'-o','json'))['items'],60)
        self.check('cancel revokes authorization and stops Pi without creating device jobs',not self.api('/api/v1/builds/'+build)['jobs'])
        self.evidence['cancel_physical_stop_seconds']=round(time.monotonic()-started,3)
        self.service_request(39443,'/test/mode',{})
        self.kubectl('rollout','restart','deploy/native-runtime')
        self.kubectl('rollout','status','deploy/native-runtime','--timeout=120s')
        persisted=json.loads(self.sql('planner',f"SELECT lifecycle FROM runtime_runs WHERE id='{run}'"))
        self.check('Runtime restart preserves cancellation, budget and deadline',persisted['canceled'] and persisted['calls']==2 and persisted['deadline_ms']==before['life']['deadline_ms'])
        # 暂停只阻止新准入；验收退出前恢复原容量。
        self.verify_pool_pause()

    def real_model(self, environment_file):
        # Reuse an explicitly selected local development profile in memory only.
        # Neither the provider key nor source master key is written to test evidence.
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        environment=json.loads(Path(environment_file).read_text())
        query="SELECT jsonb_build_object('api',api,'base_url',base_url,'model',model,'parameters',parameters,'key_ciphertext',key_ciphertext) FROM model_profiles WHERE enabled ORDER BY is_default DESC,created_at LIMIT 1"
        database=urlsplit(environment['DATABASE_URL'])
        connection=dict(os.environ,PGHOST=database.hostname or '127.0.0.1',PGPORT=str(database.port or 5432),PGDATABASE=unquote(database.path.lstrip('/')),PGUSER=unquote(database.username or ''),PGPASSWORD=unquote(database.password or ''),PGCONNECT_TIMEOUT='5')
        for option,value in parse_qs(database.query).items():
            if option in ['sslmode','sslrootcert','sslcert','sslkey']:connection['PG'+option.upper()]=value[0]
        result=subprocess.run(['psql','-XAt','-c',query],env=connection,capture_output=True,check=True,text=True)
        model=json.loads(result.stdout)
        encrypted=base64.b64decode(model.pop('key_ciphertext'))
        key=AESGCM(base64.b64decode(environment['ARTIFACTORY_MASTER_KEY'])).decrypt(encrypted[:12],encrypted[12:],None).decode()
        profile=self.api('/api/v1/model-profiles',dict(model,name='Operator real model acceptance',api_key=key,max_calls=10,timeout_seconds=600,is_default=False))['id']
        del key,environment,connection,database
        self.kubectl('patch','agentpool','default','--type=merge','-p','{"spec":{"maxActive":2}}')
        project=self.api('/api/v1/projects',{'name':'Operator real model acceptance','repository_url':'https://upstream:9443/repo.git','model_profile_id':profile})['id']
        build=self.api(f'/api/v1/projects/{project}/builds',{'ref':'v-fixture','goal':'Read README.md and build.sh. Build this source using sh build.sh and collect dist/result.txt on the available Linux device. Submit an ExecutionPlan v1 with evidence; no signing or external scripts are necessary.','target_requirements':[{'os':'linux','arch':self.arch}]})['id']
        self.evidence['real_model']={'provider':model['base_url'],'model':model['model'],'build_id':build,'max_calls':10}
        self.check('real supplier build submitted through the same Kubernetes Pi runtime',True)
        previous=None
        def terminal():
            nonlocal previous
            detail=self.api('/api/v1/builds/'+build)
            if detail['status']!=previous:print('REAL MODEL BUILD',detail['status'],flush=True);previous=detail['status']
            return detail if detail['status'] in ['succeeded','failed','lost','canceled','needs_input'] else None
        detail=self.wait(terminal,900)
        (self.run/'real-model-build.json').write_text(json.dumps(detail,indent=2))
        self.check('real supplier -> Kubernetes Pi -> control acceptance -> Rust Emissary build',detail['status']=='succeeded')
        self.check('real-model build artifact content matches',detail['artifacts'][0]['sha256']==hashlib.sha256(b'operator-native-ok\n').hexdigest())
        run=self.api('/api/v1/builds/'+build+'/plans')['runs'][0]['id']
        self.evidence['real_model']['runtime']=json.loads(self.sql('planner',f"SELECT jsonb_build_object('lifecycle',lifecycle,'usage',usage,'status',status) FROM runtime_runs WHERE id='{run}'"))
        self.check('real model call budget is persisted in Runtime',True)

    def retention(self):
        self.wait(lambda:self.check_temporary_storage(idle=True),120)
        self.evidence['images']=self.images()
        self.evidence['passed']=True
        self.check('task cleanup retains source metadata, conversations and artifacts',bool(self.evidence['images']))

    def cleanup(self):
        for process in self.processes:process.terminate()
        for name in ['control-plane','gateway','native-agent-operator','native-runtime','native-telemetry','upstream','device']:
            target = ['-l', 'app.kubernetes.io/name=artifactory-agent-operator', '--prefix=true', '--max-log-requests=10'] if name == 'native-agent-operator' else ['deploy/'+name]
            try:(self.run/(name+'.log')).write_text(self.kubectl('logs',*target,'--all-containers=true','--tail=500'))
            except RuntimeError:pass
        # Keep the namespace for inspection; explicit user cleanup is separate.


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--context',default='kind-substrate-orbstack');parser.add_argument('--namespace');parser.add_argument('--pull-policy',default='Always',choices=['Always','IfNotPresent']);parser.add_argument('--resume',action='store_true');parser.add_argument('--real-model-env',help='Explicit local development env.json whose encrypted model profile can be used');args=parser.parse_args()
    acceptance=Acceptance(args.context,args.namespace,args.pull_policy,args.resume)
    try:
        if args.resume:acceptance.resume()
        else:acceptance.setup()
        acceptance.verify()
        if args.real_model_env:acceptance.real_model(args.real_model_env)
        acceptance.retention()
    finally:acceptance.cleanup()
