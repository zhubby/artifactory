#!/usr/bin/env python3
"""[INPUT]: 当前服务与测试源码、精确资源清单和私有回环Timescale连接。
[OUTPUT]: 监督完整Telemetry与新旧发送进程，输出双库、积压新鲜度和持久ACK证据。
[POS]: 普通进程验收入口；共享POSIX监督器，不安装原生服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import tarfile
import time
import tempfile
import uuid

import check_emissary_legacy_receiver as legacy
from check_emissary_legacy_sender import Supervisor, clean_environment, compile_test, private_json, sender_environment


def compile_main(supervisor, target, directory, timeout):
    log = directory / "main-build.log"
    env = clean_environment()
    env["CARGO_TARGET_DIR"] = str(target)
    child = supervisor.start(["cargo", "build", "--locked", "-p", "artifactory-telemetry-service",
                              "--bin", "artifactory-telemetry-service", "--message-format=json"],
                             cwd=legacy.ROOT, env=env, log=log, label="build-current-main")
    if child.wait(timeout):
        raise RuntimeError("current main compilation failed; inspect private log")
    binaries = []
    for line in log.read_bytes().splitlines():
        try:
            event = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        if (event.get("reason") == "compiler-artifact" and event.get("executable")
                and event.get("target", {}).get("name") == "artifactory-telemetry-service"):
            binaries.append(Path(event["executable"]))
    if len(binaries) != 1:
        raise RuntimeError("current main compilation did not identify one executable")
    return binaries[0]


def invoke(supervisor, binary, name, *, env, directory, timeout=45):
    child = supervisor.start([binary, "--ignored", "--exact", name, "--nocapture"],
                             cwd=legacy.ROOT, env=env, log=directory / f"{name}.log", label=name)
    if child.wait(timeout):
        raise RuntimeError(f"{name} failed; inspect private log")


def frozen_sender(supervisor, directory, timeout):
    archive = directory / "frozen.tar"
    child = supervisor.start(["git", "archive", "--output", archive, legacy.BASELINE],
                             cwd=legacy.ROOT, env=clean_environment(),
                             log=directory / "export.log", label="frozen-export")
    if child.wait(30):
        raise RuntimeError("frozen source export failed")
    old = directory / "old"
    old.mkdir()
    with tarfile.open(archive) as source:
        manifest = {member.name: hashlib.sha256(source.extractfile(member).read()).hexdigest()
                    for member in source.getmembers() if member.isfile()}
        source.extractall(old, filter="data")
    private_json(directory / "frozen-manifest.json", manifest)
    archive.unlink()
    helper = legacy.ROOT / "crates/emissary/tests/support/legacy_sender_fixture.rs"
    shutil.copyfile(helper, old / "crates/emissary/tests/legacy_sender_fixture.rs")
    return compile_test(supervisor, old, directory / "old-target", "legacy_sender_fixture",
                        directory / "old-build.log", timeout)


def verify_frozen_tree(directory):
    manifest = json.loads((directory / "frozen-manifest.json").read_text())
    if not manifest or any(legacy.digest(directory / "old" / name) != digest
                           for name, digest in manifest.items()):
        raise RuntimeError("frozen product source changed during acceptance")
    return hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()


def validate_fairness(proof):
    fairness = proof.get("fairness")
    if not isinstance(fairness, dict):
        raise RuntimeError("current main fairness proof missing")
    for field in ("passed", "actual_current_main", "authorization_acl_proven",
                  "history_continuously_backlogged", "original_history_and_inflight_bytes_preserved",
                  "sender_stopped", "isolated_delay_removed"):
        if fairness.get(field) is not True:
            raise RuntimeError("current main fairness proof incomplete")
    for field, lower, upper in (("first_projection_ms", 0, 44999),
                                ("max_observed_fresh_ack_ms", 0, 60000),
                                ("max_projection_age_ms", 0, 89999),
                                ("fresh_business_acks", 3, 8192),
                                ("historical_business_acks", 3, 8192),
                                ("current_startup_queries", 1, 10000)):
        value = fairness.get(field)
        if type(value) is not int or not lower <= value <= upper:
            raise RuntimeError("current main fairness measurement invalid")


def run_delivery(supervisor, binary, service, *, env, directory, timeout=320, sender=None):
    test = supervisor.start([binary, "--ignored", "--exact",
                             "actual_current_main_uses_separate_databases_and_restricted_business_role",
                             "--nocapture"], cwd=legacy.ROOT, env=env,
                            log=directory / "runtime.log", label="current-main-acceptance")
    sequence = 0
    main = None
    deadline = time.monotonic() + timeout
    phases = ("missing_startup_grant", "authorized_main", "stop_main")
    senders = {}
    sender_sequence = 0
    try:
        while not test.finished():
            if time.monotonic() >= deadline:
                raise RuntimeError("current main outer deadline exceeded")
            request = directory / f"request-{sequence}.json"
            if sequence < len(phases) and request.exists():
                phase = phases[sequence]
                if json.loads(request.read_text()) != {"phase": phase}:
                    raise RuntimeError("current main request order differs")
                if sequence in (0, 1):
                    child_env = clean_environment()
                    child_env.update(json.loads((directory / "service-env.json").read_text()))
                    main = supervisor.start([service], cwd=legacy.ROOT, env=child_env,
                                            log=directory / f"main-{sequence}.log", label=f"current-main-{sequence}")
                    if sequence == 0:
                        if main.wait(35) == 0:
                            raise RuntimeError("main accepted missing startup grant")
                        if b"SELECT on node_startups is required" not in (directory / "main-0.log").read_bytes():
                            raise RuntimeError("main rejected for an unexpected startup reason")
                else:
                    if main is None or main.finished():
                        raise RuntimeError("authorized main exited before acceptance")
                    os.killpg(main.process.pid, signal.SIGINT)
                    if main.wait(12):
                        raise RuntimeError("authorized main failed graceful shutdown")
                private_json(directory / f"result-{sequence}.json", {"phase": phase, "success": True})
                sequence += 1
            if sequence == 2 and main.finished():
                raise RuntimeError("authorized main exited unexpectedly")
            frozen = directory / "frozen"
            for index in range(3):
                request = frozen / f"request-{index}.json"
                result = frozen / f"result-{index}.json"
                if request.exists() and index not in senders:
                    if sender is None or index != sender_sequence:
                        raise RuntimeError("unexpected frozen sender request")
                    test_name = "prepare_frozen_source" if index == 0 else "frozen_sender_to_current_receiver"
                    if json.loads(request.read_text()) != {"sequence": index, "test": test_name}:
                        raise RuntimeError("invalid frozen sender request")
                    child = supervisor.start([sender, "--ignored", "--exact", test_name, "--nocapture"],
                                             cwd=legacy.ROOT, env=sender_environment(frozen),
                                             log=directory / f"sender-{index}.log", label=f"frozen-sender-{index}")
                    senders[index] = (child, time.monotonic())
                if index in senders and not result.exists():
                    child, since = senders[index]
                    if child.finished():
                        code = child.wait(3)
                        private_json(result, {"sequence": index, "success": code == 0, "process_reaped": True})
                        sender_sequence += 1
                        if code:
                            raise RuntimeError("frozen sender failed; inspect private log")
                    elif time.monotonic() - since > 105:
                        raise RuntimeError("frozen sender deadline exceeded")
            time.sleep(0.025)
        if test.wait(3):
            raise RuntimeError("current main acceptance failed; inspect private runtime log")
        if sequence != len(phases):
            raise RuntimeError("current main did not complete supervised phases")
        if sender is not None and sender_sequence != 3:
            raise RuntimeError("current main did not complete frozen sender matrix")
    finally:
        supervisor.cleanup()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database-container")
    parser.add_argument("--evidence", type=Path, default=Path("/tmp/emissary-current-main-evidence.json"))
    parser.add_argument("--build-timeout", type=int, default=900)
    parser.add_argument("--keep-temporary", action="store_true")
    parser.add_argument("--with-legacy-sender", action="store_true")
    parser.add_argument("--with-fairness", action="store_true")
    args = parser.parse_args()
    if os.name != "posix" or not hasattr(os, "WNOWAIT"):
        raise RuntimeError("current main driver requires POSIX process ownership")
    url = legacy.database_url(args.database_container)
    directory = Path(tempfile.mkdtemp(prefix="emissary-current-main-", dir="/tmp"))
    os.chmod(directory, 0o700)
    nonce = uuid.uuid4().hex
    private_json(directory / "resources.json", {"business": "emissary_business_" + nonce,
                 "telemetry": "emissary_telemetry_" + nonce, "role": "emissary_auth_" + nonce,
                 "password": uuid.uuid4().hex, "nonce": nonce})
    env = clean_environment()
    env.update({"ARTIFACTORY_TEST_DATABASE_URL": url, "ARTIFACTORY_CURRENT_MAIN_DIR": str(directory)})
    if args.with_fairness:
        env["ARTIFACTORY_CURRENT_MAIN_FAIRNESS"] = "1"
    supervisor = Supervisor()
    attempted = False
    binary = None
    evidence = {"version": 1, "status": "failed", "phase": "compile", "nonce": nonce,
                "boundary": "ordinary current Telemetry main, distinct databases and restricted ACL; no Control main/native deployment"}
    interrupted = None
    previous = signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        evidence["current_source_manifest_sha256"] = legacy.source_manifest()
        sender = None
        if args.with_legacy_sender:
            print("Compiling unchanged frozen sender with a test-only entry", flush=True)
            sender = frozen_sender(supervisor, directory, args.build_timeout)
            env["ARTIFACTORY_CURRENT_MAIN_LEGACY"] = "1"
            evidence.update({"frozen_baseline": legacy.BASELINE,
                             "frozen_sender_sha256": legacy.digest(sender),
                             "frozen_test_entry_sha256": legacy.digest(legacy.ROOT / "crates/emissary/tests/support/legacy_sender_fixture.rs")})
        target = Path(os.environ.get("CARGO_TARGET_DIR", str(legacy.ROOT / "target"))).resolve()
        print("Compiling current Telemetry main and acceptance", flush=True)
        binary = compile_test(supervisor, legacy.ROOT, target, "telemetry_current_main",
                              directory / "test-build.log", args.build_timeout)
        service = compile_main(supervisor, target, directory, args.build_timeout)
        evidence.update({"main_binary_sha256": legacy.digest(service),
                         "test_binary_sha256": legacy.digest(binary)})
        evidence["phase"] = "reserve"
        invoke(supervisor, binary, "reserve_current_main_resources", env=env, directory=directory)
        if json.loads((directory / "reserved.json").read_text()) != {"nonce": nonce, "absent_before_run": True}:
            raise RuntimeError("current main reservation differs")
        attempted = True
        evidence["phase"] = "delivery"
        print("Verifying full main startup, dual databases, ACL and transaction ACK", flush=True)
        run_delivery(supervisor, binary, service, env=env, directory=directory, sender=sender,
                     timeout=480 if args.with_fairness else 320)
        proof = json.loads((directory / "proof.json").read_text())
        if proof.get("passed") is not True:
            raise RuntimeError("current main proof missing")
        if args.with_legacy_sender and (proof.get("frozen_sender") or {}).get("frozen_sender_executed") is not True:
            raise RuntimeError("current main frozen sender proof missing")
        if args.with_fairness:
            validate_fairness(proof)
        if args.with_legacy_sender:
            evidence["frozen_product_manifest_sha256"] = verify_frozen_tree(directory)
            evidence["frozen_product_tree_unchanged"] = True
        if legacy.source_manifest() != evidence["current_source_manifest_sha256"]:
            raise RuntimeError("source changed during current main acceptance")
        evidence.update({"receiver": proof, "work_passed": True})
    except (Exception, KeyboardInterrupt) as error:
        interrupted = error
        evidence["error_type"] = type(error).__name__
        evidence["error"] = str(error) if isinstance(error, RuntimeError) else "acceptance failed; inspect private logs"
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        prior_int = signal.signal(signal.SIGINT, signal.SIG_IGN)
        failures = []
        try:
            supervisor.cleanup()
        except Exception:
            failures.append("owned_processes")
        if attempted:
            try:
                invoke(supervisor, binary, "cleanup_current_main_resources", env=env, directory=directory)
                if json.loads((directory / "cleaned.json").read_text()) != {"nonce": nonce, "databases_removed": True, "role_removed": True}:
                    raise RuntimeError("cleanup receipt differs")
                evidence.update({"databases_removed": True, "role_removed": True})
            except Exception:
                failures.append("owned_databases_or_role")
        try:
            supervisor.cleanup()
        except Exception:
            failures.append("cleanup_process")
        evidence["process_cleanup_confirmed"] = supervisor.cleanup_confirmed()
        if not supervisor.cleanup_confirmed():
            failures.append("prior_process_cleanup")
        evidence["cleanup_status"] = "cleanup_unconfirmed" if failures else "confirmed"
        if failures:
            evidence["cleanup_errors"] = failures
        if evidence.get("work_passed") and not failures and evidence.get("databases_removed"):
            evidence.update({"status": "passed", "phase": "complete"})
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        private_json(args.evidence, evidence)
        if args.keep_temporary or evidence["status"] != "passed":
            print(f"Private diagnostic directory: {directory}", flush=True)
        else:
            shutil.rmtree(directory)
        signal.signal(signal.SIGTERM, previous)
        signal.signal(signal.SIGINT, prior_int)
    print(f"Current main acceptance: {evidence['status']}; evidence: {args.evidence}", flush=True)
    return 130 if isinstance(interrupted, KeyboardInterrupt) else (0 if evidence["status"] == "passed" else 1)


if __name__ == "__main__":
    raise SystemExit(main())
