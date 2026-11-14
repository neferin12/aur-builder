#!/usr/bin/env python3
"""Disposable Docker egress isolation test. Requires aur-builder-egress-test image."""
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

def run(*args, check=True):
    return subprocess.run(args, check=check, capture_output=True, text=True)

name = 'aur-network-test-' + uuid.uuid4().hex[:12]
proxy = name + '-proxy'
with tempfile.TemporaryDirectory(prefix='aur-network-') as directory:
    deny = Path(directory) / 'host-deny.acl'
    deny.write_text('127.0.0.1/32\n::1/128\n1.1.1.1/32\n')
    try:
        run('docker', 'network', 'create', '--internal', '-o', 'com.docker.network.bridge.gateway_mode_ipv4=isolated', name)
        run('docker', 'run', '-d', '--name', proxy, '--network', name, '--network-alias', 'build-egress', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true', '--read-only', '--tmpfs', '/tmp:size=16m,mode=1777', '-v', str(deny) + ':/etc/squid/host-deny.acl:ro', 'aur-builder-egress-test')
        run('docker', 'network', 'connect', 'bridge', proxy)
        for _ in range(20):
            status = run('docker', 'inspect', '--format', '{{.State.Status}}', proxy).stdout.strip()
            if status != 'running':
                raise RuntimeError(run('docker', 'logs', proxy).stdout)
            ready = run('docker', 'exec', proxy, 'sh', '-c', 'http_proxy=http://127.0.0.1:3128 wget -Y on -qO- -T 3 http://example.com', check=False)
            if ready.returncode == 0:
                break
            time.sleep(0.5)
        else:
            raise RuntimeError("Build proxy never became ready")
        def client(command):
            return run('docker', 'run', '--rm', '--network', name, '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true', '--entrypoint', 'sh', 'aur-builder-egress-test', '-c', command, check=False)
        result = client('http_proxy=http://build-egress:3128 wget -Y on -qO- -T 15 http://example.com')
        assert result.returncode == 0 and 'Example Domain' in result.stdout, result.stderr
        for destination in ('127.0.0.1', '169.254.169.254', '172.17.0.1', '10.0.0.1', '1.1.1.1'):
            result = client('http_proxy=http://build-egress:3128 wget -Y on -O- -T 5 http://' + destination)
            assert result.returncode != 0 and '403' in result.stderr, (destination, result.stderr)
        result = client('wget -Y off -O- -T 2 http://1.1.1.1')
        assert result.returncode != 0, 'direct public network bypass succeeded'
        result = client('wget -Y off -O- -T 2 http://172.17.0.1')
        assert result.returncode != 0, 'direct host network bypass succeeded'
        print('Public proxy downloads allowed; private, metadata, host, and direct routes denied')
    finally:
        run('docker', 'rm', '-f', proxy, check=False)
        run('docker', 'network', 'rm', name, check=False)
