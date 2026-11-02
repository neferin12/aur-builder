#!/usr/bin/env python3
"""Interactive single-host Docker Compose setup. Requires Python 3.10+."""
import argparse
import getpass
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from urllib.parse import quote, urlsplit

ROOT = Path(__file__).resolve().parent


class SetupError(Exception):
    pass


class EnvFile:
    """Edit dotenv assignments without sourcing code or expanding secret values."""
    def __init__(self, path):
        self.path = Path(path)
        self.lines = self.path.read_text().splitlines() if self.path.exists() else []

    def get(self, key, default=""):
        value = default
        for line in self.lines:
            match = re.match(r"^\s*(?:export\s+)?" + re.escape(key) + r"\s*=\s*(.*)$", line)
            if not match:
                continue
            raw = match[1].strip()
            if raw.startswith("'"):
                match = re.fullmatch(r"'((?:\\.|[^'\\])*)'\s*(?:#.*)?", raw)
                if not match:
                    raise SetupError(f"Unsupported multiline or malformed value for {key} in .env")
                value = match[1].replace("\\'", "'")
            elif raw.startswith('"'):
                match = re.fullmatch(r'"((?:\\.|[^"\\])*)"\s*(?:#.*)?', raw)
                if not match:
                    raise SetupError(f"Unsupported multiline or malformed value for {key} in .env")
                value = re.sub(r'\\([\\"$])', r'\1', match[1])
            else:
                value = re.split(r"\s+#", raw, maxsplit=1)[0].strip()
        return value

    def set(self, key, value):
        if any(c in value for c in "\r\n\0"):
            raise SetupError(f"{key} must fit on one line")
        if value.endswith("\\"):
            raise SetupError(f"{key} cannot end in a backslash in the generated single-quoted dotenv format")
        assignment = key + "='" + value.replace("'", "\\'") + "'"
        pattern = re.compile(r"^\s*(?:export\s+)?" + re.escape(key) + r"\s*=")
        indexes = [i for i, line in enumerate(self.lines) if pattern.match(line)]
        if indexes:
            self.lines[indexes[0]] = assignment
            for i in reversed(indexes[1:]):
                del self.lines[i]
        else:
            self.lines.append(assignment)

    def save(self):
        # Replace atomically, with owner-only permissions, even if the previous file was public.
        fd, name = tempfile.mkstemp(prefix=".env-", dir=self.path.parent)
        try:
            with os.fdopen(fd, "w") as handle:
                handle.write("\n".join(self.lines) + "\n")
            os.replace(name, self.path)
        finally:
            if os.path.exists(name):
                os.unlink(name)


def ask(label, default="", secret=False, required=True):
    suffix = " [keep existing]" if secret and default else (f" [{default}]" if default else "")
    while True:
        value = (getpass.getpass if secret else input)(label + suffix + ": ").strip() or default
        if value or not required:
            if not any(c in value for c in "\r\n\0"):
                return value
        print("Please enter a nonempty, single-line value.")


def yes(label, default=True):
    while True:
        value = input(label + (" [Y/n]: " if default else " [y/N]: ")).strip().lower()
        if not value:
            return default
        if value in ("y", "yes", "n", "no"):
            return value in ("y", "yes")


def origin(value):
    try:
        url = urlsplit(value)
        url.port  # Validate the port, too.
        valid = (url.scheme in ("http", "https") and url.hostname
                 and url.path in ("", "/") and not url.query and not url.fragment
                 and url.username is None and url.password is None)
    except ValueError:
        valid = False
    if not valid or any(c.isspace() for c in value):
        raise SetupError("Use an HTTP(S) origin without credentials, path, query, or fragment.")
    if url.hostname == "example.com" or url.hostname.endswith(".example.com"):
        raise SetupError("Replace the example hostname with your reachable S3 hostname.")
    return value.rstrip("/")


def run(args, root=ROOT, capture=False, check=True):
    # Never echo subprocess arguments/output on failure: commands may handle secrets.
    try:
        result = subprocess.run(args, cwd=root, text=True, stdout=subprocess.PIPE if capture else None,
                                stderr=subprocess.PIPE if capture else None)
    except OSError as error:
        raise SetupError(f"Could not start {args[0]}; check that it is installed.") from error
    if check and result.returncode:
        raise SetupError(f"{Path(args[0]).name} failed (exit {result.returncode}). Correct the problem and rerun setup.")
    return result


