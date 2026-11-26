#!/usr/bin/env python3
"""[INPUT]: 冻结Git基线、当前接收测试和私有回环Timescale连接。
[OUTPUT]: 监督编译与直接旧发送进程，定点回收自有数据库并输出反向矩阵证据。
[POS]: 普通进程兼容门禁；不安装服务，不按名称或前缀扫描资源。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tarfile
import tempfile
import threading
import time
import uuid

import check_emissary_legacy_receiver as legacy

DATABASE_ENV = ("DATABASE_URL", "ARTIFACTORY_TEST_DATABASE_URL",
                "ARTIFACTORY_BUSINESS_DATABASE_URL", "ARTIFACTORY_TELEMETRY_READ_DATABASE_URL",
                "ARTIFACTORY_PLANNING_DATABASE_URL")


def clean_environment():
    return {name: value for name, value in os.environ.items() if name not in DATABASE_ENV}


def private_json(path, value):
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        json.dump(value, output)
        output.write("\n")
    temporary.replace(path)


def owned_database_name(value):
    if not re.fullmatch(r"durable_receiver_[0-9a-f]{32}", value):
        raise RuntimeError("invalid exact owned database name")
    return value


class OwnedProcess:
    """每个直接子进程拥有独立进程组，退出后仍检查该组的活后代。"""

    def __init__(self, args, *, cwd, env, log, label):
        if os.name != "posix" or not hasattr(os, "WNOWAIT"):
            raise RuntimeError("owned process proof requires POSIX waitid WNOWAIT")
        self.label = label
        self.output = log.open("xb")
        os.chmod(log, 0o600)
        self.process = None
        self.confirmed = False
        self.cleanup_failed = False
        try:
            self.process = subprocess.Popen(list(map(str, args)), cwd=cwd, env=env,
                                            stdout=self.output, stderr=subprocess.STDOUT,
                                            stdin=subprocess.DEVNULL, start_new_session=True)
        except BaseException:
            self.output.close()
            raise

    def live_group(self):
        result = subprocess.run(["ps", "-axo", "pid=,pgid=,stat="], capture_output=True,
                                text=True, timeout=3, check=True)
        return [int(fields[0]) for line in result.stdout.splitlines()
                if len(fields := line.split()) == 3
                and int(fields[1]) == self.process.pid and not fields[2].startswith("Z")]

    def finished(self):
        if self.process.returncode is not None:
            return True
        # 先保留leader的可wait身份，回收该组后才reap，避免PID/PGID复用。
        return os.waitid(os.P_PID, self.process.pid,
                         os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None

    def close(self):
        if self.confirmed:
            return
        # wait已reap后收到中断时，不再查找或发送旧数字PGID。
        # 所有本驱动wait均先完成活进程组读回。
        if self.process.returncode is not None:
            self.confirmed = True
            self.output.close()
            return
        try:
            for signum, budget in ((signal.SIGTERM, 2), (signal.SIGKILL, 3)):
                if not self.live_group():
                    break
                try:
                    os.killpg(self.process.pid, signum)
                except ProcessLookupError:
                    pass
                except PermissionError:
                    # Darwin在整组刚变为僵尸时拒绝信号；仍以活组读回判定。
                    if self.live_group():
                        raise
                deadline = time.monotonic() + budget
                while self.live_group() and time.monotonic() < deadline:
                    threading.Event().wait(0.025)
            if self.live_group():
                raise RuntimeError(f"{self.label} process cleanup_unconfirmed")
            self.process.wait(timeout=3)
            self.confirmed = True
        except BaseException:
            self.cleanup_failed = True
            raise
        finally:
            self.output.close()

    def wait(self, timeout):
        deadline = time.monotonic() + timeout
        try:
            while not self.finished():
                if time.monotonic() > deadline:
                    raise subprocess.TimeoutExpired(self.label, timeout)
                time.sleep(0.025)
            descendants = [pid for pid in self.live_group() if pid != self.process.pid]
            if descendants:
                raise RuntimeError(f"{self.label} exited with live descendants")
            self.close()
            return self.process.returncode
        finally:
            self.close()


class Supervisor:
    def __init__(self):
        self.children = []
        self.cleanup_failed = False

    def start(self, *args, **kwargs):
        # 延后父进程的中断处理，子进程不继承被屏蔽的退出信号。
        pending = []
        previous = {value: signal.getsignal(value) for value in (signal.SIGINT, signal.SIGTERM)}
        for value in previous:
            signal.signal(value, lambda signum, _frame: pending.append(signum))
        child = None
        try:
            child = OwnedProcess(*args, **kwargs)
            self.children.append(child)
        except BaseException:
            if child is not None:
                child.close()
            raise
        finally:
            for value, handler in previous.items():
                signal.signal(value, handler)
            for value in pending:
                signal.raise_signal(value)
        return child

    def cleanup(self):
        failures = []
        for child in reversed(self.children):
            try:
                child.close()
            except Exception:
                failures.append(child.label)
        if failures:
            self.cleanup_failed = True
            raise RuntimeError("owned process cleanup_unconfirmed: " + ",".join(failures))

    def cleanup_confirmed(self):
        return not self.cleanup_failed and all(child.confirmed and not child.cleanup_failed
                                              for child in self.children)


def compile_test(supervisor, source, target, name, log, timeout):
    env = clean_environment()
    env["CARGO_TARGET_DIR"] = str(target)
    child = supervisor.start(["cargo", "test", "--locked", "-p", "artifactory-emissary",
                              "--test", name, "--message-format=json", "--no-run"],
                             cwd=source, env=env, log=log, label="build-" + name)
    if child.wait(timeout):
        raise RuntimeError(f"{name} compilation failed; inspect private build log")
    binaries = []
    for line in log.read_bytes().splitlines():
        try:
            event = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        if (event.get("reason") == "compiler-artifact"
                and event.get("target", {}).get("name") == name and event.get("executable")):
            binaries.append(Path(event["executable"]))
    if len(binaries) != 1:
        raise RuntimeError(f"{name} compilation did not identify one test binary")
    return binaries[0]


def sender_environment(fixture):
    env = clean_environment()
    env["ARTIFACTORY_LEGACY_SENDER_DIR"] = str(fixture)
    return env


def run_delivery(supervisor, receiver_args, sender_args, *, cwd, env, fixture, logs,
                 timeout=260, sender_timeout=105):
    receiver = supervisor.start(receiver_args, cwd=cwd, env=env,
                                log=logs / "runtime.log", label="current-receiver")
    senders = {}
    sequence = 0
    started = time.monotonic()
    try:
        while not receiver.finished():
            if time.monotonic() - started > timeout:
                raise RuntimeError("receiver outer deadline exceeded")
            for index in (0, 1, 2):
                request = fixture / f"request-{index}.json"
                result = fixture / f"result-{index}.json"
                if request.exists() and index not in senders:
                    if index != sequence:
                        raise RuntimeError("old sender request order differs")
                    data = json.loads(request.read_text())
                    test = "prepare_frozen_source" if index == 0 else "frozen_sender_to_current_receiver"
                    if data != {"sequence": index, "test": test}:
                        raise RuntimeError("invalid old sender request marker")
                    invocation = (sender_args[index] if isinstance(sender_args, dict) else
                                  [*sender_args, "--ignored", "--exact", test, "--nocapture"])
                    child = supervisor.start(invocation,
                                             cwd=cwd, env=sender_environment(fixture),
                                             log=logs / f"sender-{index}.log", label=f"old-sender-{index}")
                    senders[index] = (child, time.monotonic())
                if index not in senders or result.exists():
                    continue
                child, since = senders[index]
                if child.finished():
                    code = child.wait(3)
                    private_json(result, {"sequence": index, "success": code == 0,
                                          "process_reaped": True})
                    sequence += 1
                    if code:
                        raise RuntimeError("frozen old sender failed; inspect private sender log")
                elif time.monotonic() - since > sender_timeout:
                    raise RuntimeError("old sender deadline exceeded")
            time.sleep(0.025)
        if receiver.wait(3):
            raise RuntimeError("reverse legacy acceptance failed; inspect private runtime log")
        if sequence != 3 or not all(child.confirmed for child, _ in senders.values()):
            raise RuntimeError("receiver did not complete both directly supervised old senders")
    finally:
        supervisor.cleanup()


def cleanup_database(supervisor, receiver, *, cwd, env, temporary, name):
    owned_database_name(name)
    if env.get("ARTIFACTORY_LEGACY_DATABASE_NAME") != name:
        raise RuntimeError("owned database cleanup target differs")
    child = supervisor.start([receiver, "--ignored", "--exact", "cleanup_frozen_sender_database",
                              "--nocapture"], cwd=cwd, env=env,
                             log=temporary / "database-cleanup.log", label="database-cleanup")
    if child.wait(45):
        raise RuntimeError("owned database cleanup_unconfirmed")
    value = json.loads((temporary / "fixture/database-cleaned.json").read_text())
    if value != {"name": name, "database_removed": True}:
        raise RuntimeError("owned database cleanup read-back mismatch")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database-container", help="explicit local Timescale test container; never stopped")
    parser.add_argument("--evidence", type=Path, default=Path("/tmp/emissary-real-legacy-sender-evidence.json"))
    parser.add_argument("--build-timeout", type=int, default=900)
    parser.add_argument("--keep-temporary", action="store_true")
    args = parser.parse_args()
    # 进程组回收使用本机POSIX身份；不把仅等待Windows父进程写成完整回收。
    if os.name != "posix" or not hasattr(os, "WNOWAIT"):
        raise RuntimeError("legacy sender driver requires a POSIX test host")
    url = legacy.database_url(args.database_container)
    temporary = Path(tempfile.mkdtemp(prefix="emissary-old-sender-", dir="/tmp"))
    os.chmod(temporary, 0o700)
    fixture = temporary / "fixture"
    fixture.mkdir(mode=0o700)
    name = owned_database_name("durable_receiver_" + uuid.uuid4().hex)
    private_json(temporary / "owned-resources.json", {"version": 1, "database_name": name})
    supervisor = Supervisor()
    receiver = None
    database_attempted = False
    env = clean_environment()
    env.update({"ARTIFACTORY_TEST_DATABASE_URL": url,
                "ARTIFACTORY_LEGACY_SENDER_DIR": str(fixture),
                "ARTIFACTORY_LEGACY_DATABASE_NAME": name,
                "ARTIFACTORY_LEGACY_SENDER_EVIDENCE": str(temporary / "proof.json")})
    evidence = {"version": 2, "baseline_commit": legacy.BASELINE,
                "boundary": "frozen sender Handle and wire to current Service/store; no full Control/main deployment",
                "database_name": name, "status": "failed", "phase": "export"}
    interrupted = None
    previous_signal = signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        print("Exporting frozen old sender", flush=True)
        archive = temporary / "source.tar"
        export = supervisor.start(["git", "archive", "--output", archive, legacy.BASELINE],
                                  cwd=legacy.ROOT, env=clean_environment(),
                                  log=temporary / "export.log", label="source-export")
        if export.wait(30):
            raise RuntimeError("frozen source export failed")
        old = temporary / "old"
        old.mkdir()
        with tarfile.open(archive) as source:
            source.extractall(old, filter="data")
        archive.unlink()
        helper = legacy.ROOT / "crates/emissary/tests/support/legacy_sender_fixture.rs"
        shutil.copyfile(helper, old / "crates/emissary/tests/legacy_sender_fixture.rs")
        evidence["old_fixture_sha256"] = legacy.digest(helper)
        evidence["current_receiver_test_sha256"] = legacy.digest(legacy.ROOT / "crates/emissary/tests/telemetry_legacy_sender.rs")
        evidence["current_head"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=legacy.ROOT, text=True, timeout=10).strip()
        evidence["current_source_manifest_sha256"] = legacy.source_manifest()
        evidence["phase"] = "compile_old_sender"
        print("Compiling frozen old sender", flush=True)
        sender = compile_test(supervisor, old, temporary / "target", "legacy_sender_fixture",
                              temporary / "old-build.log", args.build_timeout)
        evidence["old_sender_binary_sha256"] = legacy.digest(sender)
        evidence["phase"] = "compile_current_receiver"
        print("Compiling current receiver acceptance", flush=True)
        target = Path(os.environ.get("CARGO_TARGET_DIR", str(legacy.ROOT / "target"))).resolve()
        receiver = compile_test(supervisor, legacy.ROOT, target, "telemetry_legacy_sender",
                                temporary / "current-build.log", args.build_timeout)
        evidence["current_receiver_binary_sha256"] = legacy.digest(receiver)
        evidence["phase"] = "database_reservation"
        reservation = supervisor.start([receiver, "--ignored", "--exact", "reserve_frozen_sender_database",
                                        "--nocapture"], cwd=legacy.ROOT, env=env,
                                       log=temporary / "database-reservation.log", label="database-reservation")
        if reservation.wait(30):
            raise RuntimeError("exact owned database name was not reserved")
        if json.loads((fixture / "database-reserved.json").read_text()) != {"name": name, "absent_before_run": True}:
            raise RuntimeError("owned database reservation marker mismatch")
        evidence["phase"] = "delivery"
        database_attempted = True
        print("Verifying frozen wire, real transaction ACK and replay", flush=True)
        run_delivery(supervisor, [receiver, "--ignored", "--exact",
                     "frozen_legacy_sender_to_current_durable_receiver", "--nocapture"],
                     [sender],
                     cwd=legacy.ROOT, env=env, fixture=fixture, logs=temporary)
        proof = json.loads((temporary / "proof.json").read_text())
        if proof.get("passed") is not True or proof.get("database_removed") is not True:
            raise RuntimeError("reverse acceptance did not confirm success and database cleanup")
        if legacy.source_manifest() != evidence["current_source_manifest_sha256"]:
            raise RuntimeError("source changed during acceptance; rerun with a stable working tree")
        evidence["receiver"] = proof
        evidence["work_passed"] = True
    except (Exception, KeyboardInterrupt) as error:
        interrupted = error
        evidence["error_type"] = type(error).__name__
        evidence["error"] = str(error) if isinstance(error, RuntimeError) else "acceptance failed; inspect private logs"
    finally:
        # 清理期间不再让第二次SIGTERM跳过精确资源回收。
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        old_interrupt = signal.signal(signal.SIGINT, signal.SIG_IGN)
        failures = []
        try:
            supervisor.cleanup()
        except Exception:
            failures.append("owned_processes")
        if database_attempted:
            try:
                cleanup_database(supervisor, receiver, cwd=legacy.ROOT, env=env,
                                 temporary=temporary, name=name)
                evidence["database_removed"] = True
            except Exception:
                failures.append("owned_database")
        try:
            supervisor.cleanup()
        except Exception:
            failures.append("cleanup_process")
        if not supervisor.cleanup_confirmed():
            failures.append("prior_process_cleanup")
        evidence["processes_reaped"] = supervisor.cleanup_confirmed()
        evidence["process_cleanup_confirmed"] = supervisor.cleanup_confirmed()
        evidence["cleanup_status"] = "cleanup_unconfirmed" if failures else "confirmed"
        if failures:
            evidence["cleanup_errors"] = failures
        if evidence.get("work_passed") and not failures and evidence.get("database_removed"):
            evidence["status"] = "passed"
            evidence["phase"] = "complete"
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        private_json(args.evidence, evidence)
        if args.keep_temporary or evidence["status"] != "passed":
            print(f"Private diagnostic directory: {temporary}", flush=True)
        else:
            shutil.rmtree(temporary)
        signal.signal(signal.SIGTERM, previous_signal)
        signal.signal(signal.SIGINT, old_interrupt)
    print(f"Legacy sender acceptance: {evidence['status']}; evidence: {args.evidence}", flush=True)
    return 130 if isinstance(interrupted, KeyboardInterrupt) else (0 if evidence["status"] == "passed" else 1)


if __name__ == "__main__":
    raise SystemExit(main())
