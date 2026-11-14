#!/usr/bin/env python3
"""Record this Docker host's interface addresses so build egress cannot hairpin to it.
Run on the Docker host before deploying; repeat after changing host addresses.
"""
import ipaddress
import json
from pathlib import Path
import subprocess
import socket
import os
import stat
from urllib.parse import urlsplit
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from setup import EnvFile

def provision(root):
    interfaces = json.loads(subprocess.check_output(['ip', '-json', 'address', 'show'], text=True))
    addresses = {'127.0.0.1/32', '::1/128'}
    for interface in interfaces:
        for info in interface.get('addr_info', []):
            address = ipaddress.ip_address(info['local'])
            addresses.add(str(address) + ('/32' if address.version == 4 else '/128'))
    env = EnvFile(Path(root) / '.env')
    configured_socket = env.get('DOCKER_SOCKET_PATH')
    if not configured_socket and os.environ.get('DOCKER_HOST', '').startswith('unix://'):
        configured_socket = os.environ['DOCKER_HOST'][7:]
    socket_path = Path(configured_socket or '/var/run/docker.sock').resolve(strict=True)
    metadata = socket_path.stat()
    if not stat.S_ISSOCK(metadata.st_mode):
        raise RuntimeError('DOCKER_SOCKET_PATH must identify the local Docker Unix socket')
    env.set('DOCKER_SOCKET_PATH', str(socket_path))
    # Stat inside Docker's user namespace: rootless engines remap host group IDs.
    namespace_gid = subprocess.check_output(['docker', 'run', '--rm', '--network', 'none', '--user', '0:0', '--volume', str(socket_path) + ':/docker.sock:ro', 'alpine', 'stat', '-c', '%g', '/docker.sock'], text=True).strip().splitlines()[-1]
    if not namespace_gid.isdigit():
        raise RuntimeError('Cannot discover Docker socket group mapping')
    env.set('DOCKER_SOCKET_GID', namespace_gid)
    env.save()
    # Also deny the public reverse-proxy addresses: NAT may make the Docker host
    # reachable at addresses which are absent from its interface inventory.
    for key in ('AB_WEB_URL', 'AB_S3_PUBLIC_ENDPOINT'):
        value = env.get(key)
        if value:
            host = urlsplit(value).hostname
            if not host:
                raise ValueError(f'{key} has no hostname')
            for entry in socket.getaddrinfo(host, None, type=socket.SOCK_STREAM):
                address = ipaddress.ip_address(entry[4][0])
                addresses.add(str(address) + ('/32' if address.version == 4 else '/128'))
    # Squid reads this as destination IP ACL data. Never include source secrets.
    path = Path(root) / 'build-egress' / 'host-deny.acl'
    path.parent.mkdir(exist_ok=True)
    path.write_text('\n'.join(sorted(addresses)) + '\n')

if __name__ == '__main__':
    provision(Path(__file__).resolve().parents[1])
