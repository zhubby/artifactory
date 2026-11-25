"""
[INPUT]: 依赖安装脚本、临时目录及隔离系统命令桩。
[OUTPUT]: 验证安装前校验、身份保护、服务身份与凭证清理。
[POS]: 首次安装脚本的无宿主修改行为验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import base64
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("installer.sh")
CA = b"-----BEGIN CERTIFICATE-----\nfixture\n-----END CERTIFICATE-----\n"
BINARY = b'''#!/bin/sh
printf '%s|%s|%s\n' "$*" "${ARTIFACTORY_ENROLLMENT_TOKEN-unset}" "${FAKE_UID-unset}" >> "$INSTALL_TRACE"
case "$1" in
  register)
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in --config) config=$2; shift 2;; --root) root=$2; shift 2;; *) shift;; esac
    done
    printf identity > "$config"
    mkdir -p "$root"
    printf key > "$root/node.key"
    ;;
esac
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.stubs = self.root / "commands"
        self.stubs.mkdir()
        self.data = self.root / "state with spaces"
        self.bin = self.root / "program with spaces"
        self.binary = self.root / "asset"
        self.binary.write_bytes(BINARY)
        self.trace = self.root / "trace"
        self.network = self.root / "network"
        self.env = dict(os.environ, PATH=f"{self.stubs}:{os.environ['PATH']}",
                        ARTIFACTORY_ENROLLMENT_TOKEN="short-lived-fixture",
                        INSTALL_TRACE=str(self.trace), INSTALL_NETWORK=str(self.network),
                        INSTALL_ASSET=str(self.binary))
        self.stub("uname", "case \"$1\" in -s) echo Linux;; -m) echo x86_64;; esac")
        self.stub("id", "case \"$1\" in -u) if [ \"${2-}\" = builder ]; then echo 1001; else echo 1000; fi;; -g) echo 1001;; -gn) echo builders;; -Gn) echo builders;; esac")
        self.stub("sudo", "if [ \"${1-}\" = -v ]; then exit 0; fi\nif [ \"${1-}\" = -H ]; then shift; fi\nif [ \"${1-}\" = -u ]; then export FAKE_UID=1001; shift 2; fi\nif [ \"${1-}\" = -- ]; then shift; fi\nexec \"$@\"")
        self.stub("curl", "printf request >> \"$INSTALL_NETWORK\"\nwhile [ \"$#\" -gt 0 ]; do\n case \"$1\" in --output) output=$2; shift 2;; *) shift;; esac\ndone\ncp \"$INSTALL_ASSET\" \"$output\"\nprintf 200")
        self.stub("openssl", f"if [ \"${{1-}}\" = x509 ]; then exit 0; fi\nexec '{shutil.which('openssl')}' \"$@\"")
        self.stub("chown", "exit 0")
        self.stub("systemctl", "exit 1")

    def tearDown(self):
        self.temp.cleanup()

    def stub(self, name, body):
        path = self.stubs / name
        path.write_text("#!/bin/sh\n" + body + "\n")
        path.chmod(0o755)

    def command(self, **overrides):
        values = {"download-url": "https://build.example.com/artifacts/object?ticket=fixture",
                  "sha256": hashlib.sha256(BINARY).hexdigest(), "size": str(len(BINARY)),
                  "endpoint": "agents.example.com:31443", "ca-base64": base64.b64encode(CA).decode(),
                  "ca-sha256": hashlib.sha256(CA).hexdigest(),
                  "public-keys": base64.b64encode(bytes(32)).decode(), "os": "linux", "arch": "amd64",
                  "user": "builder", "directory": str(self.data), "binary-directory": str(self.bin)}
        values.update(overrides)
        return ["sh", str(SCRIPT)] + [part for k, v in values.items() for part in (f"--{k}", v)]

    def run_install(self, **overrides):
        self.assertTrue(SCRIPT.exists(), "Unix installer has not been implemented")
        return subprocess.run(self.command(**overrides), env=self.env, capture_output=True, text=True)

    def test_platform_mismatch_stops_before_download(self):
        result = self.run_install(os="darwin")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("platform", result.stderr.lower())
        self.assertFalse(self.network.exists())

    def test_bad_digest_never_executes_binary(self):
        result = self.run_install(sha256="0" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256", result.stderr)
        self.assertFalse(self.trace.exists())
        self.assertFalse(self.data.exists())

    def test_bad_size_never_executes_binary(self):
        result = self.run_install(size="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.trace.exists())

    def test_existing_identity_survives_without_download(self):
        self.data.mkdir()
        identity = self.data / "emissary.json"
        identity.write_text("existing identity")
        result = self.run_install()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(identity.read_text(), "existing identity")
        self.assertFalse(self.network.exists())
        self.assertFalse(self.trace.exists())

    def test_missing_token_stops_before_download(self):
        del self.env["ARTIFACTORY_ENROLLMENT_TOKEN"]
        result = self.run_install()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.network.exists())

    def test_bad_ca_stops_before_download(self):
        result = self.run_install(**{"ca-sha256": "0" * 64})
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.network.exists())

    def test_http_and_malformed_endpoint_stop_before_download(self):
        for values in ({"download-url": "http://build.example.com/asset"},
                       {"endpoint": "agents.example.com:31443;bad"}):
            result = self.run_install(**values)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.network.exists())

    def test_privileged_service_account_is_rejected(self):
        self.stub("id", "case \"$1\" in -u) echo 1001;; -Gn) echo 'builders sudo';; -gn) echo builders;; esac")
        result = self.run_install()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.network.exists())

    def test_success_uses_service_identity_and_clears_token(self):
        result = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.trace.read_text().splitlines()
        self.assertTrue(calls[0].startswith("register "))
        self.assertTrue(calls[0].endswith("|short-lived-fixture|1001"))
        for call in calls[1:]:
            self.assertIn("|unset|", call)
        self.assertTrue(any(call.startswith("doctor ") and call.endswith("|1001") for call in calls))
        self.assertTrue(any(call.startswith("ota-init ") and call.endswith("|1001") for call in calls))
        self.assertTrue(any(call.startswith("service --action install ") for call in calls))
        self.assertTrue(any(call.startswith("service --action start ") for call in calls))
        self.assertNotIn("short-lived-fixture", result.stdout + result.stderr)
        self.assertEqual((self.data / "emissary.json").read_text(), "identity")

    def test_ipv6_endpoint_is_passed_as_a_single_argument(self):
        result = self.run_install(endpoint="[2001:db8::1]:31443")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--endpoint [2001:db8::1]:31443", self.trace.read_text())

    def test_failed_enrollment_preserves_identity_and_removes_secret_file(self):
        self.binary.write_bytes(BINARY.replace(b'printf key > "$root/node.key"', b'printf key > "$root/node.key"\n    exit 1'))
        body = self.binary.read_bytes()
        result = self.run_install(sha256=hashlib.sha256(body).hexdigest(), size=str(len(body)))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.data / "emissary.json").read_text(), "identity")
        self.assertFalse((self.data / ".enrollment-token").exists())
        self.assertEqual(len(self.trace.read_text().splitlines()), 1)

    def test_symlink_parent_is_rejected_before_download(self):
        actual = self.root / "actual"
        actual.mkdir()
        link = self.root / "linked"
        link.symlink_to(actual)
        result = self.run_install(directory=str(link / "state"))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.network.exists())
        self.assertFalse((actual / "state").exists())


if __name__ == "__main__":
    unittest.main()
