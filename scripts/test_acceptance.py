"""[INPUT]: 本机构建验收与可执行进程夹具。
[OUTPUT]: 验证启动失败、成组回收和凭据隔离。
[POS]: 不连接数据库、Docker 或已有服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import importlib.util
import json
import os
import signal
import socket
import tempfile
import time
import unittest
import local_gateway
from pathlib import Path
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "acceptance", Path(__file__).with_name("acceptance.py")
)
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)


class ControlStartupTests(unittest.TestCase):
    def test_exited_control_reports_status_and_log_without_readiness_timeout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "target/debug/artifactory-control-plane"
            binary.parent.mkdir(parents=True)
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import sys\n"
                "print('fixture: control startup failed', file=sys.stderr, flush=True)\n"
                "sys.exit(23)\n"
            )
            binary.chmod(0o700)
            evidence = root / "evidence"
            evidence.mkdir()
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                port = listener.getsockname()[1]
            started = time.monotonic()
            with patch.multiple(
                acceptance,
                ROOT=root,
                DIRECTORY=evidence,
                BASE=f"http://127.0.0.1:{port}",
            ):
                with self.assertRaisesRegex(RuntimeError, r"control plane exited.*23") as failure:
                    acceptance.start_control()
            self.assertIn("fixture: control startup failed", str(failure.exception))
            self.assertLess(time.monotonic() - started, 5)

    def test_startup_timeout_terminates_control(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "target/debug/artifactory-control-plane"
            binary.parent.mkdir(parents=True)
            marker = root / "control.pid"
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import os, pathlib, time\n"
                f"pathlib.Path({str(marker)!r}).write_text(str(os.getpid()))\n"
                "time.sleep(120)\n"
            )
            binary.chmod(0o700)
            evidence = root / "evidence"
            evidence.mkdir()
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                port = listener.getsockname()[1]
            wait_for = acceptance.wait_for

            def wait_after_process_started(probe, timeout):
                wait_for(marker.exists, timeout=5)
                return wait_for(probe, timeout)

            try:
                with patch.multiple(acceptance, ROOT=root, DIRECTORY=evidence, BASE=f"http://127.0.0.1:{port}"), patch.object(acceptance, 'wait_for', side_effect=wait_after_process_started):
                    with self.assertRaises(TimeoutError):
                        acceptance.start_control(timeout=0.5)
                self.assertTrue(marker.exists(), "the control process must actually have started")
                with self.assertRaises(ProcessLookupError):
                    os.kill(int(marker.read_text()), 0)
            finally:
                if marker.exists():
                    try:
                        os.kill(int(marker.read_text()), signal.SIGKILL)
                    except ProcessLookupError:
                        pass

    def test_ready_control_remains_running_until_cleanup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "target/debug/artifactory-control-plane"
            binary.parent.mkdir(parents=True)
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                port = listener.getsockname()[1]
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import http.server, os\n"
                "class Handler(http.server.BaseHTTPRequestHandler):\n"
                "    def do_GET(self):\n"
                "        self.send_response(200 if self.path == '/healthz' else 404)\n"
                "        self.end_headers()\n"
                "host, port = os.environ['ARTIFACTORY_HTTP_BIND'].rsplit(':', 1)\n"
                "http.server.HTTPServer((host, int(port)), Handler).serve_forever()\n"
            )
            binary.chmod(0o700)
            gateway = binary.with_name('artifactory-gateway')
            gateway.write_text(binary.read_text().replace('ARTIFACTORY_HTTP_BIND', 'ARTIFACTORY_GATEWAY_HTTP_BIND'))
            gateway.chmod(0o700)
            evidence = root / "evidence"
            evidence.mkdir()
            process = None
            try:
                with patch.multiple(acceptance, ROOT=root, DIRECTORY=evidence, BASE=f"http://127.0.0.1:{port}"), patch.dict(os.environ, {'ARTIFACTORY_S3_ENDPOINT': 'http://127.0.0.1:1', 'ARTIFACTORY_INSECURE_DEV': 'true'}):
                    process = acceptance.start_control(timeout=5)
                self.assertIsNone(process.poll())
                self.assertEqual(len(process.processes), 2)
                self.assertTrue(all(child.poll() is None for child in process.processes))
            finally:
                if process is not None:
                    process.terminate()
                    process.wait(timeout=5)


class GatewayEnvironmentTests(unittest.TestCase):
    def test_gateway_environment_excludes_control_and_object_credentials(self):
        config = {'ARTIFACTORY_GATEWAY_TOKEN': 'gateway-fixture-token-' + 'x' * 32,
                  'ARTIFACTORY_S3_ENDPOINT': 'http://127.0.0.1:19000',
                  'ARTIFACTORY_MASTER_KEY': 'must-not-propagate',
                  'ARTIFACTORY_S3_SECRET_KEY': 'must-not-propagate',
                  'DATABASE_URL': 'must-not-propagate', 'ARTIFACTORY_INSECURE_DEV': 'true'}
        result = local_gateway.environment(config, '127.0.0.1:8081', 'http://127.0.0.1:8080')
        self.assertNotIn('must-not-propagate', result.values())
        self.assertEqual(result['ARTIFACTORY_GATEWAY_TOKEN'], config['ARTIFACTORY_GATEWAY_TOKEN'])
        self.assertEqual(result['ARTIFACTORY_GATEWAY_CONTROL'], 'http://127.0.0.1:8080')
        self.assertEqual(result['ARTIFACTORY_GATEWAY_WEB'], 'http://127.0.0.1:3000')

    def test_browser_ci_uses_the_public_gateway_origin(self):
        workflow = (Path(__file__).resolve().parents[1] / '.github/workflows/ci.yml').read_text()
        self.assertIn('export ARTIFACTORY_BROWSER_URL=http://127.0.0.1:8080', workflow)
        self.assertEqual(acceptance.BASE, 'http://127.0.0.1:8080')

    def test_configured_web_origin_is_preserved_for_ai_and_recovery(self):
        result = local_gateway.environment({'ARTIFACTORY_GATEWAY_TOKEN': 'fixture-token-' + 'x' * 32,
            'ARTIFACTORY_S3_ENDPOINT': 'https://private.example.invalid',
            'ARTIFACTORY_GATEWAY_WEB': 'http://127.0.0.1:39000'}, '127.0.0.1:38080', 'http://127.0.0.1:38082')
        self.assertEqual(result['ARTIFACTORY_GATEWAY_WEB'], 'http://127.0.0.1:39000')
        self.assertEqual(result['ARTIFACTORY_GATEWAY_CONTROL'], 'http://127.0.0.1:38082')


class BuildFailureDiagnosticsTests(unittest.TestCase):
    def test_unexpected_build_records_errors_without_attempt_credentials(self):
        result = {'id': 'fixture-build', 'status': 'failed', 'error': 'fixture source resolution failed', 'jobs': [{'target': 'native', 'attempts': [{'status': 'failed', 'error': 'checkout failed', 'lease_token': 'must-not-appear'}]}]}
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            with patch.multiple(acceptance, DIRECTORY=directory, EVIDENCE={'checks': []}), patch.object(acceptance, 'api', return_value=result):
                with self.assertRaisesRegex(AssertionError, 'fixture source resolution failed') as failure:
                    acceptance.terminal('fixture-build', 'succeeded')
            recorded = (directory / 'evidence.json').read_text()
            self.assertNotIn('must-not-appear', recorded + str(failure.exception))
            self.assertNotIn('lease_token', recorded + str(failure.exception))
            diagnostic = json.loads(recorded)['unexpected_build']
            self.assertEqual(diagnostic['actual'], 'failed')
            self.assertEqual(diagnostic['attempts'][0]['error'], 'checkout failed')


if __name__ == "__main__":
    unittest.main()
