"""Opt-in runtime staging test: AB_RUNTIME_INTEGRATION=1 python3 -m unittest discover -s scripts -p test_runtime_permissions.py."""
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import unittest
from prepare_runtime import prepare

@unittest.skipUnless(os.environ.get('AB_RUNTIME_INTEGRATION') == '1', 'requires Docker')
class RuntimePermissionsTests(unittest.TestCase):
    def test_staging_preserves_operator_keys_and_is_rerunnable(self):
        with tempfile.TemporaryDirectory(prefix='aur-runtime-test-') as directory:
            root = Path(directory)
            source = root / 'repo-gnupg'
            source.mkdir(mode=0o700)
            (source / 'private-keys-v1.d').mkdir(mode=0o700)
            secret = source / 'private-keys-v1.d' / 'fixture.key'
            secret.write_text('test key material')
            secret.chmod(0o600)
            owner = (source.stat().st_uid, secret.stat().st_uid)
            agent = socket.socket(socket.AF_UNIX)
            agent.bind(str(source / 'S.gpg-agent'))
            try:
                prepare(root)
                prepare(root)
                self.assertEqual(owner, (source.stat().st_uid, secret.stat().st_uid))
                self.assertEqual(secret.read_text(), 'test key material')
                subprocess.run(['docker', 'run', '--rm', '--user', '10001:10001', '--volume', str(root / 'repo-signing-runtime') + ':/keys:ro', 'alpine', 'sh', '-ec', 'test -r /keys/private-keys-v1.d/fixture.key; test ! -e /keys/S.gpg-agent'], check=True)
            finally:
                agent.close()
                args = ['docker', 'run', '--rm', '--user', '0:0']
                for item in ('db', 'server_tmp', 'repo-signing-runtime'):
                    args += ['--volume', str(root / item) + ':/' + item]
                subprocess.run(args + ['alpine', 'chown', '-R', f'{os.getuid()}:{os.getgid()}', '/db', '/server_tmp', '/repo-signing-runtime'], check=True)
