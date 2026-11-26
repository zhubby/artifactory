#!/usr/bin/env python3
"""[INPUT]: Python监督器持有未回收leader的OTA进程会话。
[OUTPUT]: 回收launcher及独立runtime组，读回整个会话为空。
[POS]: 普通OTA验收清理边界；只对未回收的自有会话发送信号。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import os
import signal
import subprocess
import time


def session_members(leader):
    result = subprocess.run(['ps','-axo','pid=,stat='],capture_output=True,text=True,
                            timeout=3,check=True)
    members = []
    for line in result.stdout.splitlines():
        fields = line.split()
        if len(fields)!=2 or fields[1].startswith('Z'):
            continue
        pid = int(fields[0])
        try:
            if os.getsid(pid)==leader:
                members.append(pid)
        except ProcessLookupError:
            pass
    return members


def close_launcher(child, graceful_seconds=15):
    if child.process.returncode is not None:
        return child.process.returncode
    leader = child.process.pid
    # 保留未wait的leader固定SID，先给launcher正常退出的预算。
    if not child.finished():
        os.kill(leader,signal.SIGINT)
    end = time.monotonic()+graceful_seconds
    while session_members(leader) and time.monotonic()<end:
        time.sleep(.05)
    if session_members(leader):
        # 未回收leader固定SID；每个目标在发信号前重验所属会话。
        end = time.monotonic()+5
        while members:=session_members(leader):
            for pid in members:
                try:
                    if os.getsid(pid)==leader:
                        os.kill(pid,signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if time.monotonic()>=end:
                raise RuntimeError('owned runtime session cleanup_unconfirmed')
            time.sleep(.05)
    return child.wait(3)


def close_launchers(children):
    failures = []
    for child in reversed(children):
        try:
            close_launcher(child)
        except Exception:
            failures.append(child.label)
    if failures:
        raise RuntimeError('owned runtime session cleanup_unconfirmed: '+','.join(failures))
