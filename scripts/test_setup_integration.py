"""Opt-in disposable Garage/GPG test: AB_SETUP_INTEGRATION=1 python3 -m unittest discover -s scripts -p test_setup_integration.py."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from test_setup import setup


@unittest.skipUnless(os.environ.get("AB_SETUP_INTEGRATION") == "1", "requires Docker, Garage image and GPG")
class SetupIntegrationTests(unittest.TestCase):
    def test_fresh_setup_and_resume(self):
        with tempfile.TemporaryDirectory(prefix="aur-setup-test-") as directory:
            root = Path(directory)
            (root / "garage.toml").write_text('''metadata_dir = "/tmp/meta"
data_dir = "/tmp/data"
db_engine = "sqlite"
replication_factor = 1
rpc_bind_addr = "[::]:3901"
rpc_public_addr = "127.0.0.1:3901"
[s3_api]
s3_region = "garage"
api_bind_addr = "[::]:3900"
''')
            (root / "docker-compose.yaml").write_text('''services:
  garage:
    image: dxflrs/garage:v2.1.0
    environment:
      GARAGE_RPC_SECRET: ${GARAGE_RPC_SECRET}
    volumes:
      - ./garage.toml:/etc/garage.toml:ro
''')
            wizard = setup.Setup(root)
            for key, value in [("GARAGE_RPC_SECRET", "a" * 64), ("AB_S3_BUCKET", "setup-test")]:
                wizard.env.set(key, value)
            wizard.env.save()
            try:
                wizard.compose("up", "-d", capture=True)
                wizard.wait("test Garage", lambda: wizard.garage("status", check=False).returncode == 0, seconds=45)
                with patch.object(setup, "ask", side_effect=lambda label, default="", **kw: default), patch.object(setup, "yes", return_value=True):
                    wizard.provision_garage()
                    wizard.signing()
                    original = wizard.env.get("AB_S3_ACCESS_KEY"), wizard.env.get("AB_S3_SECRET_KEY"), wizard.env.get("AB_REPO_GPG_KEY")
                    self.assertTrue(all(original))
                    resumed = setup.Setup(root)
                    resumed.provision_garage()
                    resumed.signing()
                    self.assertEqual(original, (resumed.env.get("AB_S3_ACCESS_KEY"), resumed.env.get("AB_S3_SECRET_KEY"), resumed.env.get("AB_REPO_GPG_KEY")))
                self.assertIn("BEGIN PGP PUBLIC KEY BLOCK", (root / "repo-public-key.asc").read_text())
                # Verify Docker Compose reads dotenv values exactly, including interpolation characters.
                secret = "it's $literal ${NOT_EXPANDED} \\ text # secret"
                wizard.env.set("GARAGE_RPC_SECRET", secret)
                wizard.env.save()
                import json
                wizard.compose("create", "--force-recreate", "garage", capture=True)
                container=wizard.compose("ps", "--all", "--quiet", "garage", capture=True).stdout.strip()
                configured=json.loads(setup.run(["docker","inspect","--format","{{json .Config.Env}}",container], root, capture=True).stdout)
                self.assertIn("GARAGE_RPC_SECRET="+secret,configured)
            finally:
                wizard.compose("down", "--volumes", capture=True, check=False)
                setup.run(["gpgconf", "--homedir", str(root / "repo-gnupg"), "--kill", "gpg-agent"], root, capture=True, check=False)


if __name__ == "__main__":
    unittest.main()
