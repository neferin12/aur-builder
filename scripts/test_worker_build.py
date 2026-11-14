#!/usr/bin/env python3
"""Full worker/source checkout/build/collection/upload test with disposable Docker resources.
Requires aur-builder-rootless-test and aur-builder-egress-test images.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid
ROOT = Path(__file__).resolve().parents[1]
def run(*args, **kw):
    return subprocess.run(args, check=True, **kw)
name = 'aur-full-build-' + uuid.uuid4().hex[:12]
image = 'localhost:1/aur-security-fixture:' + uuid.uuid4().hex[:12]
proxy = name + '-proxy'
hang_image = image + '-hang'
with tempfile.TemporaryDirectory(prefix='aur-full-build-') as directory:
    root = Path(directory)
    fixture = root / 'fixture'
    fixture.mkdir()
    (fixture / 'PKGBUILD').write_text('''pkgname=security-fixture
pkgver=1
pkgrel=1
arch=(any)
license=(MIT)
makedepends=(jq)
build() {
  test "$(id -u)" = 1000
  test -z "${AB_ARTIFACT_KEY+x}"
  test -z "${AB_UPLOAD_URL+x}"
  test -z "${AB_S3_SECRET_KEY+x}"
  test -z "${DATABASE_URL+x}"
  test -z "${AMQP_ADDR+x}"
  test -z "${LD_PRELOAD+x}"
  test ! -S /var/run/docker.sock
  grep -Eq '^Uid:[[:space:]]+1000[[:space:]]+1000' /proc/self/status
  grep -Eq '^CapEff:[[:space:]]+0000000000000000' /proc/self/status
  ! touch /proc/self/root/etc/untrusted-write
  printf '%s\\n' "$AB_PKG_SECRET"
  jq -n '{built:true}' > result.json
}
package() { install -Dm644 result.json "$pkgdir/usr/share/security-fixture/result.json"; }
''')
    for args in [('init',), ('config', 'user.name', 'Security fixture'), ('config', 'user.email', 'fixture@localhost'), ('config', 'commit.gpgsign', 'false'), ('add', 'PKGBUILD'), ('commit', '-m', 'Approved fixture')]:
        run('git', '-C', str(fixture), *args, stdout=subprocess.DEVNULL)
    revision = subprocess.check_output(['git', '-C', str(fixture), 'rev-parse', 'HEAD'], text=True).strip()
    (root / 'Dockerfile').write_text('FROM aur-builder-rootless-test\nUSER root\nCOPY fixture /fixture\nRUN tar -rf /opt/arch-rootfs.tar -C / fixture\nUSER 1000:1000\n')
    deny = root / 'host-deny.acl'
    deny.write_text('127.0.0.1/32\n::1/128\n')
    try:
        run('docker', 'build', '-t', image, str(root), stdout=subprocess.DEVNULL)
        run('docker', 'network', 'create', '--internal', '-o', 'com.docker.network.bridge.gateway_mode_ipv4=isolated', name, stdout=subprocess.DEVNULL)
        run('docker', 'run', '-d', '--name', proxy, '--network', name, '--network-alias', 'build-egress', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true', '--read-only', '--tmpfs', '/tmp:size=16m,mode=1777', '-v', str(deny) + ':/etc/squid/host-deny.acl:ro', 'aur-builder-egress-test', stdout=subprocess.DEVNULL)
        run('docker', 'network', 'connect', 'bridge', proxy)
        time.sleep(1)
        env = dict(os.environ, AB_BUILD_NETWORK=name, AB_BUILDER=image.rsplit(':', 1)[0], AB_BUILDER_TAG=image.rsplit(':', 1)[1], AB_TEST_REVISION=revision)
        env.pop('AB_CONFIG_PATH', None)
        run('cargo', 'test', '-p', 'worker', 'approved_source_to_authenticated_bundle', '--locked', '--', '--ignored', cwd=ROOT, env=env)
        (root / 'Dockerfile').write_text('FROM aur-builder-rootless-test\nUSER root\nRUN printf "#!/bin/sh\\nmkfifo /results/status\\nsleep 3600\\n" > /usr/local/bin/build-entrypoint && chmod 755 /usr/local/bin/build-entrypoint\nUSER 1000:1000\n')
        run('docker', 'build', '-t', hang_image, str(root), stdout=subprocess.DEVNULL)
        env.update(AB_BUILDER=hang_image.rsplit(':', 1)[0], AB_BUILDER_TAG=hang_image.rsplit(':', 1)[1], AB_BUILD_TIMEOUT_SECONDS='3')
        run('cargo', 'test', '-p', 'worker', 'blocked_completion_obeys_deadline_and_removes_container', '--locked', '--', '--ignored', cwd=ROOT, env=env)
    finally:
        ids = subprocess.check_output(['docker', 'ps', '-aq', '--filter', 'ancestor=' + image], text=True).split()
        if ids:
            subprocess.run(['docker', 'rm', '-f', *ids], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        ids = subprocess.check_output(['docker', 'ps', '-aq', '--filter', 'ancestor=' + hang_image], text=True).split()
        if ids:
            subprocess.run(['docker', 'rm', '-f', *ids], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(['docker', 'image', 'rm', hang_image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(['docker', 'rm', '-f', proxy], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(['docker', 'network', 'rm', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(['docker', 'image', 'rm', image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
