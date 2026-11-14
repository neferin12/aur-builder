import base64
import hashlib
import json
from pathlib import Path
import re
import tempfile
import unittest
from provision_broker import provision, EnvFile, ROLES

class BrokerProvisionTests(unittest.TestCase):
    def test_credentials_are_stable_private_and_separated(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / '.env').write_text('RABBITMQ_DEFAULT_USER=admin\nRABBITMQ_DEFAULT_PASS=admin-secret\nAB_OIDC_CLIENT_SECRET=oidc-secret\nAB_REPO_PASSWORD=repo-secret\nDATABASE_URL=sqlite:/db\n')
            provision(root)
            first = (root / '.env.worker').read_text()
            provision(root)
            self.assertEqual(first, (root / '.env.worker').read_text())
            self.assertEqual((root / '.env.worker').stat().st_mode & 0o777, 0o600)
            for role in ROLES:
                data = (root / ('.env.' + role)).read_text()
                self.assertNotIn('admin-secret', data)
                self.assertNotIn('oidc-secret', data)
                self.assertIn('amqp://ab-' + role + ':', data)
            self.assertNotIn('repo-secret', first)
            artifact_key = EnvFile(root / '.env.worker').get('AB_ARTIFACT_KEY')
            self.assertGreaterEqual(len(artifact_key), 32)
            self.assertEqual(artifact_key, EnvFile(root / '.env.repo').get('AB_ARTIFACT_KEY'))
            for role in ('server', 'web', 'notifier', 'migrate'):
                self.assertNotIn(artifact_key, (root / ('.env.' + role)).read_text())
            definitions = json.loads((root / 'rabbitmq-security/definitions.json').read_text())
            for user in definitions['users']:
                raw = base64.b64decode(user['password_hash'])
                password = 'admin-secret' if user['name'] == 'admin' else EnvFile(root / '.env').get('AB_AMQP_' + user['name'][3:].upper() + '_PASSWORD')
                self.assertEqual(raw[4:], hashlib.sha256(raw[:4] + password.encode()).digest())
            for permission in definitions['permissions']:
                self.assertFalse(re.fullmatch(permission['write'], 'amq.default'))
                self.assertFalse(re.fullmatch(permission['configure'], 'pkg_build'))
                if permission['user'] != 'ab-worker':
                    self.assertFalse(re.fullmatch(permission['read'], 'pkg_build'))
