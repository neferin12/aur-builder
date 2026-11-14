#!/usr/bin/env python3
"""Smoke-test the production non-root web image against a migrated temporary DB."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.request
import uuid
ROOT = Path(__file__).resolve().parents[1]
name = 'aur-web-security-' + uuid.uuid4().hex[:12]
with tempfile.TemporaryDirectory(prefix='aur-web-container-') as directory:
    root = Path(directory)
    env = dict(os.environ, DATABASE_URL='sqlite:' + str(root / 'database.sqlite') + '?mode=rwc')
    subprocess.run(['cargo', 'run', '-p', 'database', '--example', 'migrate', '--locked'], cwd=ROOT, env=env, check=True, stdout=subprocess.DEVNULL)
    def ownership(uid):
        subprocess.run(['docker', 'run', '--rm', '--user', '0:0', '-v', str(root) + ':/data', 'alpine', 'chown', '-R', uid, '/data'], check=True)
    try:
        ownership('10001:10001')
        args = ['docker', 'run', '-d', '--name', name, '-p', '127.0.0.1::3000', '-v', str(root) + ':/data']
        for key, value in {'DATABASE_URL': 'sqlite:/data/database.sqlite', 'AB_WEB_URL': 'https://builder.example', 'AB_OIDC_ISSUER': 'https://issuer.example', 'AB_OIDC_CLIENT_ID': 'test', 'AB_OIDC_CLIENT_SECRET': 'test-only', 'AB_OIDC_ADMIN_GROUP': 'admins'}.items():
            args += ['-e', key + '=' + value]
        subprocess.run(args + [os.environ.get('AB_WEB_TEST_IMAGE', 'aur-builder-security-web')], check=True, stdout=subprocess.DEVNULL)
        port = subprocess.check_output(['docker', 'port', name, '3000'], text=True).strip().rsplit(':', 1)[1]
        for _ in range(30):
            try:
                with urllib.request.urlopen('http://127.0.0.1:' + port + '/auth/signed-out', timeout=2) as response:
                    assert response.status == 200
                    assert "default-src 'none'" in response.headers['content-security-policy']
                    assert response.headers['cross-origin-opener-policy'] == 'same-origin'
                break
            except OSError:
                time.sleep(1)
        else:
            subprocess.run(['docker', 'logs', name])
            raise RuntimeError('Web container did not become ready')
        assert subprocess.check_output(['docker', 'exec', name, 'id', '-u'], text=True).strip() == '10001'
        print('Production web image starts as UID 10001 against a migrated database')
    finally:
        subprocess.run(['docker', 'rm', '-f', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        ownership(f'{os.getuid()}:{os.getgid()}')
