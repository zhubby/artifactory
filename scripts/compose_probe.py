#!/usr/bin/env python3
"""[INPUT]: 隔离 Compose 网络、TLS 信任链与登录身份。
[OUTPUT]: 验证 HTTPS、文档、会话、CSRF、租户鉴权与重启。
[POS]: Compose 只承载开发服务；完整任务链在 Kubernetes 验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import http.cookiejar
import json
import os
from pathlib import Path
import ssl
import time
import urllib.error
import urllib.request
import uuid

DIRECTORY = Path('/acceptance')


def main():
    os.umask(0o077)
    base = os.environ['ARTIFACTORY_COMPOSE_CONSOLE_URL']
    tenant = '00000000-0000-0000-0000-000000000001'
    evidence = {'kind': 'compose-service-probe', 'checks': [], 'builds_executed': False}
    context = ssl.create_default_context(cafile=str(DIRECTORY / 'https-ca.pem'))
    cookies = http.cookiejar.CookieJar()

    def opener(authenticated=True):
        handlers = [urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context)]
        if authenticated:
            handlers.append(urllib.request.HTTPCookieProcessor(cookies))
        return urllib.request.build_opener(*handlers)

    client = opener()

    def check(name, condition):
        if not condition:
            raise AssertionError(name)
        evidence['checks'].append(name)
        print('PASS', name, flush=True)

    def wait(callback, seconds=90):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                result = callback()
                if result:
                    return result
            except (OSError, urllib.error.URLError):
                pass
            time.sleep(.5)
        raise TimeoutError('Compose service probe timed out')

    def request(path, body=None, *, authenticated=True, csrf=True, origin=base, scope=tenant):
        headers = {'Content-Type': 'application/json', 'Origin': origin, 'X-Artifactory-Tenant': scope}
        if csrf:
            headers['x-artifactory-csrf'] = '1'
        outgoing = urllib.request.Request(base + '/api/v1' + path,
            None if body is None else json.dumps(body).encode(), headers)
        try:
            with (client if authenticated else opener(False)).open(outgoing, timeout=15) as response:
                return response.status, json.load(response), response.headers
        except urllib.error.HTTPError as error:
            return error.code, json.load(error), error.headers

    def api(path, body=None):
        status, value, _ = request(path, body)
        if status != 200:
            raise AssertionError(f'{path}: HTTP {status}')
        return value

    try:
        def ready():
            with opener(False).open(base + '/', timeout=3) as response:
                return response.status == 200 and request('/auth/me', authenticated=False)[0] == 401
        wait(ready)
        check('console uses trusted HTTPS and anonymous API access is rejected', True)
        for path in ('/docs', '/docs/'):
            with opener(False).open(base + path, timeout=15) as response:
                check('anonymous documentation ' + path, response.status == 200 and 'Artifactory 文档' in response.read().decode())
        for path, kind in (('/docs/mermaid.min.js', 'javascript'),
                           ('/docs/brand/artifactory-assembly-a.png', 'image/png')):
            with opener(False).open(base + path, timeout=15) as response:
                check('documentation resource ' + path, kind in response.headers.get('Content-Type', '') and len(response.read()) > 100)
        try:
            opener(False).open(base + '/docs/missing/nested.html', timeout=15)
            raise AssertionError('missing docs returned success')
        except urllib.error.HTTPError as error:
            check('missing documentation returns 404', error.code == 404)
        status, _, headers = request('/auth/login', {'username': 'admin', 'password': os.environ['ARTIFACTORY_BOOTSTRAP_PASSWORD']})
        cookie = headers.get('Set-Cookie', '')
        check('HTTPS session uses Secure HttpOnly SameSite=Strict', status == 200 and all(flag in cookie for flag in ('Secure', 'HttpOnly', 'SameSite=Strict')))
        check('management endpoints require authentication', request('/overview', authenticated=False)[0] == 401)
        check('writes require CSRF', request('/projects', {}, csrf=False)[0] == 403)
        check('writes reject foreign Origin', request('/projects', {}, origin='https://unrelated.invalid')[0] == 403)
        check('unknown tenant cannot access project data', request('/projects', scope=str(uuid.uuid4()))[0] in (401, 403, 404))
        password = os.urandom(24).hex()
        credential = api('/credentials', {'name': 'Compose service fixture', 'kind': 'https', 'username': 'fixture', 'secret': password})['id']
        check('credential API redacts secret material', password not in json.dumps(api('/credentials')))
        project = api('/projects', {'name': 'Compose service fixture', 'repository_url': 'https://example.invalid/repo.git', 'credential_id': credential,})['id']
        check('tenant project metadata can be created without storing source', any(item['id'] == project for item in api('/projects')))
        (DIRECTORY / 'ready-for-restart').touch()
        wait(lambda: (DIRECTORY / 'control-plane-restarted').is_file())
        wait(ready)
        check('session and tenant metadata survive Control restart', any(item['id'] == project for item in api('/projects')))
        check('encrypted credential metadata survives restart', any(item['id'] == credential for item in api('/credentials')))
        evidence['passed'] = True
    except BaseException as error:
        evidence['failure'] = str(error)
        raise
    finally:
        (DIRECTORY / 'probe-evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')


if __name__ == '__main__':
    main()
