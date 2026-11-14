#!/usr/bin/env python3
"""Check rendered Compose JSON, including overrides: docker compose config --format json."""
import json
import sys
config = json.load(sys.stdin)
for name in ('rabbitmq', 'garage', 'repo'):
    for port in config['services'][name].get('ports', []):
        if port.get('host_ip') not in ('127.0.0.1', '::1'):
            raise SystemExit(f'{name}: public port publishing is forbidden')
network = config.get('networks', {}).get('build', {})
if network.get('internal') is not True or network.get('driver_opts', {}).get('com.docker.network.bridge.gateway_mode_ipv4') != 'isolated':
    raise SystemExit('Build network must use an isolated internal bridge')
proxy = config['services'].get('build-egress', {})
if proxy.get('ports'):
    raise SystemExit('Build egress proxy must not publish host ports')
for name, service in config['services'].items():
    if name != 'build-egress' and 'build' in service.get('networks', {}):
        raise SystemExit(f'{name}: application services must not join the build network')
print('Compose service ports and build network isolation validated')
