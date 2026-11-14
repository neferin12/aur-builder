#!/usr/bin/env python3
"""Local development commands. Do not read or rewrite the production .env."""
import json
from functools import lru_cache
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
DEV = ROOT / ".devcontainer"


def environment():
    env = os.environ.copy()
    for line in (DEV / "dev.env").read_text().splitlines():
        if line.strip() and not line.lstrip().startswith("#"):
            key, value = line.split("=", 1)
            env[key] = value
    env["NODE_EXTRA_CA_CERTS"] = "/dev-certs/ca.crt"
    return env


def run(*args, env=None, capture=False):
    return subprocess.run(args, cwd=ROOT, env=env or environment(), check=True,
                          text=True, stdout=subprocess.PIPE if capture else None)


@lru_cache(maxsize=1)
def compose_project():
    # Editors may override Compose's project name. Reuse the project containing
    # this devcontainer instead of accidentally addressing another stack.
    if os.environ.get("AUR_DEVCONTAINER") == "1":
        return run("docker", "inspect", os.environ["HOSTNAME"], "--format",
                   '{{index .Config.Labels "com.docker.compose.project"}}', capture=True).stdout.strip()
    return "aur-builder-dev"


def compose(*args, capture=False):
    return run("docker", "compose", "--project-name", compose_project(), "-f", str(DEV / "compose.yaml"), *args, capture=capture)


def garage(*args):
    return compose("exec", "-T", "-e", "RUST_LOG=warn", "garage", "/garage", *args, capture=True).stdout


def wait(label, probe, timeout=180):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = probe()
            if result:
                return result
        except (OSError, ValueError, subprocess.CalledProcessError):
            pass
        time.sleep(2)
    raise RuntimeError(f"Timed out waiting for {label}; inspect docker compose logs")


def provision_storage():
    status = wait("Garage", lambda: garage("status"))
    match = re.search(r"^([0-9a-f]{16,64})\s+.*NO ROLE ASSIGNED", status, re.M)
    if match:
        garage("layout", "assign", "-z", "dev", "-c", "1G", match[1])
        garage("layout", "apply", "--version", "1")
    env = environment()
    bucket, key = env["AB_S3_BUCKET"], env["AB_S3_ACCESS_KEY"]
    if not re.search(rf"\b{re.escape(bucket)}\b", garage("bucket", "list")):
        garage("bucket", "create", bucket)
    if key not in garage("key", "list"):
        garage("key", "import", "--yes", "-n", "aur-builder-dev", key, env["AB_S3_SECRET_KEY"])
    garage("bucket", "allow", "--read", "--write", "--key", key, bucket)


def seed_users(database, realm):
    from contextlib import closing
    grants = {"admin": (True, True, True, True), "reviewer": (False, True, True, True),
              "viewer": (False, True, False, False), "pending": (False, False, False, False)}
    issuer = environment()["AB_OIDC_ISSUER"]
    with closing(sqlite3.connect(database)) as db:
        with db:
            for user in realm["users"]:
                admin, view, rebuild, approve = grants[user["username"]]
                # Preserve access changes made while testing; repeated bootstrap is safe.
                db.execute("INSERT INTO web_user (id,issuer,subject,display_name,admin,can_view,can_rebuild,can_approve) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(issuer,subject) DO NOTHING",
                           (user["id"], issuer, user["id"], user["firstName"] + " Developer", admin, view, rebuild, approve))


def bootstrap():
    (DEV / "state").mkdir(exist_ok=True)
    provision_storage()
    issuer = environment()["AB_OIDC_ISSUER"]
    wait("Keycloak realm", lambda: json.load(urllib.request.urlopen(issuer + "/.well-known/openid-configuration", timeout=5)))
    run("cargo", "run", "-p", "database", "--example", "migrate")
    schema_env = environment()
    schema_env["DATABASE_URL"] = "sqlite:" + str(DEV / "state/schema.sqlite") + "?mode=rwc"
    run("cargo", "run", "-p", "database", "--example", "migrate", env=schema_env)
    # Keep schema-drift fixtures separate from the integration suite's destructive DB.
    compose("exec", "-T", "postgres", "sh", "-ec", 'psql -U builder -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname=\'aur_builder_schema\'" | grep -q 1 || createdb -U builder aur_builder_schema')
    schema_env["DATABASE_URL"] = schema_env["PRISMA_POSTGRESQL_URL"]
    run("cargo", "run", "-p", "database", "--example", "migrate", env=schema_env)
    seed_users(DEV / "state/dev.sqlite", json.loads((DEV / "keycloak/aur-builder-realm.json").read_text()))
    # Export only the public CA for browser trust on the host.
    (DEV / "state/ca.crt").write_bytes(Path("/dev-certs/ca.crt").read_bytes())
    print("Development environment ready. Run: python3 .devcontainer/run.py web")
    print("Dashboard: https://aur.localhost:8443 — all app test users use dev-password")


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else "help"
    if command == "bootstrap":
        bootstrap()
        return 0
    commands = {"web": ["npm", "--prefix", "web", "run", "dev"],
                "server": ["cargo", "run", "-p", "server"],
                "notifier": ["cargo", "run", "-p", "notifier"],
                "worker": ["cargo", "run", "-p", "worker"],
                "test": ["npm", "--prefix", "web", "test"],
                "schema-check": ["npm", "--prefix", "web", "run", "db:check"]}
    if command not in commands:
        print("Usage: python3 .devcontainer/run.py {bootstrap|web|server|notifier|worker|test|schema-check}")
        return 0 if command == "help" else 2
    return subprocess.call([*commands[command], *sys.argv[2:]], cwd=ROOT, env=environment())


if __name__ == "__main__":
    raise SystemExit(main())
