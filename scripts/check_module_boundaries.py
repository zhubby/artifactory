#!/usr/bin/env python3
"""[INPUT]: Cargo 工作区元数据与 supervisor 精简依赖图。
[OUTPUT]: 拒绝领域契约、基础设施和执行模块的反向依赖。
[POS]: CI 架构边界校验，防止镜像执行重新耦合节点服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--offline', '--no-deps', '--format-version', '1'],
        cwd=ROOT, text=True,
    ))
    packages = {p['name']: p for p in metadata['packages']}
    dependencies = {
        name: {d['name'] for d in package['dependencies'] if d['kind'] is None}
        for name, package in packages.items()
    }
    failures = []
    protocol = dependencies['artifactory-planning-protocol']
    if protocol - {'serde', 'serde_json', 'uuid'}:
        failures.append('planning-protocol must contain only serialization and identity dependencies')
    for name in ['artifactory-service-transport', 'artifactory-execution-support']:
        if any(d.startswith('artifactory-') for d in dependencies[name]):
            failures.append(name + ' must not depend on application or domain packages')
    pending = ['artifactory-image-operator']
    visited = set()
    while pending:
        name = pending.pop()
        if name in visited:
            continue
        visited.add(name)
        pending.extend(dependencies.get(name, set()) - visited)
    if 'artifactory-emissary' in visited:
        failures.append('image-operator must not depend on emissary, directly or transitively')
    supervisor = subprocess.check_output(
        ['cargo', 'tree', '--locked', '--offline', '-p', 'artifactory-image-operator',
         '--no-default-features', '--edges', 'normal', '--prefix', 'none', '--format', '{p}'],
        cwd=ROOT, text=True,
    )
    if any(line.startswith('artifactory-') and not line.startswith('artifactory-image-operator ')
           for line in supervisor.splitlines()):
        failures.append('BuildKit supervisor must exclude Operator business dependencies')
    for path in (ROOT / 'tests/operator').rglob('*.py'):
        source = path.read_text()
        if any(old in source for old in ["sql('planner',", 'sql("planner",', "datname='planner'"]):
            failures.append(str(path.relative_to(ROOT)) + ': acceptance must use the planning database')
    if failures:
        print('\n'.join(failures), file=sys.stderr)
        return 1
    print('Planning, transport, execution and supervisor dependency boundaries verified.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
