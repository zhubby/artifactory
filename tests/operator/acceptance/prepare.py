#!/usr/bin/env python3
"""[INPUT]: 工作区源码、目标架构和隔离集群镜像地址。
[OUTPUT]: 构建服务与双架构 RUN 夹具，只导入指定平台的运行镜像。
[POS]: 核对 kind 节点归属，不解包证明 manifest 或创建工作负载。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]

def run(args, cwd=ROOT, env=None):
    subprocess.run(args, cwd=cwd, env=env, check=True)


def load_kind_image(archive, cluster, platform):
    nodes = subprocess.check_output(['kind', 'get', 'nodes', '--name', cluster], text=True).split()
    if not nodes or len(nodes) != len(set(nodes)):
        raise RuntimeError('Expected distinct nodes in the selected kind cluster')
    for node in nodes:
        owner = subprocess.check_output(['docker', 'inspect', '--format',
            '{{ index .Config.Labels "io.x-k8s.kind.cluster" }}', node], text=True).strip()
        if owner != cluster:
            raise RuntimeError('Container does not belong to the selected kind cluster')
    for node in nodes:
        with archive.open('rb') as source:
            # kind 0.24 的全平台和 digest 别名会把无标签证明清单当作镜像解包。
            subprocess.run(['docker', 'exec', '--privileged', '-i', node, 'ctr', '--namespace=k8s.io',
                'images', 'import', '--platform', platform, '--snapshotter=overlayfs', '-'],
                stdin=source, cwd=ROOT, check=True)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--platform', default='linux/arm64', choices=['linux/arm64', 'linux/amd64'])
    parser.add_argument('--registry', default='localhost:5001/artifactory')
    parser.add_argument('--tag', default='rust-native')
    parser.add_argument('--rust-image', default='rust:1.95-bookworm')
    parser.add_argument('--base-image', default='debian:bookworm-slim')
    parser.add_argument('--worker-base', help='Optional Agent worker image using the current layout and identical pinned Pi dependencies')
    parser.add_argument('--skip-runtime-build', action='store_true', help='Reuse all previously built independent service and gateway images')
    parser.add_argument('--include-images', action='store_true', help='Build/import Image Operator, Runner, supervised BuildKit and Distribution')
    parser.add_argument('--kind', help='Load platform archives into this kind cluster (Docker CLI 28+, Engine API 1.48+)')
    args = parser.parse_args()
    directory = ROOT / '.local/operator-test'; directory.mkdir(parents=True, exist_ok=True)
    components = {'planning-operator':'operators/planning-operator/Dockerfile','source-preparer':'crates/source-preparer/Dockerfile','control-plane':'Dockerfile','emissary':'crates/emissary/Dockerfile','gateway':'crates/gateway/Dockerfile'}
    if args.include_images:
        components.update({'image-operator':'operators/image-operator/Dockerfile','image-worker':'operators/image-operator/Dockerfile.worker','buildkit':'operators/image-operator/Dockerfile.buildkit'})
    fixture = f'{args.registry}/operator-fixture:{args.tag}'
    pi = f'{args.registry}/planning-worker:{args.tag}'
    if not args.skip_runtime_build:
        for name, dockerfile in components.items():
            run(['docker', 'build', '--platform', args.platform, '-f', dockerfile, '--build-arg', 'RUST_IMAGE='+args.rust_image, '--build-arg', 'BASE_IMAGE='+args.base_image, '-t', f'{args.registry}/{name}:{args.tag}', '.'])
    environment = dict(os.environ, CGO_ENABLED='0', GOOS='linux', GOARCH=args.platform.split('/')[1])
    run(['go', 'build', '-o', str(directory/'fixture'), str(ROOT/'tests/operator/acceptance/fixture.go'), str(ROOT/'tests/operator/acceptance/live_provider.go')], env=environment)
    for architecture in ['arm64', 'amd64']:
        run(['go', 'build', '-trimpath', '-ldflags=-s -w', '-o', str(directory/('arch-check-'+architecture)), str(ROOT/'tests/images/arch-check.go')], env=dict(environment, GOARCH=architecture))
    run(['docker', 'build', '--platform', args.platform, '-f', str(ROOT/'tests/operator/acceptance/Dockerfile'), '--build-arg', 'BASE_IMAGE='+f'{args.registry}/emissary:{args.tag}', '-t', fixture, str(directory)])
    if args.worker_base:
        run(['npm', 'run', 'build'], cwd=ROOT/'packages/planning-worker')
        dockerfile = directory/'planning-worker.Dockerfile'
        shutil.copytree(ROOT/'packages/planning-worker/dist', directory/'dist', dirs_exist_ok=True)
        shutil.copytree(ROOT/'proto', directory/'proto', dirs_exist_ok=True)
        dockerfile.write_text('ARG BASE_IMAGE=node:24-bookworm-slim\nFROM ${BASE_IMAGE}\nWORKDIR /app/packages/planning-worker\nCOPY dist ./dist\nCOPY --chown=10001:10001 proto /app/proto\n')
        run(['docker', 'build', '--platform', args.platform, '-f', str(dockerfile), '--build-arg', 'BASE_IMAGE='+args.worker_base, '-t', pi, str(directory)])
    else:
        run(['docker', 'build', '--platform', args.platform, '-f', 'packages/planning-worker/Dockerfile', '-t', pi, '.'])
    dependencies = [('timescale/timescaledb:2.32.0-pg17', f'{args.registry}/timescaledb:2.32.0-pg17'), ('chrislusf/seaweedfs:4.53', f'{args.registry}/seaweedfs:4.53')]
    if args.include_images:
        dependencies.append(('registry:3.0.0', f'{args.registry}/distribution:3.0.0'))
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
            with tempfile.TemporaryDirectory(prefix='kind-', dir=directory) as temporary:
                archive = Path(temporary) / 'image.tar'
                run(['docker', 'image', 'save', '--platform', args.platform, '--output', str(archive), image])
                load_kind_image(archive, args.kind, args.platform)
        else:
            run(['docker', 'push', image])
