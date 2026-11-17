#!/usr/bin/env python3
"""[INPUT]: 隔离 Control 地址、私有对象源与共享网关令牌。
[OUTPUT]: 渲染最小网关环境并统一回收本机服务进程。
[POS]: 供本机构建、AI 和恢复验收复用入口边界。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import os
import socket
import subprocess


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def environment(config, bind, control):
    result = {key: value for key, value in os.environ.items() if key in ('PATH', 'HOME', 'LANG', 'TMPDIR', 'SSL_CERT_FILE')}
    result.update({
        'ARTIFACTORY_GATEWAY_PROFILE': 'all',
        'ARTIFACTORY_GATEWAY_HTTP_BIND': bind,
        'ARTIFACTORY_GATEWAY_CONTROL': control,
        'ARTIFACTORY_GATEWAY_REGISTRY': control,
        'ARTIFACTORY_GATEWAY_WEB': config.get('ARTIFACTORY_GATEWAY_WEB', 'http://127.0.0.1:3000'),
        'ARTIFACTORY_GATEWAY_TOKEN': config['ARTIFACTORY_GATEWAY_TOKEN'],
        'ARTIFACTORY_S3_ENDPOINT': config['ARTIFACTORY_S3_ENDPOINT'],
        'ARTIFACTORY_INSECURE_DEV': config.get('ARTIFACTORY_INSECURE_DEV', 'false'),
        'ARTIFACTORY_GATEWAY_ALLOW_HTTP_UPSTREAMS': 'true',
    })
    if config.get('ARTIFACTORY_SERVICE_CA'):
        result['ARTIFACTORY_SERVICE_CA'] = config['ARTIFACTORY_SERVICE_CA']
    return result


class Processes:
    """将验收入口视为一组可停止的进程。"""
    def __init__(self, *processes):
        self.processes = processes

    def poll(self):
        statuses = [process.poll() for process in self.processes]
        if any(status is None for status in statuses):
            return None
        return next((status for status in statuses if status != 0), 0)

    def terminate(self):
        for process in reversed(self.processes):
            if process.poll() is None:
                process.terminate()

    def kill(self):
        for process in reversed(self.processes):
            if process.poll() is None:
                process.kill()

    def wait(self, timeout=None):
        status = 0
        try:
            for process in reversed(self.processes):
                status = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.kill()
            for process in self.processes:
                process.wait(timeout=5)
            raise
        return status
