#!/usr/bin/env python3
# [INPUT]: Kubernetes 任务、卷和 Secret 名称。
# [OUTPUT]: 检查任务临时卷、1.28 双容器隔离与空闲清理。
# [POS]: 只读验收入口，不删除资源或读取 Secret 内容。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
"""Verify ephemeral source storage in one Kubernetes namespace."""

import argparse
import json
import re
import subprocess
import sys

TASK_APPS = {"artifactory-source-task", "artifactory-pi", "artifactory-image-worker"}
OLD_APPS = {"artifactory-source", "artifactory-local-source", "artifactory-source-service"}
TASK_SECRET = re.compile(r"image-[a-f0-9]{32}-task$")


def image_violations(spec, subject):
    failures = []
    containers = spec.get("containers", [])
    init = spec.get("initContainers", [])
    publisher = any(
        "artifactory-image-publisher" in c.get("command", [])
        or any(e.get("name") == "ARTIFACTORY_IMAGE_PLATFORM" and e.get("value") == "publish"
               for e in c.get("env", []))
        for c in containers
    )
    if any("restartPolicy" in c for c in init):
        failures.append(f"{subject}: restartable init is not a Kubernetes 1.28 task")
    if "appArmorProfile" in spec.get("securityContext", {}) or any(
        "appArmorProfile" in c.get("securityContext", {}) for c in [*init, *containers]
    ):
        failures.append(f"{subject}: structured AppArmor field requires a newer Kubernetes API")
    if spec.get("shareProcessNamespace"):
        failures.append(f"{subject}: task containers share their process namespace")
    if spec.get("automountServiceAccountToken") is not False:
        failures.append(f"{subject}: application containers can receive an automatic API token")
    if publisher:
        if init or len(containers) != 1:
            failures.append(f"{subject}: publisher must be a single container without source or BuildKit")
        return failures
    if len(containers) != 2 or {c["name"] for c in containers} != {"worker", "buildkit"}:
        failures.append(f"{subject}: image build requires Runner and supervised BuildKit ordinary containers")
    if len(init) != 1 or init[0]["name"] != "source":
        failures.append(f"{subject}: image build requires one ordinary source init")
    buildkit = next((c for c in containers if c["name"] == "buildkit"), None)
    if buildkit:
        if buildkit.get("command") not in (["artifactory-image-buildkit"], ["/usr/bin/artifactory-image-buildkit"]):
            failures.append(f"{subject}: BuildKit does not use the lifecycle supervisor")
        allowed = {"socket", "buildkit-state", "buildkit-config", "ca"}
        if any(mount["name"] not in allowed for mount in buildkit.get("volumeMounts", [])):
            failures.append(f"{subject}: BuildKit mounts Runner task, source, or private material")
    return failures


def violations(resources, secret_names=(), *, idle=False):
    failures = []
    for item in resources:
        kind = item["kind"]
        metadata = item.get("metadata", {})
        name = metadata.get("name", "")
        app = metadata.get("labels", {}).get("app.kubernetes.io/name")
        if app in OLD_APPS or re.fullmatch(r"sources(?:-\d+)?", name):
            failures.append(f"{kind}/{name}: persistent source service remains")
        if kind == "PersistentVolumeClaim":
            if "source" in name.lower():
                failures.append(f"{kind}/{name}: source PVC remains")
            continue
        spec = item.get("spec", {})
        if kind == "Job":
            template = spec.get("template", {})
            app = template.get("metadata", {}).get("labels", {}).get("app.kubernetes.io/name")
            spec = template.get("spec", {})
        if app not in TASK_APPS or kind not in {"Pod", "Job"}:
            continue
        if app == "artifactory-image-worker":
            failures.extend(image_violations(spec, f"{kind}/{name}"))
        if idle:
            failures.append(f"{kind}/{name}: task workload remains while idle")
        for volume in spec.get("volumes", []):
            if any(key in volume for key in ("persistentVolumeClaim", "hostPath", "nfs", "csi")):
                failures.append(f"{kind}/{name}: task volume {volume['name']} is persistent")
        init = next((c for c in spec.get("initContainers", []) if c["name"] == "source"), None)
        if init:
            init_mounts = {mount["name"] for mount in init.get("volumeMounts", [])}
            sensitive = init_mounts & {"source-identity", "source-auth", "auth"}
            for container in spec.get("containers", []):
                mounts = container.get("volumeMounts", [])
                if sensitive & {mount["name"] for mount in mounts}:
                    failures.append(f"{kind}/{name}: main container mounts source identity or credentials")
                workspace = next((m for m in mounts if m.get("mountPath") == "/workspace" or m["name"] == "workspace"), None)
                if workspace and not workspace.get("readOnly"):
                    failures.append(f"{kind}/{name}: main container can write source workspace")
    if idle:
        failures.extend(f"Secret/{name}: task material remains while idle"
                        for name in secret_names if TASK_SECRET.fullmatch(name))
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--context", required=True)
    parser.add_argument("--namespace", default="artifactory")
    parser.add_argument("--idle", action="store_true", help="also require all task Pods, Jobs and Secrets to be gone")
    args = parser.parse_args()
    command = ["kubectl", "--context", args.context, "-n", args.namespace]
    resources = json.loads(subprocess.check_output(
        command + ["get", "pods,jobs,pvc,deployments,statefulsets", "-o", "json"], text=True,
    ))["items"]
    # 只查询名称，不把 Secret 数据带入验收进程。
    secret_names = subprocess.check_output(
        command + ["get", "secrets", "-o", "custom-columns=NAME:.metadata.name", "--no-headers"],
        text=True,
    ).splitlines()
    failures = violations(resources, secret_names, idle=args.idle)
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print("Ephemeral source storage and task cleanup verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
