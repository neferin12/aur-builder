"""Offline contract tests for the local development environment."""
from contextlib import closing
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("dev_commands", ROOT / ".devcontainer/run.py")
dev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dev)


class DevcontainerTests(unittest.TestCase):
    def setUp(self):
        self.realm = json.loads((ROOT / ".devcontainer/keycloak/aur-builder-realm.json").read_text())
        self.env = dev.environment()

    def test_oidc_contract_matches_application_and_password_fixtures(self):
        client = self.realm["clients"][0]
        self.assertEqual(client["clientId"], self.env["AB_OIDC_CLIENT_ID"])
        self.assertEqual(client["secret"], self.env["AB_OIDC_CLIENT_SECRET"])
        self.assertEqual(client["redirectUris"], [self.env["AB_WEB_URL"] + "/auth/callback"])
        self.assertFalse(client["directAccessGrantsEnabled"])
        self.assertEqual(client["attributes"]["pkce.code.challenge.method"], "S256")
        mapper = client["protocolMappers"][0]
        self.assertEqual(mapper["config"]["claim.name"], self.env["AB_OIDC_GROUPS_CLAIM"])
        self.assertEqual(mapper["config"]["full.path"], "false")
        self.assertEqual(mapper["config"]["id.token.claim"], "true")
        users = {u["username"]: u for u in self.realm["users"]}
        self.assertEqual(set(users), {"admin", "reviewer", "viewer", "pending"})
        self.assertIn("/" + self.env["AB_OIDC_ADMIN_GROUP"], users["admin"]["groups"])
        for user in users.values():
            self.assertTrue(user["enabled"])
            self.assertEqual(user["requiredActions"], [])
            self.assertFalse(user["credentials"][0]["temporary"])

    def test_users_are_seeded_with_correct_subjects_and_preserve_access_edits(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "dev.sqlite"
            with closing(sqlite3.connect(path)) as db:
                db.execute("CREATE TABLE web_user (id TEXT PRIMARY KEY, issuer TEXT, subject TEXT, display_name TEXT, admin BOOLEAN, can_view BOOLEAN, can_rebuild BOOLEAN, can_approve BOOLEAN, UNIQUE(issuer,subject))")
                db.commit()
                dev.seed_users(path, self.realm)
                rows = {r[0]: r[1:] for r in db.execute("SELECT subject,admin,can_view,can_rebuild,can_approve FROM web_user")}
                users = {u["username"]: u["id"] for u in self.realm["users"]}
                self.assertEqual(rows[users["admin"]], (1, 1, 1, 1))
                self.assertEqual(rows[users["reviewer"]], (0, 1, 1, 1))
                self.assertEqual(rows[users["viewer"]], (0, 1, 0, 0))
                self.assertEqual(rows[users["pending"]], (0, 0, 0, 0))
                db.execute("UPDATE web_user SET can_approve=0 WHERE subject=?", (users["reviewer"],))
                db.commit()
                dev.seed_users(path, self.realm)
                self.assertEqual(db.execute("SELECT can_approve FROM web_user WHERE subject=?", (users["reviewer"],)).fetchone(), (0,))
                self.assertEqual(db.execute("SELECT COUNT(*) FROM web_user").fetchone(), (4,))

    def test_bootstrap_uses_the_editors_actual_compose_project(self):
        dev.compose_project.cache_clear()
        try:
            with patch.dict(dev.os.environ, {"AUR_DEVCONTAINER": "1", "HOSTNAME": "dev-container-id"}), patch.object(dev, "run") as run:
                run.return_value.stdout = "editor_project\n"
                dev.compose("exec", "-T", "garage", "/garage", "status")
                args = run.call_args_list[-1].args
                self.assertEqual(args[:4], ("docker", "compose", "--project-name", "editor_project"))
        finally:
            dev.compose_project.cache_clear()

    def test_dev_configuration_overrides_exported_production_settings(self):
        with patch.dict(dev.os.environ, {"AB_OIDC_CLIENT_SECRET": "production", "DATABASE_URL": "production", "AMQP_ADDR": "production"}):
            env = dev.environment()
        self.assertEqual(env["AB_OIDC_CLIENT_SECRET"], "dev-oidc-secret")
        self.assertIn(".devcontainer/state/dev.sqlite", env["DATABASE_URL"])
        self.assertIn("dev-rabbitmq-password", env["AMQP_ADDR"])
        self.assertRegex(env["AB_S3_ACCESS_KEY"], r"^GK[0-9a-f]{24}$")
        self.assertNotEqual(env["TEST_POSTGRESQL_URL"], env["PRISMA_POSTGRESQL_URL"])


if __name__ == "__main__":
    unittest.main()
