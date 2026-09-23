import ctypes
import hashlib
import json
import os
from pathlib import Path
import plistlib
import sqlite3
import stat
import subprocess
import sys
import time

assert sys.platform == 'darwin', 'This verifier requires macOS'
def env_path(name):
    value = Path(os.environ[name])
    assert value.is_absolute(), name + ' must be absolute'
    return value.resolve()

evidence = env_path('MUNIMENT_UPDATE_PROOF_EVIDENCE')
proof = json.loads(evidence.read_text())
installed = env_path('MUNIMENT_UPDATE_PROOF_INSTALLED_FILE')
expected_app = env_path('MUNIMENT_UPDATE_PROOF_EXPECTED_APP')
app = installed.parents[2]
app_log = env_path('MUNIMENT_E2E_DRIVER_APP_LOG')
window_probe = env_path('MUNIMENT_UPDATE_PROOF_WINDOW_PROBE')
endpoint = env_path('MUNIMENT_UPDATE_PROOF_ENDPOINT')
database = env_path('MUNIMENT_UPDATE_PROOF_DATABASE')
repo = env_path('MUNIMENT_UPDATE_PROOF_REPO')
assert proof['installedBytesMatch'] and proof['sentinelPreserved']

libproc = ctypes.CDLL('/usr/lib/libproc.dylib')
libproc.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
libproc.proc_pidpath.restype = ctypes.c_int
def exact_pids():
    for item in subprocess.check_output(['ps', '-axo', 'pid='], text=True).split():
        pid = int(item)
        buf = ctypes.create_string_buffer(4096)
        if libproc.proc_pidpath(pid, buf, len(buf)) > 0:
            if Path(os.fsdecode(buf.value)) == installed and pid != proof['beforePid']:
                yield pid

deadline = time.monotonic() + 120
pid = None
while time.monotonic() < deadline:
    candidates = list(exact_pids())
    if len(candidates) == 1:
        result = subprocess.run([str(window_probe), str(candidates[0])], capture_output=True, text=True)
        if result.returncode == 0 and int(result.stdout.strip()) > 0:
            pid = candidates[0]
            break
    time.sleep(0.5)
if not pid:
    import shutil
    diagnostics = evidence.parent / 'raw' / 'restart-diagnostics'
    diagnostics.mkdir(exist_ok=True)
    for root in [Path.home() / 'Library/Logs/DiagnosticReports', Path('/Library/Logs/DiagnosticReports')]:
        for report in root.glob('*muniment*'):
            if report.is_file(): shutil.copy2(report, diagnostics / report.name)
    for log in (Path.home() / '.muniment').rglob('*.log'):
        if log.stat().st_size < 2_000_000:
            shutil.copy2(log, diagnostics / (log.name + '-' + str(len(list(diagnostics.iterdir())))))
    with (diagnostics / 'manual-launch.log').open('wb') as output:
        child = subprocess.Popen([str(installed), '--probe-runtime-notice'], stdout=output, stderr=subprocess.STDOUT)
        time.sleep(20)
        (diagnostics / 'manual-launch-status.json').write_text(json.dumps({'pid':child.pid,'exitCode':child.poll()}))
        if child.poll() is None:
            child.terminate()
            try: child.wait(timeout=10)
            except subprocess.TimeoutExpired: child.kill()
assert pid, 'The updater did not restart the exact installed app with a visible window' 
assert proof['beforePid'] not in [int(p) for p in subprocess.check_output(['ps', '-axo', 'pid='], text=True).split()], 'The old process remains alive'

fresh_log = evidence.with_name('restart-app.log')
deadline = time.monotonic() + 60
while True:
    with app_log.open('rb') as source:
        source.seek(proof['restartLogOffset'])
        fresh = source.read()
    states = [line for line in fresh.splitlines() if line.startswith(b'desktop runtime client connected=')]
    if states and states[-1] == b'desktop runtime client connected=true':
        fresh_log.write_bytes(fresh)
        break
    assert time.monotonic() < deadline, 'No fresh connected-runtime notice followed the restart'
    time.sleep(0.5)
os.chmod(fresh_log, 0o600)
assert stat.S_ISSOCK(endpoint.stat().st_mode), 'The restarted runtime socket is absent'
subprocess.run(['bash', '-c', 'source "$1"; probe_macos_runtime "$2" "$3" "$4" "$5" "$6" "$7"',
    'verify-runtime', str(repo / 'test/e2e/support/macos-runtime-probe.sh'),
    f'gui/{os.getuid()}/ai.muniment.runtime', str(pid), str(fresh_log), str(endpoint),
    str(evidence.with_name('restart-runtime.log')), str(app / 'Contents/Library/LaunchServices/muniment-runtime')], check=True)

def tree(root):
    result = {}
    for file in sorted(root.rglob('*')):
        key = str(file.relative_to(root))
        mode = file.lstat().st_mode
        if file.is_symlink():
            result[key] = ['link', os.readlink(file)]
        elif file.is_file():
            digest = hashlib.sha256()
            with file.open('rb') as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(chunk)
            result[key] = ['file', mode & 0o111, digest.hexdigest()]
        elif file.is_dir():
            result[key] = ['directory']
        else:
            raise AssertionError('Unexpected bundle entry: ' + key)
    return result
expected_tree = tree(expected_app)
assert tree(app) == expected_tree, 'Installed bundle differs from the independently extracted signed release'
bundle_digest = hashlib.sha256(json.dumps(expected_tree, sort_keys=True).encode()).hexdigest()
subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
subprocess.run(['spctl', '--assess', '--type', 'execute', str(app)], check=True)
with (app / 'Contents/Info.plist').open('rb') as stream:
    assert plistlib.load(stream)['CFBundleShortVersionString'] == proof['offeredVersion']
with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as connection:
    row = connection.execute("SELECT envelope_json FROM thread_events WHERE thread_id=? AND event_type='thread.title.renamed' ORDER BY thread_seq DESC LIMIT 1", (proof['threadId'],)).fetchone()
assert row and json.loads(row[0])['payload_json']['title'] == proof['title'], 'The saved thread did not survive the update'
proof.update(restartVerified=True, runtimeConnectionVerified=True, storedThreadVerifiedAfterRestart=True,
    installedBundleMatches=True, bundleManifestSha256=bundle_digest, signatureVerified=True, gatekeeperAccepted=True, afterPid=pid)
evidence.write_text(json.dumps(proof, indent=2) + '\n')
os.chmod(evidence, 0o600)
