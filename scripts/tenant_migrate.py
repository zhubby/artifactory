#!/usr/bin/env python3
"""Back up databases before the offline tenant migration. Connection secrets stay in env."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
from urllib.parse import parse_qsl, unquote, urlsplit


def pg_env(url):
    parsed = urlsplit(url)
    if parsed.scheme not in ("postgres", "postgresql"):
        raise ValueError("expected PostgreSQL connection URL")
    env = dict(os.environ)
    env.update(PGHOST=parsed.hostname or "localhost", PGPORT=str(parsed.port or 5432),
               PGUSER=unquote(parsed.username or "postgres"),
               PGPASSWORD=unquote(parsed.password or ""), PGDATABASE=unquote(parsed.path.lstrip("/")))
    options = {"sslmode": "PGSSLMODE", "ssl-mode": "PGSSLMODE",
               "sslrootcert": "PGSSLROOTCERT", "sslcert": "PGSSLCERT",
               "sslkey": "PGSSLKEY", "sslcrl": "PGSSLCRL",
               "connect_timeout": "PGCONNECT_TIMEOUT", "options": "PGOPTIONS",
               "application_name": "PGAPPNAME"}
    for key, value in parse_qsl(parsed.query, keep_blank_values=True):
        if key not in options:
            raise ValueError("unsupported PostgreSQL URL option for backup")
        env[options[key]] = value
    return env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--maintenance-confirmed", action="store_true")
    parser.add_argument("--backup-dir", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.apply:
        if not args.maintenance_confirmed or not args.backup_dir:
            parser.error("--apply requires --maintenance-confirmed and a new --backup-dir")
        dump, restore = shutil.which("pg_dump"), shutil.which("pg_restore")
        if not dump or not restore:
            parser.error("PostgreSQL pg_dump and pg_restore are required")
        args.backup_dir.mkdir(mode=0o700, parents=True, exist_ok=False)
        backups = []
        for name, variable in [("control", "DATABASE_URL"), ("runtime", "ARTIFACTORY_RUNTIME_DATABASE_URL")]:
            if variable not in os.environ:
                if name == "control":
                    parser.error("DATABASE_URL required")
                continue
            destination = args.backup_dir / f"{name}.dump"
            # Never include the connection URL or password in child arguments or diagnostics.
            with destination.open("wb") as output:
                subprocess.run([dump, "--format=custom", "--no-owner", "--no-acl"],
                               env=pg_env(os.environ[variable]), stdout=output, check=True)
            destination.chmod(0o600)
            subprocess.run([restore, "--list", str(destination)], stdout=subprocess.DEVNULL, check=True)
            digest = hashlib.sha256()
            with destination.open("rb") as backup:
                while chunk := backup.read(1024 * 1024):
                    digest.update(chunk)
            backups.append({"name": name, "file": destination.name,
                            "sha256": digest.hexdigest()})
        (args.backup_dir / "manifest.json").write_text(json.dumps(backups, indent=2) + "\n")
    command = ["cargo", "run", "--locked", "-p", "artifactory-control-plane", "--example", "tenant_migrate", "--"]
    if args.apply:
        command += ["--apply", "--maintenance-confirmed"]
    subprocess.run(command, cwd=root, check=True)


if __name__ == "__main__":
    main()
