import subprocess, os, glob, sys

REPO = r'<USER-HOME>\WorkBuddy\Worktrees\remote-mic-app-windows\origin-main-01fa85f8'
PG = r'<USER-HOME>\.workbuddy\binaries\PortableGit\versions\1.2.0\mingw64\bin'
OUT = r'<PROBE-DIR>\verify-build.out'

env = os.environ.copy()
env['PATH'] = PG + os.pathsep + r'<USER-HOME>\.cargo\bin' + os.pathsep + env.get('PATH', '')
env['GIT_EXEC_PATH'] = PG

cands = [r'<USER-HOME>\.cargo\bin\cargo.exe']
cands += glob.glob(r'<USER-HOME>\.rustup\toolchains\*\bin\cargo.exe')
cands = [c for c in cands if os.path.exists(c)]
CARGO = cands[0] if cands else 'cargo'


def run(args, timeout=None):
    r = subprocess.run(args, cwd=REPO, capture_output=True, env=env, timeout=timeout)
    o = r.stdout
    for enc in ('utf-8', 'gbk', 'cp936', 'latin-1'):
        try:
            o = o.decode(enc)
            break
        except Exception:
            pass
    e = r.stderr.decode('utf-8', 'replace')
    return r.returncode, o, e


lines = []
lines.append('cargo = ' + CARGO)
lines.append('exists = %s' % os.path.exists(CARGO))

rc, o, e = run([CARGO, 'fmt', '-p', 'sayall-windows', '--', '--check'])
lines.append('=== cargo fmt --check : exit=%s ===' % rc)
lines.append(o[-4000:])
lines.append(e[-2000:])

rc2, o2, e2 = run([CARGO, 'check', '-p', 'sayall-windows'])
lines.append('=== cargo check -p sayall-windows : exit=%s ===' % rc2)
lines.append(o2[-8000:])
lines.append(e2[-5000:])

open(OUT, 'w', encoding='utf-8').write('\n'.join(lines))
print('done, wrote', OUT)
