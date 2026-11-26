#!/usr/bin/env python3
"""[INPUT]: 冻结Git基线、当前发送器和私有隔离Timescale管理员连接。
[OUTPUT]: 编译旧路由或完整旧main，输出跨版本上报与回收证据。
[POS]: 独立本地验收入口；完整进程验证迁移握手，不触碰安装服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
import urllib.parse

BASELINE = "16535fdf1f7b2f10baaab36d58fb666d73903118"
ROOT = Path(__file__).resolve().parents[1]


def command(*values):
    return list(map(str, values))


def private_write(path, data):
    with open(path, "xb") as output:
        os.chmod(path, 0o600)
        output.write(data)


def validate_database_url(value):
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme not in ("postgres", "postgresql") or parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise RuntimeError("isolated test database must use a loopback connection")
    # URL查询参数不得把已检查的回环目标替换成外部主机。
    if any(key.lower() in ("host", "hostaddr", "port", "dbname", "database", "service", "servicefile") for key, _ in urllib.parse.parse_qsl(parsed.query)):
        raise RuntimeError("isolated test database query must not override its target")
    return value


def database_url(container):
    value = os.environ.get("ARTIFACTORY_TEST_DATABASE_URL")
    if container:
        details = json.loads(subprocess.check_output(command("docker", "inspect", container)))[0]
        if "timescale" not in details["Config"]["Image"].lower():
            raise RuntimeError("explicit test container must use a Timescale image")
        values = dict(item.split("=", 1) for item in details["Config"]["Env"] if "=" in item)
        ports = details["NetworkSettings"]["Ports"].get("5432/tcp") or []
        if not ports:
            raise RuntimeError("explicit test container must publish PostgreSQL locally")
        port = ports[0]["HostPort"]
        user = values.get("POSTGRES_USER", "postgres")
        database = values.get("POSTGRES_DB", user)
        quote = lambda item: urllib.parse.quote(item, safe="")
        value = f"postgresql://{quote(user)}:{quote(values.get('POSTGRES_PASSWORD', ''))}@127.0.0.1:{port}/{quote(database)}"
    if not value:
        raise RuntimeError("provide a private ARTIFACTORY_TEST_DATABASE_URL or explicit --database-container")
    return validate_database_url(value)


def compile_test(source, target, package, name, log, timeout, *, binary=False):
    env = os.environ.copy()
    env.pop("ARTIFACTORY_TEST_DATABASE_URL", None)
    env["CARGO_TARGET_DIR"] = str(target)
    # 旧源码隔离编译到内部磁盘，不向接近满盘的工作区写target。
    args = ["cargo", "build" if binary else "test", "--locked", "-p", package,
            "--bin" if binary else "--test", name, "--message-format=json"]
    if not binary:
        args.append("--no-run")
    result = subprocess.run(command(*args), cwd=source, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout)
    private_write(log, result.stdout)
    if result.returncode:
        raise RuntimeError(f"{name} compilation failed; inspect private build log")
    binaries = []
    for line in result.stdout.splitlines():
        try:
            event = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        if event.get("reason") == "compiler-artifact" and event.get("target", {}).get("name") == name and event.get("executable"):
            binaries.append(Path(event["executable"]))
    if len(binaries) != 1:
        raise RuntimeError(f"{name} compilation did not identify one test binary")
    return binaries[0]


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def source_manifest():
    paths = subprocess.check_output(command("git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"), cwd=ROOT).split(b"\0")
    result = hashlib.sha256()
    for relative in sorted(set(path for path in paths if path)):
        source = ROOT / os.fsdecode(relative)
        if not source.is_file():
            continue
        result.update(relative + b"\0" + digest(source).encode() + b"\n")
    return result.hexdigest()


def stop_receiver(process, fixture):
    if process is None:
        return
    (fixture / "stop").touch(mode=0o600)
    try:
        process.wait(timeout=12)
    except subprocess.TimeoutExpired:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        raise RuntimeError("owned legacy fixture required forced shutdown; database cleanup unconfirmed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database-container", help="explicit dedicated local Timescale test container; never stopped")
    parser.add_argument("--evidence", type=Path, default=Path("/tmp/emissary-real-legacy-evidence.json"))
    parser.add_argument("--build-timeout", type=int, default=900)
    parser.add_argument("--receiver-main", action="store_true", help="run the frozen service main with real business migrations and a restricted authorization role")
    parser.add_argument("--keep-temporary", action="store_true", help="retain private export, build and runtime logs for diagnosis")
    args = parser.parse_args()
    url = database_url(args.database_container)
    temporary = Path(tempfile.mkdtemp(prefix="emissary-legacy-", dir="/tmp"))
    os.chmod(temporary, 0o700)
    fixture = temporary / "fixture"
    fixture.mkdir(mode=0o700)
    process = None
    receiver_log = None
    evidence = {
        "version": 1, "baseline_commit": BASELINE,
        "receiver_boundary": "frozen main with real migrations and restricted business authorization" if args.receiver_main else "frozen generated router, Service and store; old main migration handshake bypassed",
        "status": "failed", "phase": "export",
    }
    try:
        print("Exporting frozen legacy receiver", flush=True)
        archive = temporary / "source.tar"
        with archive.open("wb") as output:
            subprocess.run(command("git", "archive", BASELINE), cwd=ROOT, stdout=output, check=True)
        old = temporary / "old"
        old.mkdir()
        with tarfile.open(archive) as source:
            source.extractall(old, filter="data")
        archive.unlink()
        helper = ROOT / "crates/emissary/tests/support/legacy_receiver_fixture.rs"
        main_helper = ROOT / "crates/emissary/tests/support/legacy_main_fixture.rs"
        support = old / "crates/telemetry-service/tests/support"
        support.mkdir(exist_ok=True)
        shutil.copyfile(helper, support / "legacy_receiver_fixture.rs")
        shutil.copyfile(main_helper, support / "legacy_main_fixture.rs")
        for source, destination in (
            (ROOT / "crates/control-plane/migrations/migrations/0010_node_operations.sql", "current_business.sql"),
            (ROOT / "crates/telemetry-service/migrations/migrations/0003_inventory_delivery.sql", "current_telemetry.sql"),
            (ROOT / "crates/telemetry-service/migrations/migrations/0004_inventory_integrity.sql", "current_integrity.sql"),
        ):
            shutil.copyfile(source, support / destination)
            evidence[destination.replace(".sql", "_sha256")] = digest(source)
        (old / "crates/telemetry-service/tests/legacy_receiver_fixture.rs").write_text(
            '#[path = "support/legacy_receiver_fixture.rs"]\nmod fixture;\n')
        evidence["legacy_fixture_sha256"] = digest(helper)
        evidence["legacy_main_fixture_sha256"] = digest(main_helper)
        evidence["sender_test_sha256"] = digest(ROOT / "crates/emissary/tests/telemetry_legacy_receiver.rs")
        evidence["current_head"] = subprocess.check_output(command("git", "rev-parse", "HEAD"), cwd=ROOT, text=True).strip()
        changes = subprocess.check_output(command("git", "diff", "--binary", "HEAD"), cwd=ROOT)
        evidence["current_tracked_diff_sha256"] = hashlib.sha256(changes).hexdigest()
        evidence["current_source_manifest_sha256"] = source_manifest()
        evidence["phase"] = "compile_legacy"
        print("Compiling frozen legacy receiver", flush=True)
        receiver = compile_test(old, temporary / "target", "artifactory-telemetry-service", "legacy_receiver_fixture", temporary / "legacy-build.log", args.build_timeout)
        evidence["receiver_binary_sha256"] = digest(receiver)
        service_binary = None
        if args.receiver_main:
            evidence["phase"] = "compile_legacy_main"
            print("Compiling frozen service main", flush=True)
            service_binary = compile_test(old, temporary / "target", "artifactory-telemetry-service", "artifactory-telemetry-service", temporary / "main-build.log", args.build_timeout, binary=True)
            evidence["receiver_main_sha256"] = digest(service_binary)
        evidence["phase"] = "compile_sender"
        print("Compiling current sender", flush=True)
        current_target = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()
        sender = compile_test(ROOT, current_target, "artifactory-emissary", "telemetry_legacy_receiver", temporary / "sender-build.log", args.build_timeout)
        evidence["sender_binary_sha256"] = digest(sender)
        env = os.environ.copy()
        env["ARTIFACTORY_TEST_DATABASE_URL"] = url
        env["ARTIFACTORY_LEGACY_FIXTURE_DIR"] = str(fixture)
        if service_binary:
            env["ARTIFACTORY_LEGACY_MAIN_BINARY"] = str(service_binary)
        evidence["phase"] = "legacy_startup"
        print("Starting isolated legacy receiver", flush=True)
        receiver_log = (temporary / "receiver.log").open("xb")
        os.chmod(temporary / "receiver.log", 0o600)
        process = subprocess.Popen(command(receiver, "--ignored", "--exact", "fixture::serve_legacy_receiver", "--nocapture"), cwd=old, env=env, stdout=receiver_log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 30
        while not (fixture / "ready.json").exists():
            if process.poll() is not None:
                raise RuntimeError("legacy fixture exited before ready; inspect private runtime log")
            if time.monotonic() > deadline:
                raise RuntimeError("legacy fixture ready deadline exceeded")
            time.sleep(0.05)
        ready = json.loads((fixture / "ready.json").read_text())
        evidence["database_name"] = ready["database_name"]
        evidence["phase"] = "sender"
        print("Verifying real mTLS delivery and durable legacy storage", flush=True)
        with (temporary / "sender.log").open("xb") as output:
            os.chmod(temporary / "sender.log", 0o600)
            result = subprocess.run(command(sender, "--ignored", "--exact", "current_sender_to_frozen_legacy_receiver", "--nocapture"), cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT, timeout=65)
        if result.returncode:
            raise RuntimeError("current sender acceptance failed; inspect private runtime log")
        evidence["sender"] = json.loads((fixture / "sender-evidence.json").read_text())
        evidence["phase"] = "cleanup"
        stop_receiver(process, fixture)
        if process.returncode:
            raise RuntimeError("legacy fixture failed during shutdown; inspect private runtime log")
        process = None
        closed = json.loads((fixture / "closed.json").read_text())
        if closed.get("database_removed") is not True or closed.get("database_name") != ready["database_name"]:
            raise RuntimeError("random test database cleanup was not confirmed")
        evidence["database_removed"] = True
        if args.receiver_main:
            if closed.get("authorization_role_removed") is not True:
                raise RuntimeError("random authorization role cleanup was not confirmed")
            evidence["authorization_role_removed"] = True
            evidence["main"] = json.loads((fixture / "main-evidence.json").read_text())
        evidence["status"] = "passed"
        evidence["phase"] = "complete"
    except Exception as error:
        # 异常只输出有限阶段信息；数据库URL、密钥和原始日志留在私有目录。
        evidence["error_type"] = type(error).__name__
        evidence["error"] = str(error) if isinstance(error, RuntimeError) else "acceptance operation failed; inspect private logs"
    finally:
        if process is not None:
            try:
                stop_receiver(process, fixture)
            except RuntimeError:
                evidence["cleanup_error"] = "owned legacy fixture forced shutdown; database cleanup unconfirmed"
            if (fixture / "closed.json").exists():
                closed = json.loads((fixture / "closed.json").read_text())
                evidence["database_name"] = closed.get("database_name")
                evidence["database_removed"] = closed.get("database_removed")
                evidence["authorization_role_removed"] = closed.get("authorization_role_removed")
        if receiver_log is not None:
            receiver_log.close()
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        args.evidence.write_text(json.dumps(evidence, indent=2) + "\n")
        os.chmod(args.evidence, 0o600)
        if args.keep_temporary or evidence["status"] != "passed":
            print(f"Private diagnostic directory: {temporary}", flush=True)
        else:
            shutil.rmtree(temporary)
    print(f"Legacy receiver acceptance: {evidence['status']}; evidence: {args.evidence}", flush=True)
    return 0 if evidence["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
