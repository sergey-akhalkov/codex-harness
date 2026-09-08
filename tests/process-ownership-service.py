"""Owned Windows service bootstrap proof; never attaches the test runner to a Job."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import process_ownership as ownership


def service_fixture(root, mode):
    child = subprocess.Popen([sys.executable, '-B', '-c', 'import time; time.sleep(25)'],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    value = {'pid': os.getpid(), 'child': child.pid, 'sid': ownership._current_user_sid(),
             'cwd': str(Path.cwd()), 'environment': os.environ['HARNESS_SERVICE_FIXTURE']}
    (root / 'service.json').write_text(json.dumps(value), encoding='utf-8')
    print('Owned service Unicode log: проверка', flush=True)
    if mode == 'crash':
        raise RuntimeError('Owned startup crash before readiness')
    if mode == 'ready':
        ownership.mark_service_ready()
    until = time.monotonic() + 20
    while not (root / 'stop').exists() and time.monotonic() < until:
        time.sleep(0.05)


def starter(root, mode):
    service = ownership.spawn_service(root / 'entrypoint.py', arguments=['--service-fixture', str(root), mode],
        env={**os.environ, 'HARNESS_SERVICE_FIXTURE': 'проверка $() secret-sentinel'},
        cwd=root, log_path=root / 'service.log', startup_timeout=5)
    (root / 'starter.json').write_text(json.dumps({'pid': service.pid}), encoding='utf-8')
    time.sleep(20)


def main():
    import psutil
    sys.path.insert(0, str(ROOT))
    from tools.lsp.broker import private_directory
    root = Path(tempfile.mkdtemp(prefix='service ownership-'))
    private_directory(root)
    report = {'root': str(root), 'checks': [], 'passed': False}
    def check(value, label):
        if not value:
            raise AssertionError(label)
        report['checks'].append(label)
    def wait_until(condition, timeout=10):
        until = time.monotonic() + timeout
        while not condition() and time.monotonic() < until:
            time.sleep(0.05)
        return condition()
    try:
        for mode in ('ready', 'timeout', 'crash'):
            directory = root / mode
            directory.mkdir()
            (directory / 'sibling_marker.py').write_text("marker = 'owned sibling import'\n", encoding='utf-8')
            (directory / 'entrypoint.py').write_text(
                "from sibling_marker import marker\nassert marker == 'owned sibling import'\nimport runpy\n"
                + f"runpy.run_path({str(Path(__file__).resolve())!r}, run_name='__main__')\n", encoding='utf-8')
            captured = []
            try:
                with ownership.JobGuard(memory_limit_bytes=192 * 1024**2) as client_job:
                    with (directory / 'starter.log').open('wb') as log:
                        client = client_job.popen([sys.executable, '-B', str(Path(__file__).resolve()), '--starter', str(directory), mode],
                                    stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                    creationflags=subprocess.CREATE_NO_WINDOW)
                        check(wait_until(lambda: (directory / 'service.json').exists()), mode + ': service actually started')
                        check(True, mode + ': entrypoint sibling import resolves')
                        value = json.loads((directory / 'service.json').read_text())
                        for pid in (value['pid'], value['child']):
                            try:
                                captured.append(psutil.Process(pid))
                            except psutil.NoSuchProcess:
                                pass
                        check(value['sid'] == ownership._current_user_sid(), mode + ': requesting user SID retained')
                        check(value['environment'] == 'проверка $() secret-sentinel', mode + ': Unicode environment preserved as data')
                        check(Path(value['cwd']) == directory, mode + ': exact service working directory')
                        if mode == 'ready':
                            check(wait_until(lambda: (directory / 'starter.json').exists()), 'Starter retained service launch identity')
                            starter_info = json.loads((directory / 'starter.json').read_text())
                            captured.append(psutil.Process(starter_info['pid']))
                            check(all('secret-sentinel' not in ' '.join(process.cmdline()) for process in captured),
                                  'Environment value is absent from process command lines')
                        client_job.close()
                        client.wait(timeout=5)
                if mode == 'ready':
                    check(all(process.is_running() for process in captured), 'Client Job close preserves service and descendants')
                    (directory / 'stop').touch()
                check(wait_until(lambda: not any(process.is_running() for process in captured), 8),
                      mode + ': service exit reclaims all owned descendants')
                log = (directory / 'service.log').read_text(encoding='utf-8')
                check('проверка' in log, mode + ': native bootstrap log is valid UTF-8')
                if mode == 'timeout':
                    check('startup deadline elapsed' in log, 'Unpublished service has a finite startup lifetime')
            finally:
                (directory / 'stop').touch()
                for process in captured:
                    if process.is_running():
                        process.terminate()  # Exact retained identity from this owned fixture only.
                        process.wait(timeout=5)
        report['passed'] = True
    finally:
        (root / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Service ownership evidence: ' + str(root / 'report.json'))


if __name__ == '__main__':
    if '--service-fixture' in sys.argv:
        service_fixture(Path(sys.argv[2]), sys.argv[3])
    elif '--starter' in sys.argv:
        starter(Path(sys.argv[2]), sys.argv[3])
    else:
        main()
