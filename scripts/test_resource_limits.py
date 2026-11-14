#!/usr/bin/env python3
"""Verify kernel PID and OOM enforcement using disposable, tightly bounded containers."""
import json
from pathlib import Path
import subprocess
import uuid
ROOT = Path(__file__).resolve().parents[1]

def verify(program, expected, oom=False):
    name = 'aur-resource-test-' + uuid.uuid4().hex[:12]
    try:
        subprocess.run(['docker', 'create', '--name', name, '--user', '1000:1000', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true', '--security-opt', 'seccomp=' + str(ROOT / 'worker/src/build/seccomp.json'), '--read-only', '--memory', '64m', '--memory-swap', '64m', '--pids-limit', '32', '--cpus', '1', '--entrypoint', '/usr/bin/python', 'aur-builder-rootless-test', '-c', program], check=True, stdout=subprocess.DEVNULL)
        subprocess.run(['docker', 'start', '-a', name], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
        state = json.loads(subprocess.check_output(['docker', 'inspect', name], text=True))[0]
        assert state['State']['ExitCode'] == expected, state['State']
        assert state['State']['OOMKilled'] == oom, state['State']
        assert state['HostConfig']['PidsLimit'] == 32
        assert state['HostConfig']['Memory'] == state['HostConfig']['MemorySwap'] == 64 * 1024 * 1024
        assert state['HostConfig']['ReadonlyRootfs']
    finally:
        subprocess.run(['docker', 'rm', '-f', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

verify('''import os, errno, signal
children=[]
try:
    for _ in range(100):
        pid=os.fork()
        if pid == 0:
            signal.pause()
            os._exit(0)
        children.append(pid)
    raise AssertionError('PID ceiling was bypassed')
except OSError as error:
    assert error.errno == errno.EAGAIN
    assert 0 < len(children) < 32
finally:
    for pid in children: os.kill(pid, signal.SIGKILL)
    for pid in children: os.waitpid(pid, 0)
''', 0)
verify('allocation=bytearray(128 * 1024 * 1024); print(len(allocation))', 137, oom=True)
print('PID exhaustion contained and memory exhaustion killed only the bounded container')
