"""Run with: python3 -m unittest discover -s scripts -p 'test_setup.py'."""
import importlib.util
from pathlib import Path
import os
import stat
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("aur_setup", Path(__file__).resolve().parents[1] / "setup.py")
setup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)


class SetupTests(unittest.TestCase):
    def test_env_preserves_unrelated_values_and_protects_secrets(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / ".env"
            path.write_text("# Existing configuration\nCUSTOM=keep-me\nSECRET=old\nSECRET=duplicate\n")
            env = setup.EnvFile(path)
            secret = "don't interpolate ${TOKEN} or $(commands) \\ here"
            env.set("SECRET", secret)
            env.save()
            self.assertEqual(setup.EnvFile(path).get("SECRET"), secret)
            self.assertEqual(setup.EnvFile(path).get("CUSTOM"), "keep-me")
            self.assertIn("# Existing configuration", path.read_text())
            self.assertEqual(path.read_text().count("SECRET="), 1)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            with self.assertRaises(setup.SetupError):
                env.set("SECRET", "value\nINJECTED=true")

    def test_endpoints(self):
        self.assertEqual(setup.origin("https://storage.internal:9443/"), "https://storage.internal:9443")
        for invalid in ["https://storage.example.com", "ftp://storage.local", "https://user:pass@storage.local", "https://storage.local/prefix", "http://storage.local/#part", "http://storage.local:bad"]:
            with self.subTest(invalid=invalid), self.assertRaises(setup.SetupError):
                setup.origin(invalid)

    def test_configuration_only_is_rerunnable_without_docker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".env").write_text("CUSTOM=untouched\nRABBITMQ_DEFAULT_PASS='special@:/password'\n")
            def answer(label, default="", secret=False, required=True):
                if label.startswith("Web UI HTTPS"):
                    return "https://builder.internal"
                if label.startswith("OIDC"):
                    return "https://identity.internal" if "URL" in label else "builder"
                if label.startswith("S3 origin"):
                    return "https://storage.internal"
                if label.startswith("AUR package"):
                    return "example example another"
                return default
            with patch.object(setup, "ask", side_effect=answer), patch.object(setup, "run", side_effect=AssertionError("configure-only must not start a process")):
                wizard = setup.Setup(root)
                wizard.configure()
                original_key = wizard.env.get("GARAGE_RPC_SECRET")
                original_password = wizard.env.get("AB_REPO_PASSWORD")
                config = (root / "config.yaml").read_text()
                wizard = setup.Setup(root)
                wizard.configure()
            env = setup.EnvFile(root / ".env")
            self.assertEqual(env.get("CUSTOM"), "untouched")
            self.assertEqual(env.get("GARAGE_RPC_SECRET"), original_key)
            self.assertEqual(env.get("AB_REPO_PASSWORD"), original_password)
            self.assertIn("special%40%3A%2Fpassword", env.get("AMQP_ADDR"))
            self.assertEqual((root / "config.yaml").read_text(), config)
            self.assertEqual(config.count("name: example\n"), 1)
            self.assertEqual(env.get("AB_S3_ACCESS_KEY"), "")

    def test_mismatched_database_requires_explicit_choice(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / ".env"
            original = "DATABASE_URL=postgres://existing\n"
            path.write_text(original)
            with patch.object(setup, "yes", return_value=False):
                with self.assertRaises(setup.SetupError):
                    setup.Setup(directory).configure()
            self.assertEqual(path.read_text(), original)

    def test_running_check_allows_historical_restarts_but_detects_new_ones(self):
        with tempfile.TemporaryDirectory() as directory:
            wizard=setup.Setup(directory)
            container=subprocess.CompletedProcess([],0,"container-id\n","")
            running=subprocess.CompletedProcess([],0,"true 4\n","")
            restarting=subprocess.CompletedProcess([],0,"true 5\n","")
            with patch.object(wizard,"compose",return_value=container),patch.object(setup,"run",side_effect=[running,running,restarting]):
                wizard.ensure_running("server")
                wizard.ensure_running("server")
                with self.assertRaises(setup.SetupError):
                    wizard.ensure_running("server")

    def test_deployment_starts_migrations_before_dependents(self):
        with tempfile.TemporaryDirectory() as directory:
            wizard = setup.Setup(directory)
            calls=[]
            def compose(*args,**kwargs):
                calls.append(args)
                return subprocess.CompletedProcess(args,0,"","")
            with patch.object(setup.shutil,"which",return_value="/bin/tool"), patch.object(setup,"run"), patch.object(setup,"yes",side_effect=[True,False,False]), patch.object(setup.time,"sleep"), patch.object(wizard,"compose",side_effect=compose), patch.object(wizard,"wait"), patch.object(wizard,"provision_garage"), patch.object(wizard,"signing"), patch.object(wizard,"ensure_running"):
                wizard.deploy()
            server=("up","-d","--no-deps","server")
            dependents=("up","-d","--no-deps","worker","repo","web")
            self.assertLess(calls.index(server),calls.index(dependents))
            self.assertFalse(any("down" in c or "rm" in c for c in calls))

    def test_garage_resume_keeps_key_and_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            wizard = setup.Setup(directory)
            wizard.env.set("AB_S3_BUCKET", "aur-builder")
            wizard.env.set("AB_S3_ACCESS_KEY", "GKexisting")
            wizard.env.set("AB_S3_SECRET_KEY", "existing-secret")
            calls = []
            def garage(*args, **kwargs):
                calls.append(args)
                outputs = {
                    ("status",): "==== HEALTHY NODES ====\n0123456789abcdef host address local 10 GB\n",
                    ("bucket", "info", "aur-builder"): "Website access: false\n",
                    ("key", "info", "--show-secret", "GKexisting"): "Key ID: GKexisting\nSecret key: existing-secret\n",
                }
                return subprocess.CompletedProcess(args, 0, outputs.get(args, ""), "")
            with patch.object(wizard, "garage", side_effect=garage):
                wizard.provision_garage()
            self.assertFalse(any("create" in call or "apply" in call for call in calls))
            self.assertEqual(setup.EnvFile(Path(directory) / ".env").get("AB_S3_SECRET_KEY"), "existing-secret")

    def test_subprocess_failures_do_not_expose_credentials(self):
        result = subprocess.CompletedProcess([], 1, "secret-value", "secret-value")
        with patch.object(setup.subprocess, "run", return_value=result):
            with self.assertRaises(setup.SetupError) as caught:
                setup.run(["docker", "secret-value"], capture=True)
        self.assertNotIn("secret-value", str(caught.exception))

    def test_shell_environment_does_not_override_saved_values(self):
        with tempfile.TemporaryDirectory() as directory:
            wizard = setup.Setup(directory)
            wizard.env.set("AB_REPO_PASSWORD", "saved")
            def execute(*args, **kwargs):
                self.assertNotIn("AB_REPO_PASSWORD", os.environ)
                return subprocess.CompletedProcess([], 0, "", "")
            with patch.dict(os.environ, {"AB_REPO_PASSWORD": "exported"}), patch.object(setup, "run", side_effect=execute):
                wizard.compose("config", capture=True)
                self.assertEqual(os.environ["AB_REPO_PASSWORD"], "exported")


if __name__ == "__main__":
    unittest.main()
