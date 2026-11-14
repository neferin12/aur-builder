#!/usr/bin/env python3
"""Disposable real Garage, GPG, and pacman publication integration test."""
from pathlib import Path
import os
import json
import secrets
import subprocess
import sys
import tempfile
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from setup import Setup, ask
ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='aur-publication-test-') as directory:
    root = Path(directory)
    (root / 'garage.toml').write_text('''metadata_dir = "/tmp/meta"
data_dir = "/tmp/data"
db_engine = "sqlite"
replication_factor = 1
rpc_bind_addr = "[::]:3901"
rpc_public_addr = "127.0.0.1:3901"
[s3_api]
s3_region = "garage"
api_bind_addr = "[::]:3900"
''')
    (root / 'docker-compose.yaml').write_text('''services:
  garage:
    image: dxflrs/garage:v2.1.0
    environment:
      GARAGE_RPC_SECRET: ${GARAGE_RPC_SECRET}
    ports: ["127.0.0.1::3900"]
    volumes:
      - ./garage.toml:/etc/garage.toml:ro
''')
    wizard = Setup(root)
    wizard.env.set('GARAGE_RPC_SECRET', secrets.token_hex(32))
    wizard.env.set('AB_S3_BUCKET', 'security-test')
    wizard.env.save()
    try:
        wizard.compose('up', '-d', capture=True)
        wizard.wait('disposable Garage', lambda: wizard.garage('status', check=False).returncode == 0, seconds=60)
        with patch('setup.ask', side_effect=lambda label, default='', **kw: default), patch('setup.yes', return_value=True):
            wizard.provision_garage()
            wizard.signing()
        endpoint = 'http://' + wizard.compose('port', 'garage', '3900', capture=True).stdout.strip()
        env = dict(os.environ, GNUPGHOME=str(root / 'repo-gnupg'), AB_S3_ENDPOINT=endpoint, AB_S3_PUBLIC_ENDPOINT=endpoint, AB_S3_REGION='garage', AB_ARTIFACT_KEY=secrets.token_urlsafe(48))
        for key in ('AB_S3_BUCKET', 'AB_S3_ACCESS_KEY', 'AB_S3_SECRET_KEY', 'AB_REPO_GPG_KEY'):
            env[key] = wizard.env.get(key)
        if os.environ.get('AB_PUBLICATION_IN_CONTAINER') == '1':
            # Ubuntu CI supplies the Rust executable; the already-built Arch image
            # supplies repo-add, pacman, fakeroot, bsdtar and GPG for the test only.
            output = subprocess.check_output(['cargo', 'test', '-p', 'repo', '--no-run', '--message-format=json', '--locked'], cwd=ROOT, text=True)
            records = [json.loads(line) for line in output.splitlines() if line.startswith('{')]
            executable = next(record['executable'] for record in records if record.get('reason') == 'compiler-artifact' and record.get('target', {}).get('name') == 'repo' and record.get('executable') and record.get('profile', {}).get('test'))
            args = ['docker', 'run', '--rm', '--network', 'host', '--read-only', '--user', '0:0', '--tmpfs', '/tmp:size=256m,mode=1777,exec', '-v', executable + ':/repo-tests:ro', '-v', str(root / 'repo-gnupg') + ':/keys:ro']
            for key in ('AB_S3_ENDPOINT', 'AB_S3_PUBLIC_ENDPOINT', 'AB_S3_REGION', 'AB_S3_BUCKET', 'AB_S3_ACCESS_KEY', 'AB_S3_SECRET_KEY', 'AB_REPO_GPG_KEY', 'AB_ARTIFACT_KEY'):
                args += ['-e', key + '=' + env[key]]
            args += ['-e', 'GNUPGHOME=/tmp/gnupg', '--entrypoint', '/bin/bash', 'aur-builder-rootless-test', '-ec', 'mkdir /tmp/gnupg; cp -R /keys/private-keys-v1.d /tmp/gnupg/; cp /keys/pubring.kbx /keys/trustdb.gpg /tmp/gnupg/; chown -R 1000:1000 /tmp/gnupg; chmod 700 /tmp/gnupg; exec setpriv --reuid=1000 --regid=1000 --init-groups /repo-tests private_signed_publication --ignored']
            subprocess.run(args, check=True)
        else:
            subprocess.run(['cargo', 'test', '-p', 'repo', 'private_signed_publication', '--locked', '--', '--ignored'], cwd=ROOT, env=env, check=True)
    finally:
        wizard.compose('down', '--volumes', capture=True, check=False)
        subprocess.run(['gpgconf', '--homedir', str(root / 'repo-gnupg'), '--kill', 'gpg-agent'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