class Setup:
    def __init__(self, root=ROOT):
        self.root = Path(root)
        self.env = EnvFile(self.root / ".env")
        self.restart_counts = {}
        self.compose_args = ["docker", "compose", "--project-directory", str(self.root),
                             "--env-file", str(self.root / ".env"), "-f", str(self.root / "docker-compose.yaml")]

    def compose(self, *args, **kwargs):
        # Prevent an unrelated exported shell variable from overriding the saved configuration.
        names = {m[1] for line in self.env.lines if (m := re.match(r"\s*(?:export\s+)?(\w+)\s*=", line))}
        saved = {key: os.environ.pop(key) for key in names if key in os.environ}
        try:
            return run(self.compose_args + list(args), self.root, **kwargs)
        finally:
            os.environ.update(saved)

    def garage(self, *args, **kwargs):
        return self.compose("exec", "-T", "garage", "/garage", *args, capture=True, **kwargs)

    def configure(self):
        print("\nConfigure a single-host deployment. Existing settings are offered as defaults.")
        db_url = "sqlite:/app/db/sqlite.db?mode=rwc"
        if self.env.get("DATABASE_URL") not in ("", db_url):
            if not yes("Switch DATABASE_URL to the shared Compose SQLite database? No data will be moved.", False):
                raise SetupError("This setup flow requires the shared Compose SQLite database. Existing values were not saved.")
        self.env.set("DATABASE_URL", db_url)
        self.env.set("RUST_LOG", self.env.get("RUST_LOG") or "info")
        user = ask("RabbitMQ username", self.env.get("RABBITMQ_DEFAULT_USER") or "builder")
        if user == "guest":
            raise SetupError("Choose a RabbitMQ user other than guest; workers connect from other containers.")
        password = ask("RabbitMQ password (Enter keeps existing or uses a generated password)",
                       self.env.get("RABBITMQ_DEFAULT_PASS") or secrets.token_urlsafe(32), True)
        self.env.set("RABBITMQ_DEFAULT_USER", user)
        self.env.set("RABBITMQ_DEFAULT_PASS", password)
        self.env.set("AMQP_ADDR", f"amqp://{quote(user, safe='')}:{quote(password, safe='')}@rabbitmq:5672/%2f")
        rpc_secret = self.env.get("GARAGE_RPC_SECRET") or secrets.token_hex(32)
        if not re.fullmatch(r"[0-9a-fA-F]{64}", rpc_secret):
            raise SetupError("GARAGE_RPC_SECRET must contain 64 hexadecimal characters.")
        self.env.set("GARAGE_RPC_SECRET", rpc_secret)
        self.env.set("AB_S3_ENDPOINT", "http://garage:3900")
        default = self.env.get("AB_S3_PUBLIC_ENDPOINT")
        if "example.com" in default:
            default = ""
        while True:
            try:
                endpoint = origin(ask("S3 origin reachable from workers AND pacman clients", default))
                if urlsplit(endpoint).hostname in ("localhost", "127.0.0.1", "::1"):
                    print("Loopback addresses point to the build container itself. Use a host reachable over the network.")
                    continue
                break
            except SetupError as error:
                print(error)
        self.env.set("AB_S3_PUBLIC_ENDPOINT", endpoint)
        bucket = ask("Private Garage bucket", self.env.get("AB_S3_BUCKET") or "aur-builder")
        if not re.fullmatch(r"[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]", bucket):
            raise SetupError("Use a 3–63 character lowercase S3 bucket name.")
        self.env.set("AB_S3_BUCKET", bucket)
        self.env.set("AB_S3_REGION", "garage")
        for key in ("AB_S3_ACCESS_KEY", "AB_S3_SECRET_KEY", "AB_REPO_GPG_KEY"):
            self.env.set(key, self.env.get(key))
        for key, default in [("AB_UPLOAD_TTL", "86400"), ("AB_DOWNLOAD_TTL", "900"), ("AB_REPO_MAX_BYTES", "4294967296")]:
            value = self.env.get(key) or default
            if not value.isdigit() or int(value) < 1 or (key.endswith("TTL") and int(value) > 604800):
                raise SetupError(f"Invalid {key}; URL lifetimes must be 1–604800 seconds and size limits positive.")
            self.env.set(key, value)
        repo_user = ask("Repository username", self.env.get("AB_REPO_USER") or "builder")
        if ":" in repo_user:
            raise SetupError("Repository usernames cannot contain a colon.")
        self.env.set("AB_REPO_USER", repo_user)
        self.env.set("AB_REPO_PASSWORD", ask("Repository password (Enter keeps existing or uses a generated password)",
                                           self.env.get("AB_REPO_PASSWORD") or secrets.token_urlsafe(32), True))
        name = ask("Repository name", self.env.get("AB_REPO_NAME") or "aur-builder")
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9@_+.-]*", name):
            raise SetupError("Use a repository name beginning with a letter or digit.")
        self.env.set("AB_REPO_NAME", name)
        web_url = origin(ask("Web UI HTTPS origin", self.env.get("AB_WEB_URL")))
        if urlsplit(web_url).scheme != "https":
            raise SetupError("The web UI requires HTTPS for OIDC session cookies.")
        self.env.set("AB_WEB_URL", web_url)
        for key, label in [("AB_OIDC_ISSUER", "OIDC issuer URL"),
                           ("AB_OIDC_CLIENT_ID", "OIDC client ID"),
                           ("AB_OIDC_CLIENT_SECRET", "OIDC client secret"),
                           ("AB_OIDC_ADMIN_GROUP", "OIDC administrator group")]:
            self.env.set(key, ask(label, self.env.get(key), key.endswith("SECRET")))
        self.env.set("AB_OIDC_GROUPS_CLAIM", self.env.get("AB_OIDC_GROUPS_CLAIM") or "groups")
        self.env.set("AB_AI_MODE", self.env.get("AB_AI_MODE") or "disabled")
        self.env.save()
        for directory in ("db", "server_tmp"):
            (self.root / directory).mkdir(exist_ok=True)
        config = self.root / "config.yaml"
        if config.exists():
            print("Keeping existing config.yaml.")
        else:
            packages = ask("AUR package names (space-separated; leave empty to configure later)", required=False).split()
            if any(not re.fullmatch(r"[a-z0-9][a-z0-9@._+-]*", name) for name in packages):
                raise SetupError("Invalid AUR package name. Edit config.yaml and rerun setup.")
            git_sources = ask("Git repository URLs (space-separated; empty for none)", required=False).split()
            git_packages = []
            for source in git_sources:
                parsed = urlsplit(source)
                if not ((parsed.scheme in ("http", "https", "ssh", "git") and parsed.hostname)
                        or re.fullmatch(r"[^\s@]+@[^\s:]+:.+", source)):
                    raise SetupError("Use a Git HTTP(S), SSH, git:// or git@host:path URL.")
                subfolder = ask("Package subfolder (empty for repository root)", required=False)
                if Path(subfolder).is_absolute() or ".." in Path(subfolder).parts:
                    raise SetupError("Package subfolders must stay inside the checkout.")
                git_packages.append("  - source: " + json.dumps(source) + "\n" +
                                    ("    subfolder: " + json.dumps(subfolder) + "\n" if subfolder else ""))
            config.write_text("sleepduration: 300\n\naur_packages:" +
                              ("\n" + "".join(f"  - name: {name}\n" for name in dict.fromkeys(packages)) if packages else " []\n") +
                              "\ngit_packages:" + ("\n" + "".join(git_packages) if git_packages else " []\n"))
        print("Saved .env with owner-only permissions. Existing unrelated variables were preserved.")

    def wait(self, label, probe, seconds=120):
        print(f"Waiting for {label}…")
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if probe():
                return
            time.sleep(2)
        raise SetupError(f"Timed out waiting for {label}. Inspect docker compose logs and rerun setup.")

    def provision_garage(self):
        status = self.garage("status").stdout
        nodes = re.findall(r"^([0-9a-f]{16,64})\s+(.+)$", status, re.M)
        if len(nodes) != 1:
            raise SetupError("Automatic layout setup requires exactly one Garage node; configure multi-node layouts manually.")
        node, details = nodes[0]
        if "NO ROLE ASSIGNED" in details:
            layout = self.garage("layout", "show").stdout
            if not re.search(r"(?:version|Version)\s*:?\s*0\b", layout):
                raise SetupError("Existing Garage layout needs manual attention; setup will not overwrite it.")
            capacity = ask("Garage storage capacity", "10G")
            if not re.fullmatch(r"[1-9][0-9]*(?:[KMGTPE]i?B?|B)?", capacity, re.I):
                raise SetupError("Invalid storage capacity; use e.g. 10G or 100G.")
            self.garage("layout", "assign", "-z", "local", "-c", capacity, node)
            self.garage("layout", "apply", "--version", "1")
        bucket = self.env.get("AB_S3_BUCKET")
        info = self.garage("bucket", "info", bucket, check=False)
        if info.returncode:
            if not re.search(r"not exist|not found|no such", info.stderr, re.I):
                raise SetupError("Cannot inspect the Garage bucket; inspect Garage logs before continuing.")
            self.garage("bucket", "create", bucket)
            info = self.garage("bucket", "info", bucket)
        if not re.search(r"Website access:\s+false", info.stdout):
            raise SetupError("The selected bucket has website access enabled or cannot be verified private. Choose a private bucket.")
        key_name = self.env.get("AB_S3_ACCESS_KEY")
        if not key_name:
            # Listing succeeds independently of a missing-key lookup's error wording.
            listing = self.garage("key", "list").stdout
            matching = [line.split()[0] for line in listing.splitlines()
                        if re.search(r"\saur-builder-setup\s*$", line)]
            if len(matching) > 1:
                raise SetupError("Several Garage keys are named aur-builder-setup; set AB_S3_ACCESS_KEY explicitly.")
            key_name = matching[0] if matching else None
        if key_name:
            key = self.garage("key", "info", "--show-secret", key_name)
        else:
            key = self.garage("key", "create", "aur-builder-setup")
        access = re.search(r"Key ID:\s*(\S+)", key.stdout)
        secret = re.search(r"Secret key:\s*(\S+)", key.stdout, re.I)
        if not access or not secret:
            raise SetupError("Unexpected Garage key output; credentials were not changed.")
        if self.env.get("AB_S3_SECRET_KEY") and self.env.get("AB_S3_SECRET_KEY") != secret[1]:
            raise SetupError("The configured S3 secret does not match Garage; correct it before rerunning setup.")
        self.env.set("AB_S3_ACCESS_KEY", access[1])
        self.env.set("AB_S3_SECRET_KEY", secret[1])
        self.env.save()  # Persist immediately, before any later step can fail.
        self.garage("bucket", "allow", "--read", "--write", "--key", access[1], bucket)
        print("Private Garage bucket and access key are ready. Credentials saved to .env.")

    def signing(self):
        home = self.root / "repo-gnupg"
        home.mkdir(mode=0o700, exist_ok=True)
        home.chmod(0o700)
        def gpg(*args, **kwargs):
            return run(["gpg", "--homedir", str(home), "--batch", *args], self.root, capture=True, **kwargs)
        def fingerprints():
            output = gpg("--with-colons", "--list-secret-keys").stdout
            found, need_fingerprint = [], False
            for line in output.splitlines():
                fields = line.split(":")
                if fields[0] == "sec":
                    need_fingerprint = True
                elif fields[0] == "fpr" and need_fingerprint:
                    found.append(fields[9])
                    need_fingerprint = False
            return found
        keys = fingerprints()
        if not keys:
            if yes("Generate a dedicated unattended repository signing key? (No: import a secret key)"):
                identity = ask("Signing key identity", "AUR Builder <builder@localhost>")
                gpg("--pinentry-mode", "loopback", "--passphrase", "", "--quick-generate-key", identity, "ed25519", "sign", "1y")
            else:
                source = Path(ask("Secret-key export file to import")).expanduser()
                if not source.is_file():
                    raise SetupError("Signing key export file not found.")
                gpg("--import", str(source.resolve()))
            keys = fingerprints()
        if not keys:
            raise SetupError("No signing secret key found in repo-gnupg.")
        current = self.env.get("AB_REPO_GPG_KEY").upper()
        if current and current not in keys:
            raise SetupError("AB_REPO_GPG_KEY does not match a key in repo-gnupg; select the correct key before rerunning.")
        if current:
            selected = current
        elif len(keys) == 1:
            selected = keys[0]
        else:
            print("Available signing fingerprints:\n" + "\n".join(keys))
            selected = ask("Signing fingerprint").upper()
            if selected not in keys:
                raise SetupError("Choose a fingerprint listed above.")
        # A cached host-agent passphrase must not make a protected key appear deployable.
        run(["gpgconf", "--homedir", str(home), "--kill", "gpg-agent"], self.root, capture=True)
        with tempfile.TemporaryDirectory() as directory:
            probe = Path(directory) / "signing-test"
            probe.write_text("AUR Builder setup signing check\n")
            try:
                gpg("--pinentry-mode", "error", "--local-user", selected, "--detach-sign", str(probe))
            except SetupError as error:
                raise SetupError("The selected key cannot sign unattended. Use an unexpired signing key without an interactive passphrase in repo-gnupg.") from error
        public_key = gpg("--armor", "--export", selected).stdout
        if not public_key:
            raise SetupError("Unable to export the signing public key.")
        (self.root / "repo-public-key.asc").write_text(public_key)
        self.env.set("AB_REPO_GPG_KEY", selected)
        self.env.save()
        run(["gpgconf", "--homedir", str(home), "--kill", "gpg-agent"], self.root, capture=True)
        print("Signing verified. Public key exported to repo-public-key.asc.")

    def database_ready(self):
        path = self.root / "db" / "sqlite.db"
        if not path.exists():
            return False
        try:
            with sqlite3.connect(f"file:{path}?mode=ro", uri=True, timeout=1) as db:
                db.execute("SELECT catalog FROM repo_catalog LIMIT 1").fetchall()
                db.execute("SELECT attempt FROM build_attempt LIMIT 1").fetchall()
            return True
        except sqlite3.Error:
            return False

    def ensure_running(self, service):
        ids = self.compose("ps", "--all", "--quiet", service, capture=True).stdout.split()
        if not ids:
            raise SetupError(f"No container found for {service}; inspect docker compose logs {service}.")
        for container in ids:
            state = run(["docker", "inspect", "--format", "{{.State.Running}} {{.RestartCount}}", container], self.root, capture=True).stdout.strip()
            fields = state.split()
            if len(fields) != 2 or fields[0] != "true" or not fields[1].isdigit():
                raise SetupError(f"{service} is not running; inspect docker compose logs {service} and rerun setup.")
            count = int(fields[1])
            if container in self.restart_counts and count > self.restart_counts[container]:
                raise SetupError(f"{service} is restarting; inspect docker compose logs {service} and rerun setup.")
            self.restart_counts[container] = count

    def deploy(self):
        for tool in ("docker", "gpg", "gpgconf"):
            if not shutil.which(tool):
                raise SetupError(f"Install {tool} before running full setup, or use --configure-only.")
        self.compose("version", capture=True)
        run(["docker", "info"], self.root, capture=True)
        print("\nExisting RabbitMQ users and volumes are preserved; changing .env does not reset broker passwords.")
        print("Existing installations need the queue migration described in STORAGE.md; setup never deletes queues or volumes.")
        if not yes("Build images and start Docker Compose services now?", False):
            print("Configuration saved. Rerun setup when ready to start services.")
            return
        self.compose("config", "--quiet", capture=True)
        self.compose("up", "-d", "garage", "rabbitmq")
        self.wait("Garage", lambda: self.garage("status", check=False).returncode == 0)
        self.provision_garage()
        self.signing()
        self.wait("RabbitMQ", lambda: self.compose("exec", "-T", "rabbitmq", "rabbitmq-diagnostics", "-q", "check_running", capture=True, check=False).returncode == 0)
        if yes("Build a local Arch build-container image from this checkout?"):
            image = "aur-builder-build-container"
            tag = "setup-local"
            run(["docker", "build", "-t", f"{image}:{tag}", "build-container"], self.root)
            self.env.set("AB_BUILDER", image)
            self.env.set("AB_BUILDER_TAG", tag)
            self.env.save()
        self.compose("build", "server", "worker", "web", "repo")
        self.compose("up", "-d", "--no-deps", "server")
        self.wait("database migrations", self.database_ready)
        self.ensure_running("server")
        self.compose("up", "-d", "--no-deps", "worker", "repo", "web")
        for service in ("worker", "repo", "web"):
            self.ensure_running(service)
        if yes("Configure and start SMTP email notifications?", False):
            for key, label in [("AB_SMTP_HOST", "SMTP host"), ("AB_SMTP_USER", "SMTP username"),
                               ("AB_SMTP_PASS", "SMTP password"), ("AB_SMTP_FROM", "Sender address"),
                               ("AB_SMTP_TO", "Recipient address"), ("AB_MAILLOGO", "Email logo URL")]:
                self.env.set(key, ask(label, self.env.get(key), key == "AB_SMTP_PASS"))
            self.env.save()
            self.compose("up", "-d", "--build", "notifier")
        print("Checking initial service startup…")
        time.sleep(5)
        for service in ("server", "worker", "web", "repo"):
            self.ensure_running(service)
        self.compose("ps")
        print(f"\nServices started. Dashboard: {self.env.get('AB_WEB_URL')}; repo: port 3001.")
        print("Configure HTTPS for the dashboard, repo and S3. Sign in with the configured OIDC administrator group and approve releases.")
        print("Credentials are in .env. Trust repo-public-key.asc on pacman clients as described in README.md.")
        print("If no packages were configured, edit config.yaml and restart server.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--configure-only", action="store_true", help="write .env and config.yaml without Docker, provisioning, or key generation")
    args = parser.parse_args()
    try:
        setup = Setup()
        setup.configure()
        if args.configure_only:
            print("Configuration saved. S3 credentials and signing will be provisioned during full setup.")
        else:
            setup.deploy()
    except (SetupError, OSError, KeyboardInterrupt, EOFError) as error:
        print(f"\nSetup stopped: {error or 'interrupted'}. Existing data was not deleted; rerun to continue.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
