#!/usr/bin/env python3
"""[INPUT]: 依赖独立 PostgreSQL 和 Runtime 准入测试。
[OUTPUT]: 写入一万个排队运行的准入结果与日志。
[POS]: 验证数据库准入，不推断活动 Pod 容量。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
import os
from pathlib import Path
import subprocess
import time

root=Path(__file__).resolve().parents[3]
if not os.environ.get('ARTIFACTORY_TEST_DATABASE_URL'):
    raise SystemExit('Set ARTIFACTORY_TEST_DATABASE_URL to a disposable PostgreSQL database')
directory=root/'.local/operator-load'/time.strftime('%Y%m%d-%H%M%S')
directory.mkdir(parents=True,exist_ok=False)
started=time.monotonic()
result=subprocess.run(['cargo','test','--locked','-p','artifactory-planning-coordinator','--test','database','ten_thousand_queued_runs','--','--include-ignored'],cwd=root,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
(directory/'test.log').write_text(result.stdout)
evidence={'scenario':'10,000 queued, pool 8, each project 2, concurrent dispatcher calls','elapsed_seconds':round(time.monotonic()-started,3),'passed':result.returncode==0,'active_pod_capacity_tested':False}
(directory/'evidence.json').write_text(json.dumps(evidence,indent=2))
print(json.dumps(evidence,ensure_ascii=False));print(directory/'evidence.json')
raise SystemExit(result.returncode)
