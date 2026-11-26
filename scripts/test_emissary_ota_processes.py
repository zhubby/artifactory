#!/usr/bin/env python3
"""[INPUT]: 自有会话内launcher与独立runtime组的真实普通进程。
[OUTPUT]: 验证未reap清理、强制回收及多会话失败隔离。
[POS]: OTA驱动清理回归，不连接数据库或安装服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
from pathlib import Path
import os
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from check_emissary_legacy_sender import Supervisor
import emissary_ota_processes as processes


class RuntimeCleanup(unittest.TestCase):
    def test_exited_launcher_keeps_identity_until_independent_runtime_group_is_gone(self):
        with tempfile.TemporaryDirectory(prefix='ota-session-test-') as directory:
            root = Path(directory)
            supervisor = Supervisor()
            runtime = 'import signal,time; signal.signal(signal.SIGINT,signal.SIG_IGN); signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(60)'
            source = ('import pathlib,subprocess,sys; '
                      'child=subprocess.Popen([sys.executable,"-c",sys.argv[1]],process_group=0); '
                      'pathlib.Path(sys.argv[2]).write_text(str(child.pid))')
            child = supervisor.start([sys.executable,'-c',source,runtime,root/'runtime.pid'],
                cwd=root,env={'PATH':os.environ['PATH']},log=root/'launcher.log',label='runner-0')
            try:
                end = time.monotonic()+5
                while not child.finished() and time.monotonic()<end:
                    time.sleep(.02)
                self.assertTrue(child.finished())
                pid = int((root/'runtime.pid').read_text())
                self.assertIsNone(child.process.returncode)
                self.assertIn(pid,processes.session_members(child.process.pid))
                self.assertEqual(processes.close_launcher(child,graceful_seconds=0),0)
                self.assertTrue(child.confirmed)
                self.assertEqual(processes.session_members(child.process.pid),[])
            finally:
                processes.close_launcher(child,graceful_seconds=0)
                supervisor.cleanup()

    def test_cleanup_failure_does_not_skip_other_owned_launcher(self):
        class Named:
            def __init__(self,name): self.label=name
        children = [Named('first'),Named('second')]
        visited = []
        def close(child):
            visited.append(child.label)
            if child.label=='second':
                raise RuntimeError('injected snapshot failure')
        with patch.object(processes,'close_launcher',side_effect=close):
            with self.assertRaisesRegex(RuntimeError,'second'):
                processes.close_launchers(children)
        self.assertEqual(visited,['second','first'])

    def test_reaped_launcher_is_never_looked_up_or_signaled_again(self):
        class Process:
            returncode = 0
        class Child:
            process = Process()
        with patch.object(processes,'session_members',side_effect=AssertionError('old SID queried')):
            self.assertEqual(processes.close_launcher(Child()),0)


if __name__ == '__main__':
    unittest.main()
