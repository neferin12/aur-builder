#!/usr/bin/env python3
"""Disposable real-AMQP authorization test. Requires Docker; leaves no host ports open."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid
from provision_broker import provision, EnvFile, ROOT, ROLES

def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)

with tempfile.TemporaryDirectory(prefix='aur-broker-test-') as directory:
    root = Path(directory)
    (root / '.env').write_text('RABBITMQ_DEFAULT_USER=test-admin\nRABBITMQ_DEFAULT_PASS=test-admin-password\n')
    provision(root)
    name = 'aur-broker-test-' + uuid.uuid4().hex[:12]
    try:
        run('docker', 'run', '-d', '--name', name, '--user', '100:101', '--tmpfs', '/var/lib/rabbitmq:uid=100,gid=101,mode=0700', '-p', '127.0.0.1::5672',
            '-e', 'RABBITMQ_SERVER_ADDITIONAL_ERL_ARGS=+S 2:2 +A 4',
            '-e', 'RABBITMQ_DEFAULT_USER=test-admin', '-e', 'RABBITMQ_DEFAULT_PASS=test-admin-password',
            '-v', str(ROOT / 'rabbitmq/rabbitmq.conf') + ':/etc/rabbitmq/rabbitmq.conf:ro',
            '-v', str(root / 'rabbitmq-security/definitions.json') + ':/etc/rabbitmq/definitions.json:ro',
            'rabbitmq:3-management-alpine', stdout=subprocess.DEVNULL)
        for _ in range(60):
            ready = subprocess.run(['docker', 'exec', name, 'rabbitmq-diagnostics', '-q', 'check_running'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if ready.returncode == 0:
                break
            state = subprocess.check_output(['docker', 'inspect', '--format', '{{.State.Status}}', name], text=True).strip()
            if state == 'exited':
                subprocess.run(['docker', 'logs', name])
                raise RuntimeError('RabbitMQ exited during startup')
            time.sleep(1)
        else:
            subprocess.run(['docker', 'logs', name])
            raise RuntimeError('RabbitMQ did not become ready')
        port = subprocess.check_output(['docker', 'port', name, '5672'], text=True).strip().rsplit(':', 1)[1]
        env = dict(os.environ)
        for role in ROLES:
            url = EnvFile(root / ('.env.' + role)).get('AMQP_ADDR')
            env['AB_TEST_AMQP_' + role.upper()] = url.replace('@rabbitmq:5672', '@127.0.0.1:' + port)
        run('cargo', 'test', '-p', 'common', 'messaging::broker_tests::service_permissions', '--', '--ignored', env=env, cwd=ROOT)
    finally:
        subprocess.run(['docker', 'rm', '-f', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
