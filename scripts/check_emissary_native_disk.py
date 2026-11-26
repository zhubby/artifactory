#!/usr/bin/env python3
"""[INPUT]: 专用Linux宿主、私有TimescaleDB与原生服务故障目标。
[OUTPUT]: 私有ext4卷运行系统服务，验证满盘、事务补报和精确清理。
[POS]: Linux原生磁盘门禁；拒绝覆盖既有服务和账户。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import sys
import tempfile
import time
import uuid

import check_emissary_legacy_receiver as legacy
import check_emissary_ota_filesystem as filesystem
import emissary_linux_filesystem as linux
from check_emissary_legacy_sender import Supervisor, clean_environment, compile_test, private_json

ROOT = Path(__file__).resolve().parents[1]


def service_absent():
    result = subprocess.run(["systemctl", "show", "artifactory-emissary.service",
                             "-p", "LoadState", "-p", "ActiveState"],
                            capture_output=True, text=True, timeout=10, check=True)
    state = dict(row.split("=", 1) for row in result.stdout.splitlines() if "=" in row)
    return state == {"LoadState": "not-found", "ActiveState": "inactive"}


def account_absent():
    rows = Path("/etc/passwd").read_text().splitlines()
    if not rows or any(len(row.split(":")) != 7 for row in rows):
        raise RuntimeError("local account inventory invalid")
    return not any(row.split(":", 1)[0] == "artifactory-ci" for row in rows)


def validate_proof(proof):
    required = ("passed", "native_service_executed", "real_business_migrations",
                "transaction_fence_observed", "source_bytes_retained_before_commit",
                "business_ack_removed_sources", "terminal_operation_unchanged",
                "service_account_directory_removed", "database_removed",
                "resumed_sample_business_ack_verified")
    if any(proof.get(key) is not True for key in required):
        raise RuntimeError("native disk transaction or cleanup evidence incomplete")
    if proof.get("source_batches") != 2 or proof.get("source_inventories") != 3:
        raise RuntimeError("native disk original source matrix incomplete")
    if type(proof.get("native_startups_registered")) is not int or proof["native_startups_registered"] < 4:
        raise RuntimeError("native maintenance startup coverage incomplete")
    uuid.UUID(proof["resumed_sample_batch_id"])
    disk = proof.get("native_disk_fault", {})
    required = ("passed", "native_service_executed", "actual_enospc",
                "one_byte_write_enospc", "repeated_collection_failures_observed",
                "service_pid_unchanged", "runtime_session_unchanged",
                "launcher_instance_unchanged", "identity_and_permissions_unchanged",
                "original_sources_retained", "same_service_resumed_after_space_release",
                "health_confirmation_resumed")
    if any(disk.get(key) is not True for key in required):
        raise RuntimeError("native service continuity evidence incomplete")
    if type(disk.get("observed_disk_errors")) is not int or disk["observed_disk_errors"] < 2:
        raise RuntimeError("native repeated disk failures missing")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--database-container")
    args = parser.parse_args()
    if sys.platform != "linux":
        raise RuntimeError("native disk gate requires Linux")
    if os.environ.get("ARTIFACTORY_NATIVE_SERVICE_TESTS") != "1":
        raise RuntimeError("explicit native service authorization required")
    if not service_absent() or not account_absent():
        raise RuntimeError("refusing existing native service or test account")
    url = legacy.database_url(args.database_container)
    directory = Path(tempfile.mkdtemp(prefix="emissary-ota-fs-", dir="/tmp"))
    # 服务账户只穿过父目录；证书、配置和日志仍保留私有权限。
    directory.chmod(0o755)
    nonce = uuid.uuid4().hex
    (directory / "nonce").write_text(nonce)
    database = "durable_receiver_" + uuid.uuid4().hex
    supervisor = Supervisor()
    evidence = {"status": "failed", "boundary": "Linux systemd service on private ext4; no physical power loss",
                "diagnostic_directory": str(directory)}
    reserved = False
    previous = signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    started = time.monotonic()
    try:
        source = legacy.source_manifest()
        evidence["source_manifest_sha256"] = source
        target = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()
        native = compile_test(supervisor, ROOT, target, "native_service", directory / "native-build.log", 900)
        utility = compile_test(supervisor, ROOT, target, "ota_filesystem_faults", directory / "utility-build.log", 900)
        evidence["test_binary_sha256"] = hashlib.sha256(native.read_bytes()).hexdigest()
        filesystem.invoke(supervisor, utility, directory, nonce, "reserve_database", database=database, url=url)
        reservation = json.loads((directory / "reserved.json").read_text())
        if reservation != {"name": database, "nonce": nonce, "absent_before_run": True}:
            raise RuntimeError("native disk reservation differs")
        reserved = True
        linux.create(directory, size_mib=768)
        evidence["filesystem"] = linux.mount(directory)
        (directory / "mount/owner").write_text(nonce)
        env = clean_environment()
        arch = {"aarch64": "arm64", "x86_64": "amd64"}[platform.machine()]
        env.update(ARTIFACTORY_NATIVE_EXPECTED_PLATFORM=f"linux/{arch}",
                   ARTIFACTORY_NATIVE_FAULT_MOUNT=str(directory / "mount"),
                   ARTIFACTORY_NATIVE_FAULT_NONCE=nonce,
                   ARTIFACTORY_NATIVE_DISK_DATABASE=database,
                   ARTIFACTORY_NATIVE_EVIDENCE_DIR=str(directory / "evidence"),
                   ARTIFACTORY_TEST_DATABASE_URL=url)
        child = supervisor.start([native, "native_service_disk_recovery", "--ignored", "--exact", "--nocapture"],
                                 cwd=ROOT, env=env, log=directory / "native-disk.log", label="native-disk")
        if child.wait(930):
            # 只输出断言和Cause，避免把私有服务环境或凭据带入CI日志。
            lines = (directory / "native-disk.log").read_text(errors="replace").splitlines()
            diagnostics = []
            for index, line in enumerate(lines):
                if "panicked at" in line:
                    diagnostics.extend(lines[index:index + 4])
            for line in diagnostics[:12]:
                if not any(marker in line.lower() for marker in ("postgres://", "postgresql://", "password", "secret", "token")):
                    print(line[:600], file=sys.stderr, flush=True)
            raise RuntimeError("native disk test failed; inspect private log")
        proof = json.loads((directory / "evidence/durable_recovery.json").read_text())
        validate_proof(proof)
        evidence["native_recovery"] = proof
        if legacy.source_manifest() != source:
            raise RuntimeError("source changed during native disk acceptance")
        evidence["work_passed"] = True
    except (Exception, KeyboardInterrupt) as error:
        evidence.update(error_type=type(error).__name__, error=str(error))
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        prior_int = signal.signal(signal.SIGINT, signal.SIG_IGN)
        failures = []
        try:
            supervisor.cleanup()
            if not supervisor.cleanup_confirmed():
                raise RuntimeError("owned child process cleanup unconfirmed")
            evidence["process_cleanup_confirmed"] = True
            if reserved:
                filesystem.invoke(supervisor, utility, directory, nonce, "cleanup_database", database=database, url=url)
                evidence["database_removed"] = True
            if not service_absent() or not account_absent():
                raise RuntimeError("service or account cleanup unconfirmed; recovery image retained")
            if list((directory / "mount").glob("emissary-service-*")):
                raise RuntimeError("native service directory remains; recovery image retained")
            evidence["service_account_directory_removed"] = True
            linux.detach(directory)
            evidence["owned_image_detached"] = True
            (directory / "volume.dmg").unlink(missing_ok=True)
            evidence["owned_image_removed"] = True
        except Exception as error:
            failures.append(str(error))
        evidence["elapsed_seconds"] = time.monotonic() - started
        evidence["cleanup_status"] = "unconfirmed" if failures else "confirmed"
        if failures:
            evidence["cleanup_errors"] = failures
        elif evidence.get("work_passed"):
            evidence["status"] = "passed"
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        private_json(args.evidence, evidence)
        signal.signal(signal.SIGTERM, previous)
        signal.signal(signal.SIGINT, prior_int)
    print(f"Native disk acceptance: {evidence['status']}; evidence: {args.evidence}")
    print(f"Private diagnostic directory: {directory}")
    return 0 if evidence["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
