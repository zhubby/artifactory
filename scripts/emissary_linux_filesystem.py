"""[INPUT]: Linux loop工具、sudo权限与160至1024MiB自有镜像。
[OUTPUT]: 创建仅操作员可读的ext4小卷，读回归属并精确卸载。
[POS]: 文件系统故障门禁的平台适配；不修改宿主盘或服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
"""
import json
import os
from pathlib import Path
import re
import subprocess


def run(args, *, absent=False):
    result = subprocess.run(args, stdin=subprocess.DEVNULL, capture_output=True, timeout=45)
    if result.returncode and not (absent and result.returncode == 1):
        raise RuntimeError(f"{args[0]} failed ({result.returncode})")
    return result.stdout


def owned_loops(directory):
    info = json.loads(run(["sudo", "-n", "losetup", "--json", "--list", "--output", "NAME,BACK-FILE"]))
    image = (directory / "volume.dmg").resolve()
    matches = []
    for entry in info.get("loopdevices", []):
        backing = entry.get("back-file")
        if backing and Path(backing).resolve() == image:
            device = entry.get("name", "")
            if not re.fullmatch(r"/dev/loop[0-9]+", device):
                raise RuntimeError("owned loop identity invalid")
            matches.append(device)
    return matches


def mount_info(directory):
    data = run(["findmnt", "--json", "--mountpoint", str(directory / "mount"),
                "--output", "SOURCE,FSTYPE,TARGET"], absent=True)
    if not data.strip():
        return None
    entries = json.loads(data).get("filesystems", [])
    if len(entries) != 1:
        raise RuntimeError("mount inventory ambiguous")
    return entries[0]


def filesystem_proof(directory):
    loops = owned_loops(directory)
    info = mount_info(directory)
    mountpoint = (directory / "mount").resolve()
    if len(loops) != 1 or not info or info.get("source") != loops[0]:
        raise RuntimeError("mount does not belong to owned image")
    if info.get("fstype") != "ext4" or Path(info.get("target", "/missing")).resolve() != mountpoint:
        raise RuntimeError("filesystem type or mount identity differs")
    if not mountpoint.is_mount():
        raise RuntimeError("owned mount missing")
    return {"requested": "ext4", "observed_type": "ext4", "mount_verified": True,
            "owned_loop_verified": True}


def create(directory, *, size_mib=160):
    if type(size_mib) is not int or not 160 <= size_mib <= 1024:
        raise RuntimeError("private image size exceeds fixture budget")
    image = directory / "volume.dmg"
    with image.open("xb") as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.truncate(size_mib << 20)
    run(["mkfs.ext4", "-F", "-q", "-m", "0", str(image)])


def mount(directory, *, readonly=False):
    if owned_loops(directory) or mount_info(directory):
        raise RuntimeError("owned image already attached")
    mountpoint = directory / "mount"
    mountpoint.mkdir(exist_ok=True)
    if mountpoint.is_mount() or any(mountpoint.iterdir()):
        raise RuntimeError("mount target is occupied")
    options = "loop,ro" if readonly else "loop,rw"
    run(["sudo", "-n", "mount", "-t", "ext4", "-o", options,
         str(directory / "volume.dmg"), str(mountpoint)])
    proof = filesystem_proof(directory)
    if not readonly:
        run(["sudo", "-n", "chown", f"{os.getuid()}:{os.getgid()}", str(mountpoint)])
    return proof


def detach(directory):
    loops = owned_loops(directory)
    info = mount_info(directory)
    if len(loops) > 1:
        raise RuntimeError("owned loop inventory ambiguous")
    if info:
        if (len(loops) != 1 or info.get("source") != loops[0]
                or info.get("fstype") != "ext4"
                or Path(info.get("target", "/missing")).resolve() != (directory / "mount").resolve()):
            raise RuntimeError("refusing to unmount unrelated filesystem")
        run(["sudo", "-n", "umount", str(directory / "mount")])
    if mount_info(directory) or (directory / "mount").is_mount():
        raise RuntimeError("owned mount removal unconfirmed")
    for loop in owned_loops(directory):
        if run(["findmnt", "-rn", "--source", loop, "--output", "TARGET"], absent=True).strip():
            raise RuntimeError("owned loop remains mounted elsewhere")
        run(["sudo", "-n", "losetup", "--detach", loop])
    if owned_loops(directory):
        raise RuntimeError("owned loop removal unconfirmed")
