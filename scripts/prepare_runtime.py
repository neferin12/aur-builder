#!/usr/bin/env python3
"""Stage service-owned runtime files without taking ownership of operator GPG keys."""
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from setup import run

def prepare(root, runner=run):
    root = Path(root).resolve()
    for directory in ('db', 'server_tmp', 'repo-signing-runtime'):
        (root / directory).mkdir(mode=0o700, exist_ok=True)
    if not (root / 'repo-gnupg').is_dir():
        raise RuntimeError('Provision repo-gnupg before preparing runtime permissions')
    script = '''set -o pipefail
if [ ! -f /runtime/.aur-builder-runtime ] && [ -n "$(ls -A /runtime)" ]; then
  echo 'Refusing to replace an unmarked signing runtime directory' >&2
  exit 1
fi
find /runtime -mindepth 1 -maxdepth 1 -exec rm -rf {} +
tar -C /source --exclude='S.gpg-agent*' -cf - . | tar -C /runtime -xf -
touch /runtime/.aur-builder-runtime
chown -R 10001:10001 /data /scratch /runtime
chmod 700 /runtime
'''
    runner(['docker', 'run', '--rm', '--user', '0:0',
            '--volume', str(root / 'db') + ':/data',
            '--volume', str(root / 'server_tmp') + ':/scratch',
            '--volume', str(root / 'repo-gnupg') + ':/source:ro',
            '--volume', str(root / 'repo-signing-runtime') + ':/runtime',
            'alpine', 'sh', '-euc', script], root)

if __name__ == '__main__':
    prepare(Path(__file__).resolve().parents[1])
