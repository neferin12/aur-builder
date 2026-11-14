#!/usr/bin/env python3
"""Real Arch dependency/build/hook test under the production filesystem restrictions.
Requires docker build -t aur-builder-rootless-test build-container.
"""
from pathlib import Path
import subprocess
script = r'''
set -e
! touch /etc/host-root-write
! unshare -m true
mkdir -p /sandbox/root
tar --no-same-owner -xf /opt/arch-rootfs.tar -C /sandbox/root
proot -r /sandbox/root -b /dev -b /proc -b /etc/resolv.conf -w /build -i 1000:1000 /bin/bash -e <<'GUEST'
proot -0 pacman -S --noconfirm --needed jq
jq --version
mkdir dep
cd dep
cat > PKGBUILD <<'PKG'
pkgname=aur-security-test-dep
pkgver=1
pkgrel=1
arch=(any)
license=(MIT)
install=dependency.install
package() { mkdir -p "$pkgdir/usr/share/aur-security-test"; echo dependency > "$pkgdir/usr/share/aur-security-test/value"; }
PKG
cat > dependency.install <<'HOOK'
post_install() { grep '^Uid:' /proc/self/status > /tmp/install-hook-uid; }
HOOK
makepkg --noconfirm
proot -0 pacman -U --noconfirm --needed -- aur-security-test-dep-*.pkg.tar.zst
grep -Eq '^Uid:[[:space:]]+1000[[:space:]]+1000' /tmp/install-hook-uid
cd ..
mkdir target
cd target
cat > PKGBUILD <<'PKG'
pkgname=aur-security-test-target
pkgver=1
pkgrel=1
arch=(any)
license=(MIT)
depends=(aur-security-test-dep jq)
package() { test "$(cat /usr/share/aur-security-test/value)" = dependency; jq -n '{built:true}' > "$pkgdir/result.json"; }
PKG
makepkg --noconfirm
test -s aur-security-test-target-1-1-any.pkg.tar.zst
proot -0 /bin/bash -c 'grep -E "^(Uid|CapEff|NoNewPrivs|Seccomp):" /proc/self/status'
GUEST
# The emulated-root environment cannot change the real runtime filesystem.
! touch /etc/still-read-only
# Sized tmpfs gives an actual disk ceiling even without filesystem-specific quotas.
if dd if=/dev/zero of=/tmp/fill bs=1M count=65 2>/dev/null; then exit 1; fi
printf 'Rootless official and AUR-style dependency installation, hooks, makepkg and disk ceiling verified\n'
'''
subprocess.run(['docker', 'run', '--rm', '--user', '1000:1000', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true', '--security-opt', 'seccomp=' + str(Path(__file__).resolve().parents[1] / 'worker/src/build/seccomp.json'), '--read-only', '--memory', '8g', '--memory-swap', '8g', '--pids-limit', '512', '--tmpfs', '/sandbox:uid=1000,gid=1000,mode=0700,size=6g,exec', '--tmpfs', '/tmp:mode=1777,size=64m,exec', '--tmpfs', '/results:uid=1000,gid=1000,mode=0700,size=1g', '--entrypoint', '/bin/bash', 'aur-builder-rootless-test', '-c', script], check=True)
