#!/usr/bin/env python3
"""[INPUT]: 工作区源码、目标架构和隔离集群镜像地址。
[OUTPUT]: 构建独立服务、网关、Pi 和设备夹具镜像。
[POS]: 仅构建并导入指定镜像，不创建集群工作负载。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
from pathlib import Path
import os
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[3]

def run(args, cwd=ROOT, env=None):
    subprocess.run(args, cwd=cwd, env=env, check=True)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--platform', default='linux/arm64', choices=['linux/arm64', 'linux/amd64'])
    parser.add_argument('--registry', default='localhost:5001/artifactory')
    parser.add_argument('--tag', default='rust-native')
    parser.add_argument('--rust-image', default='rust:1.95-bookworm')
    parser.add_argument('--base-image', default='debian:bookworm-slim')
    parser.add_argument('--pi-base', help='Optional Agent worker image using the current layout and identical pinned Pi dependencies')
    parser.add_argument('--skip-runtime-build', action='store_true', help='Reuse all previously built independent service and gateway images')
    parser.add_argument('--kind', help='Load into this kind cluster instead of pushing')
    args = parser.parse_args()
    directory = ROOT / '.local/operator-test'; directory.mkdir(parents=True, exist_ok=True)
    components = {'agent-operator':'operators/agent-operator/Dockerfile','planner-runtime':'crates/planner-runtime/Dockerfile','source-service':'crates/source-service/Dockerfile','control-plane':'Dockerfile','emissary':'crates/emissary/Dockerfile','gateway':'crates/gateway/Dockerfile'}
    fixture = f'{args.registry}/operator-fixture:{args.tag}'
    pi = f'{args.registry}/operator-pi:{args.tag}'
    if not args.skip_runtime_build:
        for name, dockerfile in components.items():
            run(['docker', 'build', '--platform', args.platform, '-f', dockerfile, '--build-arg', 'RUST_IMAGE='+args.rust_image, '--build-arg', 'BASE_IMAGE='+args.base_image, '-t', f'{args.registry}/{name}:{args.tag}', '.'])
    environment = dict(os.environ, CGO_ENABLED='0', GOOS='linux', GOARCH=args.platform.split('/')[1])
    run(['go', 'build', '-o', str(directory/'fixture'), str(ROOT/'tests/operator/acceptance/fixture.go'), str(ROOT/'tests/operator/acceptance/live_provider.go')], env=environment)
    run(['docker', 'build', '--platform', args.platform, '-f', str(ROOT/'tests/operator/acceptance/Dockerfile'), '--build-arg', 'BASE_IMAGE='+f'{args.registry}/emissary:{args.tag}', '-t', fixture, str(directory)])
    if args.pi_base:
        run(['npm', 'run', 'build'], cwd=ROOT/'packages/agent-worker')
        dockerfile = directory/'pi.Dockerfile'
        shutil.copytree(ROOT/'packages/agent-worker/dist', directory/'dist', dirs_exist_ok=True)
        shutil.copytree(ROOT/'proto', directory/'proto', dirs_exist_ok=True)
        dockerfile.write_text('ARG BASE_IMAGE=node:24-bookworm-slim\nFROM ${BASE_IMAGE}\nWORKDIR /app/packages/agent-worker\nCOPY dist ./dist\nCOPY --chown=10001:10001 proto /app/proto\n')
        run(['docker', 'build', '--platform', args.platform, '-f', str(dockerfile), '--build-arg', 'BASE_IMAGE='+args.pi_base, '-t', pi, str(directory)])
    else:
        run(['docker', 'build', '--platform', args.platform, '-f', 'packages/agent-worker/Dockerfile', '-t', pi, '.'])
    dependencies = [('timescale/timescaledb:2.32.0-pg17', f'{args.registry}/timescaledb:2.32.0-pg17'), ('chrislusf/seaweedfs:4.53', f'{args.registry}/seaweedfs:4.53')]
    for upstream, local in dependencies:
        existing = subprocess.run(['docker','image','inspect','--format','{{.Os}}/{{.Architecture}}',local],text=True,capture_output=True)
        if existing.returncode == 0 and existing.stdout.strip() == args.platform:
            continue
        run(['docker', 'pull', '--platform', args.platform, upstream])
        run(['docker', 'tag', upstream, local])
    images = [*[f'{args.registry}/{name}:{args.tag}' for name in components], fixture, pi, *[local for _, local in dependencies]]
    for image in images:
        actual = subprocess.check_output(['docker','image','inspect','--format','{{.Os}}/{{.Architecture}}',image], text=True).strip()
        if actual != args.platform:
            raise RuntimeError(f'Wrong architecture for {image}: {actual}')
        if args.kind:
            run(['kind', 'load', 'docker-image', '--name', args.kind, image])
        else:
            run(['docker', 'push', image])
