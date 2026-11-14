#!/usr/bin/env python3
"""Host-side integration check for this stack; no third-party Python packages."""
import base64
from email.message import EmailMessage
import http.client
import json
from pathlib import Path
import smtplib
import socket
import ssl
import tempfile
import uuid

from run import compose, environment, provision_storage, wait


def main():
    env = environment()
    with tempfile.TemporaryDirectory(prefix="aur-dev-smoke-") as temp:
        certificate = Path(temp) / "ca.crt"
        compose("cp", "keycloak:/certs/ca.crt", str(certificate))
        context = ssl.create_default_context(cafile=str(certificate))

        def https(host, path):
            connection = http.client.HTTPSConnection(host, 8443, context=context, timeout=5)
            # Exercise actual TLS hostname verification without requiring host DNS edits.
            connection._create_connection = lambda address, timeout, source_address: socket.create_connection(("127.0.0.1", 8443), timeout)
            try:
                connection.request("GET", path)
                response = connection.getresponse()
                body = response.read()
                if response.status != 200:
                    raise OSError(f"{host}{path}: HTTP {response.status}")
                return json.loads(body)
            finally:
                connection.close()

        metadata = wait("Keycloak HTTPS discovery", lambda: https("auth.localhost", "/realms/aur-builder/.well-known/openid-configuration"))
        assert metadata["issuer"] == env["AB_OIDC_ISSUER"], metadata["issuer"]
        assert metadata["token_endpoint"].startswith("https://auth.localhost:8443/")
        assert (wait("Keycloak JWKS", lambda: https("auth.localhost", "/realms/aur-builder/protocol/openid-connect/certs")))["keys"]
        users = json.loads(compose("exec", "-T", "keycloak", "sh", "-ec",
            '/opt/keycloak/bin/kcadm.sh config credentials --server http://localhost:8080 --realm master --user keycloak-admin --password dev-keycloak-password >/dev/null; /opt/keycloak/bin/kcadm.sh get users -r aur-builder', capture=True).stdout)
        expected = {u["username"]: u["id"] for u in json.loads((Path(__file__).parent / "keycloak/aur-builder-realm.json").read_text())["users"]}
        assert {u["username"]: u["id"] for u in users} == expected

        def rabbit():
            connection = http.client.HTTPConnection("127.0.0.1", 15672, timeout=5)
            try:
                credentials = base64.b64encode(b"builder:dev-rabbitmq-password").decode()
                connection.request("GET", "/api/overview", headers={"Authorization": "Basic " + credentials})
                response = connection.getresponse()
                data = response.read()
                return response.status == 200 and json.loads(data)["rabbitmq_version"]
            finally:
                connection.close()
        wait("RabbitMQ management authentication", rabbit)
        provision_storage()
        provision_storage()  # Must be safe after rebuilding/reopening the workspace.

        subject = "aur-dev-smoke-" + uuid.uuid4().hex
        message = EmailMessage()
        message["From"], message["To"], message["Subject"] = env["AB_SMTP_FROM"], env["AB_SMTP_TO"], subject
        message.set_content("Development SMTP capture works over authenticated TLS.")
        with smtplib.SMTP_SSL("localhost", 1465, context=context, timeout=10) as smtp:
            smtp.login(env["AB_SMTP_USER"], env["AB_SMTP_PASS"])
            smtp.send_message(message)
        mailbox = https("mail.localhost", "/api/v1/messages")
        assert any(m["Subject"] == subject for m in mailbox["messages"]), "Message not captured"
        print("PASS: trusted HTTPS, Keycloak realm/users/JWKS, RabbitMQ auth, Garage provisioning, SMTP capture")


if __name__ == "__main__":
    main()
