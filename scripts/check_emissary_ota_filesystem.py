#!/usr/bin/env python3
"""[INPUT]: 平台磁盘镜像工具、文件系统读回与真实OTA测试。
[OUTPUT]: 创建HFS+/APFS/ext4私有卷，验证故障、队列恢复及精确清理。
[POS]: 普通进程故障门禁；只修改自有临时镜像，不安装服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import uuid

from check_emissary_legacy_sender import Supervisor, clean_environment, compile_test, private_json
from check_emissary_legacy_receiver import source_manifest
import check_emissary_legacy_receiver as legacy

ROOT = Path(__file__).resolve().parents[1]
TEST = "actual_filesystem_faults_preserve_original_assets"


def command(args):
    return subprocess.run(args, stdin=subprocess.DEVNULL, capture_output=True,
                          timeout=45, check=True).stdout


def owned_images(directory):
    info = plistlib.loads(command(["hdiutil", "info", "-plist"]))
    image = (directory / "volume.dmg").resolve()
    return [entry for entry in info.get("images", [])
            if Path(entry.get("image-path", "/missing")).resolve() == image]


def filesystem_proof(directory, expected):
    info = plistlib.loads(command(["diskutil", "info", "-plist", str(directory / "mount")]))
    expected_type = {"HFS+": "hfs", "APFS": "apfs"}[expected]
    if info.get("FilesystemType") != expected_type or not (directory / "mount").is_mount():
        raise RuntimeError("mounted filesystem type differs")
    if Path(info.get("MountPoint", "/missing")).resolve() != (directory / "mount").resolve():
        raise RuntimeError("filesystem mount identity differs")
    return {"requested": expected, "observed_type": expected_type, "mount_verified": True}


def mount(directory, *, readonly=False, filesystem="HFS+"):
    if sys.platform == "linux":
        import emissary_linux_filesystem as linux
        return linux.mount(directory, readonly=readonly)
    if owned_images(directory):
        raise RuntimeError("owned image already attached")
    args = ["hdiutil", "attach", "-nobrowse", "-noverify", "-mountpoint",
            str(directory / "mount"), "-plist"]
    if readonly:
        args.append("-readonly")
    command([*args, str(directory / "volume.dmg")])
    images = owned_images(directory)
    if len(images) != 1:
        raise RuntimeError("owned attachment not uniquely identified")
    mountpoint = (directory / "mount").resolve()
    entries = [entry for entry in images[0].get("system-entities", [])
               if "mount-point" in entry and Path(entry["mount-point"]).resolve() == mountpoint]
    if len(entries) != 1 or not mountpoint.is_mount():
        raise RuntimeError("owned mount point differs")
    return filesystem_proof(directory, filesystem)


def detach(directory):
    if sys.platform == "linux":
        import emissary_linux_filesystem as linux
        return linux.detach(directory)
    images = owned_images(directory)
    if not images:
        if (directory / "mount").is_mount():
            raise RuntimeError("mount remains without owned image")
        return
    if len(images) != 1:
        raise RuntimeError("owned image attachment ambiguous")
    entries = images[0].get("system-entities", [])
    devices = [entry["dev-entry"] for entry in entries if "dev-entry" in entry]
    if not devices or not all(device.startswith("/dev/disk") for device in devices):
        raise RuntimeError("owned device identity missing")
    command(["hdiutil", "detach", devices[0]])
    if owned_images(directory) or (directory / "mount").is_mount():
        raise RuntimeError("owned image detach not confirmed")


def invoke(supervisor, binary, directory, nonce, case, *, database=None, url=None, active=False):
    env = clean_environment()
    env.update(ARTIFACTORY_OTA_FAULT_DIR=str(directory), ARTIFACTORY_OTA_FAULT_CASE=case)
    if database:
        env["ARTIFACTORY_OTA_FAULT_DATABASE"] = database
    if url:
        env["ARTIFACTORY_TEST_DATABASE_URL"] = url
    if active:
        env["ARTIFACTORY_OTA_FAULT_ACTIVE_SPOOL"] = "1"
    child = supervisor.start([binary, "--ignored", "--exact", TEST, "--nocapture"],
                             cwd=ROOT, env=env, log=directory / f"{case}.log", label=case)
    if child.wait(330 if case.startswith("recover_") else 60):
        raise RuntimeError(f"{case} failed; inspect private log")
    proof = json.loads((directory / f"{case}.json").read_text())
    validate_proof(proof, nonce, case)
    if active:
        validate_active_collection(proof.get("recovery", {}).get("active_collection"))
    if database and case in ("low_space", "after_precheck", "supervisor_enospc", "readonly"):
        if proof.get("real_source_records_preserved") is not True:
            raise RuntimeError("original spool preservation missing")
    return proof


def validate_proof(proof, nonce, case):
    if proof.get("passed") is not True or proof.get("nonce") != nonce:
        raise RuntimeError("filesystem proof identity differs")
    if case.startswith("recover_"):
        validate_recovery(proof.get("recovery", {}), case.removeprefix("recover_"))
    elif case == "supervisor_enospc":
        validate_supervisor(proof)
    elif case == "seed_spool":
        validate_queues(proof.get("queue_records"))
    elif case not in ("prepare_readonly", "seed_spool", "reserve_database", "cleanup_database"):
        if proof.get("case") != case:
            raise RuntimeError("filesystem case differs")
        required = ("actual_mounted_filesystem", "state_and_protected_assets_preserved",
                    "unconfirmed_data_sentinel_preserved", "target_not_installed", "partial_files_removed")
        if any(proof.get(key) is not True for key in required):
            raise RuntimeError("filesystem preservation proof incomplete")
        if proof.get("native_service_executed") is not False:
            raise RuntimeError("filesystem proof boundary differs")
        if case == "after_precheck" and proof.get("actual_enospc") is not True:
            raise RuntimeError("real ENOSPC proof missing")
        if case == "after_precheck" and type(proof.get("terminal_progress_persisted")) is not bool:
            raise RuntimeError("ENOSPC progress persistence observation missing")


def validate_supervisor(proof):
    if proof.get("case") != "supervisor_enospc" or proof.get("native_service_executed") is not False:
        raise RuntimeError("supervisor filesystem boundary differs")
    observation = proof.get("supervisor")
    required = ("one_byte_write_enospc", "real_supervisor",
                "download_failed_after_first_persisted_byte", "current_runtime_kept_running",
                "pending_state_protection_checked",
                "failed_state_saved_after_space_release", "protection_released_after_recovery",
                "target_not_installed", "original_asset_and_source_preserved", "normal_stop_confirmed")
    if not isinstance(observation, dict) or any(observation.get(key) is not True for key in required):
        raise RuntimeError("supervisor ENOSPC proof incomplete")
    if any(type(observation.get(key)) is not bool for key in ("state_write_probe_enospc", "terminal_saved_while_full")):
        raise RuntimeError("supervisor state persistence observation missing")
    free = observation.get("free_bytes_at_failure")
    if type(free) is not int or not 0 <= free <= 200 << 20:
        raise RuntimeError("supervisor ENOSPC free bytes invalid")


def validate_recovery(proof, case):
    required = ("passed", "real_business_migrations", "transaction_fence_observed",
                "fresh_pinned_by_real_sender",
                "source_bytes_retained_before_commit", "business_ack_removed_sources",
                "original_ids_digests_times_verified", "terminal_operation_unchanged",
                "identity_configuration_unchanged", "database_removed")
    if any(proof.get(key) is not True for key in required):
        raise RuntimeError("spool recovery proof incomplete")
    if proof.get("case") != case or proof.get("native_service_executed") is not False:
        raise RuntimeError("spool recovery identity or boundary differs")
    if proof.get("authorization_acl_proven") is not False:
        raise RuntimeError("spool recovery ACL boundary differs")
    if type(proof.get("source_batches")) is not int or proof["source_batches"] != 2:
        raise RuntimeError("original batch count differs")
    if type(proof.get("source_inventories")) is not int or proof["source_inventories"] != 3:
        raise RuntimeError("original inventory count differs")
    validate_queues(proof.get("queue_records"))


def validate_queues(records):
    expected = {"history_batch", "ordinary_batch", "legacy_inventory", "startup_inventory",
                "refresh_inventory", "terminal_operation"}
    if not isinstance(records, list) or len(records) != len(expected):
        raise RuntimeError("original queue categories missing")
    seen = set()
    for group in records:
        if not isinstance(group, dict) or group.get("category") not in expected or group["category"] in seen:
            raise RuntimeError("original queue category differs")
        seen.add(group["category"])
        if type(group.get("count")) is not int or group["count"] != 1:
            raise RuntimeError("original queue category count differs")
        values = group.get("records")
        if not isinstance(values, list) or len(values) != 1 or not isinstance(values[0], dict):
            raise RuntimeError("original queue digest missing")
        value = values[0]
        if type(value.get("bytes")) is not int or not 0 < value["bytes"] <= 1 << 20:
            raise RuntimeError("original queue bytes invalid")
        if not isinstance(value.get("sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", value["sha256"]):
            raise RuntimeError("original queue digest invalid")


def validate_active_collection(proof):
    if not isinstance(proof, dict):
        raise RuntimeError("active collector proof missing")
    required = ("passed", "actual_enospc", "one_byte_write_enospc", "repeated_collection_failures_observed",
                "original_sources_retained", "same_handle_resumed_after_space_release",
                "disk_counters_survive_reopen", "resumed_sample_business_ack_verified",
                "resumed_sample_source_removed")
    if any(proof.get(key) is not True for key in required):
        raise RuntimeError("active collector proof incomplete")
    if type(proof.get("observed_disk_errors")) is not int or proof["observed_disk_errors"] < 2:
        raise RuntimeError("active collector repeated disk errors missing")
    total = proof.get("filesystem_total_bytes")
    if type(total) is not int or not 100 << 20 <= total <= 200 << 20:
        raise RuntimeError("active collector private filesystem size differs")
    if type(proof.get("free_bytes_at_failure")) is not int or not 0 <= proof["free_bytes_at_failure"] <= total:
        raise RuntimeError("active collector pressure differs")
    if proof.get("native_service_executed") is not False:
        raise RuntimeError("active collector boundary differs")
    try:
        uuid.UUID(proof["resumed_batch_id"])
    except (KeyError, ValueError, TypeError, AttributeError) as error:
        raise RuntimeError("active collector resumed batch identity missing") from error


def cleanup_owned(supervisor, directory, evidence, *, database_cleanup=None):
    failures = []
    try:
        supervisor.cleanup()
    except Exception:
        failures.append("owned_processes")
    confirmed = not failures and supervisor.cleanup_confirmed()
    evidence["process_cleanup_confirmed"] = confirmed
    if not confirmed:
        failures.append("owned_process_cleanup_unconfirmed")
        evidence["recovery_image_retained"] = (directory / "volume.dmg").exists()
        return failures
    if database_cleanup:
        try:
            database_cleanup()
            evidence["database_removed"] = True
        except Exception:
            failures.append("owned_database_cleanup_unconfirmed")
            evidence["process_cleanup_confirmed"] = supervisor.cleanup_confirmed()
            evidence["recovery_image_retained"] = (directory / "volume.dmg").exists()
            return failures
    try:
        detach(directory)
        evidence["owned_image_detached"] = True
        image = directory / "volume.dmg"
        if image.exists():
            image.unlink()
        evidence["owned_image_removed"] = not image.exists()
    except Exception:
        failures.append("owned_image")
        evidence["recovery_image_retained"] = (directory / "volume.dmg").exists()
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, default=Path("/tmp/emissary-ota-filesystem-evidence.json"))
    parser.add_argument("--with-spool", action="store_true", help="retain real source queues through faults and verify transactional recovery")
    parser.add_argument("--with-active-spool", action="store_true", help="also keep real collection running through ENOSPC and space recovery")
    parser.add_argument("--database-container", help="explicit existing local TimescaleDB container for spool recovery")
    parser.add_argument("--filesystem", choices=("HFS+", "APFS", "ext4"),
                        default="ext4" if sys.platform == "linux" else "HFS+",
                        help="filesystem of the private 160MiB image")
    args = parser.parse_args()
    args.with_spool = args.with_spool or args.with_active_spool
    if sys.platform not in ("darwin", "linux"):
        raise RuntimeError("filesystem driver requires macOS or Linux private images")
    if (sys.platform == "linux") != (args.filesystem == "ext4"):
        raise RuntimeError("filesystem does not match the host platform")
    url = legacy.database_url(args.database_container) if args.with_spool else None
    directory = Path(tempfile.mkdtemp(prefix="emissary-ota-fs-", dir="/tmp"))
    os.chmod(directory, 0o700)
    nonce = uuid.uuid4().hex
    (directory / "nonce").write_text(nonce)
    supervisor = Supervisor()
    database = "durable_receiver_" + uuid.uuid4().hex if args.with_spool else None
    database_reserved = False
    evidence = {"status": "failed", "boundary": f"ordinary {sys.platform} {args.filesystem} image, no native service/deployment"}
    evidence["platform"] = {"os": sys.platform, "machine": os.uname().machine}
    previous = signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        evidence["source_manifest_sha256"] = source_manifest()
        target = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()
        binary = compile_test(supervisor, ROOT, target, "ota_filesystem_faults", directory / "build.log", 900)
        evidence["test_binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        if database:
            invoke(supervisor, binary, directory, nonce, "reserve_database", database=database, url=url)
            if json.loads((directory / "reserved.json").read_text()) != {"name": database, "nonce": nonce, "absent_before_run": True}:
                raise RuntimeError("owned database reservation differs")
            database_reserved = True
        if sys.platform == "linux":
            import emissary_linux_filesystem as linux
            linux.create(directory)
        else:
            command(["hdiutil", "create", "-size", "160m", "-fs", args.filesystem, "-volname",
                     "EmissaryOtaFault", str(directory / "volume.dmg")])
        evidence["filesystem"] = mount(directory, filesystem=args.filesystem)
        (directory / "mount/owner").write_text(nonce)
        evidence["cases"] = {}
        if database:
            evidence["seed_spool"] = invoke(supervisor, binary, directory, nonce, "seed_spool", database=database)
        for case in ("low_space", "after_precheck", "supervisor_enospc", "prepare_readonly"):
            if (directory / "mount/ota").exists():
                shutil.rmtree(directory / "mount/ota")
            evidence["cases"][case] = invoke(supervisor, binary, directory, nonce, case, database=database)
            if database and case != "prepare_readonly" and evidence["cases"][case].get("queue_records") != evidence["seed_spool"].get("queue_records"):
                raise RuntimeError("fault case did not retain the original source matrix")
        detach(directory)
        mount(directory, readonly=True, filesystem=args.filesystem)
        evidence["cases"]["readonly"] = invoke(supervisor, binary, directory, nonce, "readonly", database=database)
        if database:
            if evidence["cases"]["readonly"].get("queue_records") != evidence["seed_spool"].get("queue_records"):
                raise RuntimeError("read-only remount changed original source matrix")
            detach(directory)
            mount(directory, filesystem=args.filesystem)
            evidence["spool_recovery"] = invoke(supervisor, binary, directory, nonce, "recover_all", database=database, url=url, active=args.with_active_spool)
            if evidence["spool_recovery"]["recovery"].get("queue_records") != evidence["seed_spool"].get("queue_records"):
                raise RuntimeError("recovery changed original source matrix")
        if source_manifest() != evidence["source_manifest_sha256"]:
            raise RuntimeError("source changed during filesystem acceptance")
        evidence["source_unchanged"] = True
        evidence["work_passed"] = True
    except (Exception, KeyboardInterrupt) as error:
        evidence.update(error_type=type(error).__name__, error=str(error))
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        prior_int = signal.signal(signal.SIGINT, signal.SIG_IGN)
        def cleanup_database():
            proof = invoke(supervisor, binary, directory, nonce, "cleanup_database", database=database, url=url)
            if proof.get("name") != database:
                raise RuntimeError("database cleanup target differs")
            supervisor.cleanup()
            if not supervisor.cleanup_confirmed():
                raise RuntimeError("database cleanup process unconfirmed")
        failures = cleanup_owned(supervisor, directory, evidence,
                                 database_cleanup=cleanup_database if database_reserved else None)
        evidence["cleanup_status"] = "unconfirmed" if failures else "confirmed"
        if failures:
            evidence["cleanup_errors"] = failures
        elif evidence.get("work_passed"):
            evidence["status"] = "passed"
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        private_json(args.evidence, evidence)
        signal.signal(signal.SIGTERM, previous)
        signal.signal(signal.SIGINT, prior_int)
    print(f"Filesystem acceptance: {evidence['status']}; evidence: {args.evidence}")
    print(f"Private diagnostic directory: {directory}")
    return 0 if evidence["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
