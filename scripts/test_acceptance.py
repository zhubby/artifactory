"""[INPUT]: 本机构建验收与可执行进程夹具。
[OUTPUT]: 验证启动回收、会话同源、租约终态和凭据隔离。
[POS]: 不连接数据库、Docker 或已有服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import http.server
import importlib.util
import json
import os
import signal
import socket
import subprocess
import sys
import tempfile
import threading
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


class SessionOriginTests(unittest.TestCase):
    def test_new_process_and_recovery_use_the_destination_origin(self):
        origins = []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                self.rfile.read(int(self.headers['Content-Length']))
                origins.append((self.headers.get('Origin'), self.headers.get('x-artifactory-csrf')))
                valid = origins[-1] == (self.server.public_url, '1')
                self.send_response(200 if valid else 403)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(json.dumps({'accepted': valid}).encode())

            def log_message(self, *_):
                pass

        with http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler) as initial, \
                http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler) as restored:
            threads = []
            for server in (initial, restored):
                server.public_url = f'http://127.0.0.1:{server.server_port}'
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                threads.append(thread)
            try:
                snippet = (
                    'import json, sys, acceptance\n'
                    'try:\n'
                    '    print(json.dumps(acceptance.request("/auth/login", {"username":"fixture","password":"fixture"})))\n'
                    '    acceptance.BASE=sys.argv[1]\n'
                    '    print(json.dumps(acceptance.request("/auth/login", {"username":"fixture","password":"fixture"})))\n'
                    'finally:\n'
                    '    acceptance.DIRECTORY.rmdir()\n'
                )
                environment = dict(os.environ, ARTIFACTORY_ACCEPTANCE_HTTP_URL=initial.public_url,
                                   ARTIFACTORY_PUBLIC_URL='http://127.0.0.1:8081')
                result = subprocess.run([sys.executable, '-c', snippet, restored.public_url],
                                        cwd=Path(__file__).parent, env=environment,
                                        capture_output=True, text=True, check=True, timeout=10)
                self.assertEqual([json.loads(line)[0] for line in result.stdout.splitlines()], [200, 200])
                self.assertEqual(origins, [(initial.public_url, '1'), (restored.public_url, '1')])
            finally:
                for server in (initial, restored):
                    server.shutdown()
                for thread in threads:
                    thread.join(timeout=5)


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


class PartitionLeaseTests(unittest.TestCase):
    def result(self, status):
        return {'id': 'fixture-build', 'status': status, 'artifacts': [], 'jobs': [
            {'target': 'native', 'status': status, 'attempts': [
                {'status': status, 'error': 'execution lease expired'}]}]}

    def assert_terminal(self, result, accepted):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.multiple(acceptance, DIRECTORY=Path(temporary), EVIDENCE={'checks': []}), \
                    patch.object(acceptance, 'api', return_value=result):
                if accepted:
                    self.assertIs(acceptance.terminal('fixture-build', 'lost or lease-expired failed',
                        validator=acceptance.partition_lease_expired), result)
                else:
                    with self.assertRaisesRegex(AssertionError, 'unexpected build result'):
                        acceptance.terminal('fixture-build', 'lost or lease-expired failed',
                            validator=acceptance.partition_lease_expired)

    def test_accepts_watchdog_loss_and_worker_lease_failure(self):
        for status in ('lost', 'failed'):
            with self.subTest(status=status):
                self.assert_terminal(self.result(status), accepted=True)

    def test_rejects_failures_without_exact_lease_expiry_reason(self):
        for status in ('lost', 'failed'):
            for error in ('checkout failed', 'execution lease expired or revoked', '', None):
                with self.subTest(status=status, error=error):
                    result = self.result(status)
                    result['jobs'][0]['attempts'][0]['error'] = error
                    self.assert_terminal(result, accepted=False)

    def test_rejects_retries_artifacts_and_inconsistent_terminal_states(self):
        for scenario in ('retry', 'extra-job', 'no-job', 'no-attempt', 'artifacts', 'missing-artifacts',
                         'job-status', 'attempt-status', 'succeeded', 'canceled'):
            with self.subTest(scenario=scenario):
                result = self.result('failed')
                job = result['jobs'][0]
                if scenario == 'retry':
                    job['attempts'].append(dict(job['attempts'][0]))
                elif scenario == 'extra-job':
                    result['jobs'].append(dict(job))
                elif scenario == 'no-job':
                    result['jobs'] = []
                elif scenario == 'no-attempt':
                    job['attempts'] = []
                elif scenario == 'artifacts':
                    result['artifacts'] = [{'id': 'unexpected-artifact'}]
                elif scenario == 'missing-artifacts':
                    del result['artifacts']
                elif scenario == 'job-status':
                    job['status'] = 'lost'
                elif scenario == 'attempt-status':
                    job['attempts'][0]['status'] = 'lost'
                else:
                    result = self.result(scenario)
                self.assert_terminal(result, accepted=False)

    def test_other_terminal_expectations_remain_strict(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.multiple(acceptance, DIRECTORY=Path(temporary), EVIDENCE={'checks': []}), \
                    patch.object(acceptance, 'api', return_value=self.result('failed')):
                with self.assertRaisesRegex(AssertionError, 'unexpected build result'):
                    acceptance.terminal('fixture-build', 'lost')


if __name__ == "__main__":
    unittest.main()
