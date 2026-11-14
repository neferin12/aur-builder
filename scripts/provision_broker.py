#!/usr/bin/env python3
"""Generate broker topology and least-privilege service environments before Compose starts.
Rerun after changing .env; restart RabbitMQ and application containers afterward.
"""
import base64
import hashlib
import json
from pathlib import Path
import secrets
import sys
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from setup import EnvFile, SetupError

QUEUES = ('pkg_build', 'build_results', 'expired_tasks', 'repo_ingest', 'repo_ingest_failed', 'notifications')
ROLES = {
    'server': (('pkg_build', 'repo_ingest', 'notifications'), ('build_results', 'expired_tasks')),
    'worker': (('build_results', 'expired_tasks'), ('pkg_build',)),
    'repo': ((), ('repo_ingest',)),
    'notifier': ((), ('notifications',)),
}
S3 = ('AB_S3_ENDPOINT', 'AB_S3_PUBLIC_ENDPOINT', 'AB_S3_BUCKET', 'AB_S3_REGION', 'AB_S3_ACCESS_KEY', 'AB_S3_SECRET_KEY', 'AB_UPLOAD_TTL')
KEYS = {
    'server': ('DATABASE_URL', 'RUST_LOG', 'AB_CONFIG_PATH', 'AB_WEB_URL', 'OPENAI_API_KEY', 'OPENAI_BASE_URL', 'AB_AI_MODE', 'AB_AI_MODEL', 'AB_AI_MAX_BYTES', 'AB_AI_TIMEOUT_SECONDS') + S3,
    'worker': ('DATABASE_URL', 'RUST_LOG', 'AB_BUILDER', 'AB_BUILDER_TAG', 'AB_BUILD_NETWORK', 'AB_BUILD_TIMEOUT_SECONDS', 'AB_BUILD_MEMORY_MIB', 'AB_BUILD_DISK_MIB', 'AB_BUILD_OUTPUT_MIB', 'AB_ARTIFACT_KEY', 'AB_REPO_MAX_BYTES') + S3,
    'repo': ('DATABASE_URL', 'RUST_LOG', 'AB_REPO_USER', 'AB_REPO_PASSWORD', 'AB_REPO_NAME', 'AB_REPO_GPG_KEY', 'AB_DOWNLOAD_TTL', 'AB_REPO_MAX_BYTES', 'AB_ARTIFACT_KEY') + S3,
    'notifier': ('RUST_LOG', 'AB_SMTP_HOST', 'AB_SMTP_USER', 'AB_SMTP_PASS', 'AB_SMTP_FROM', 'AB_SMTP_TO', 'AB_MAILLOGO'),
    'web': ('DATABASE_URL', 'AB_WEB_URL', 'AB_OIDC_ISSUER', 'AB_OIDC_CLIENT_ID', 'AB_OIDC_CLIENT_SECRET', 'AB_OIDC_ADMIN_GROUP', 'AB_OIDC_GROUPS_CLAIM', 'AB_OIDC_SCOPES', 'AB_SESSION_SECONDS', 'AB_TRUSTED_PROXIES') + S3,
    'migrate': ('DATABASE_URL', 'RUST_LOG'),
}

def password_hash(password):
    salt = secrets.token_bytes(4)
    return base64.b64encode(salt + hashlib.sha256(salt + password.encode()).digest()).decode()

def regex(names):
    import re
    return '^(' + '|'.join(re.escape(n) for n in names) + ')$' if names else '^$'

def provision(root=ROOT):
    root = Path(root)
    env = EnvFile(root / '.env')
    env.set('AB_ARTIFACT_KEY', env.get('AB_ARTIFACT_KEY') or secrets.token_urlsafe(48))
    env.set('AB_UPLOAD_TTL', str(min(900, int(env.get('AB_UPLOAD_TTL') or '900'))))
    admin = env.get('RABBITMQ_DEFAULT_USER')
    password = env.get('RABBITMQ_DEFAULT_PASS')
    if not admin or not password or admin in ('guest', *('ab-' + r for r in ROLES)):
        raise SetupError('Set a non-guest RabbitMQ administrator and password before provisioning')
    users = [{'name': admin, 'password_hash': password_hash(password), 'hashing_algorithm': 'rabbit_password_hashing_sha256', 'tags': 'administrator'}]
    # Explicitly revoke the legacy administrator's AMQP access, including existing volumes.
    permissions = [{'user': admin, 'vhost': '/', 'configure': '^$', 'write': '^$', 'read': '^$'}]
    for role, (writes, reads) in ROLES.items():
        key = 'AB_AMQP_' + role.upper() + '_PASSWORD'
        password = env.get(key) or secrets.token_urlsafe(32)
        env.set(key, password)
        user = 'ab-' + role
        users.append({'name': user, 'password_hash': password_hash(password), 'hashing_algorithm': 'rabbit_password_hashing_sha256', 'tags': ''})
        # No service may create/delete/bind topology or read a publishing exchange.
        permissions.append({'user': user, 'vhost': '/', 'configure': '^$', 'write': regex('ab.' + q for q in writes), 'read': regex(reads)})
    definitions = {'policies': [{'vhost': '/', 'name': 'ab-budgets', 'pattern': '^(pkg_build|build_results|expired_tasks|repo_ingest|repo_ingest_failed|notifications)$', 'apply-to': 'queues', 'priority': 10, 'definition': {'max-length': 1000, 'max-length-bytes': 268435456, 'overflow': 'reject-publish'}}], 'users': users, 'vhosts': [{'name': '/'}], 'permissions': permissions,
        'queues': [{'name': q, 'vhost': '/', 'durable': True, 'auto_delete': False, 'arguments': ({'x-dead-letter-exchange': '', 'x-dead-letter-routing-key': 'repo_ingest_failed'} if q == 'repo_ingest' else {})} for q in QUEUES],
        'exchanges': [{'name': 'ab.' + q, 'vhost': '/', 'type': 'direct', 'durable': True, 'auto_delete': False, 'internal': False, 'arguments': {}} for q in QUEUES],
        'bindings': [{'source': 'ab.' + q, 'vhost': '/', 'destination': q, 'destination_type': 'queue', 'routing_key': q, 'arguments': {}} for q in QUEUES]}
    directory = root / 'rabbitmq-security'
    directory.mkdir(mode=0o700, exist_ok=True)
    path = directory / 'definitions.json'
    # RabbitMQ's unprivileged user needs to read the bind mount. The parent on the
    # host remains private; the file contains only salted password hashes.
    path.write_text(json.dumps(definitions, indent=2) + '\n')
    path.chmod(0o644)
    for role, keys in KEYS.items():
        destination = EnvFile(root / ('.env.' + role))
        destination.lines = []
        for key in keys:
            value = env.get(key)
            if value:
                destination.set(key, value)
        if role in ROLES:
            destination.set('AMQP_ADDR', 'amqp://ab-' + role + ':' + quote(env.get('AB_AMQP_' + role.upper() + '_PASSWORD'), safe='') + '@rabbitmq:5672/%2f')
        destination.save()
    env.save()

if __name__ == '__main__':
    try:
        provision()
    except SetupError as error:
        raise SystemExit(str(error))
