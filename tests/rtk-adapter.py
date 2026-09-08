"""Native harness-rtk checks against real child processes in an owned TEMP copy."""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ADAPTER_SRC = Path()
RTK_SRC = Path()
PYTEST_PYTHON = None
RAW_LIMIT = 4 * 1024 * 1024
LOCATOR = re.compile(br"\[rtk raw: (.+?)\]")


def run(argv, *, cwd=None, env=None, data=None, timeout=30):
    return subprocess.run(argv, cwd=cwd, env=env, input=data, capture_output=True, timeout=timeout)


def bash(command, **extra):
    tool_input = {"command": command, **extra}
    return {"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": tool_input}


def locator_path(stdout: bytes) -> Path:
    match = LOCATOR.search(stdout)
    if not match:
        raise AssertionError("missing rtk raw locator")
    return Path(match.group(1).decode("utf-8", "surrogateescape"))


class AdapterChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(tempfile.mkdtemp(prefix="harness-rtk-adapter-")).resolve()
        cls.home = cls.root / "codex"
        cls.workspace = cls.root / "workspace"
        cls.bin_dir = cls.root / "bin"
        cls.missing_dir = cls.root / "missing-rtk"
        cls.counter = cls.root / "once.bin"
        cls.home.mkdir()
        cls.workspace.mkdir()
        cls.bin_dir.mkdir()
        cls.missing_dir.mkdir()
        cls.adapter = cls.bin_dir / "harness-rtk.exe"
        cls.rtk = cls.bin_dir / "rtk.exe"
        cls.adapter_missing = cls.missing_dir / "harness-rtk.exe"
        shutil.copy2(ADAPTER_SRC, cls.adapter)
        shutil.copy2(RTK_SRC, cls.rtk)
        shutil.copy2(ADAPTER_SRC, cls.adapter_missing)
        cls.env = {**os.environ, "CODEX_HOME": str(cls.home)}
        cls.env.pop("HARNESS_RTK_DISABLE", None)
        subprocess.run(["git", "init", "-q", str(cls.workspace)], check=True)
        for number in range(80):
            subprocess.run(
                ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "--allow-empty", "-qm", f"receipt-{number:04d}"],
                cwd=cls.workspace, check=True,
            )

    def invoke(self, args, *, cwd=None, env=None, data=None, timeout=30, adapter=None):
        merged = {**self.env, **(env or {})}
        return run([str(adapter or self.adapter), *args], cwd=cwd or self.workspace, env=merged, data=data, timeout=timeout)

    def hook(self, payload, **kwargs):
        data = payload if isinstance(payload, (bytes, bytearray)) else json.dumps(payload).encode("utf-8")
        return self.invoke(["hook"], data=data, **kwargs)

    def test_exec_preserves_child_identity_and_runs_once(self):
        receipt = self.root / "child-identity.json"
        script = (
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "counter = Path(sys.argv[1])\n"
            "counter.write_bytes(counter.read_bytes() + b'x') if counter.exists() else counter.write_bytes(b'x')\n"
            "Path(sys.argv[2]).write_text(json.dumps({\n"
            "    'arg': sys.argv[3], 'cwd': os.getcwd(),\n"
            "    'mark': os.environ.get('HARNESS_RTK_MARK', ''), 'argv': sys.argv[1:],\n"
            "}, ensure_ascii=False), encoding='utf-8')\n"
            "sys.stdout.buffer.write(b'OUT\\n')\n"
            "sys.stderr.write('ERR\\n')\n"
        )
        completed = self.invoke(
            ["exec", sys.executable, "-c", script, str(self.counter), str(receipt), "проверка-файл"],
            cwd=self.workspace,
            env={"HARNESS_RTK_MARK": "yes"},
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        observed = json.loads(receipt.read_text(encoding="utf-8"))
        self.assertEqual(observed["arg"], "проверка-файл")
        self.assertEqual(Path(observed["cwd"]).resolve(), self.workspace)
        self.assertEqual(observed["mark"], "yes")
        self.assertEqual(observed["argv"][2], "проверка-файл")
        self.assertEqual(completed.stdout.replace(b"\r\n", b"\n"), b"OUT\n")
        self.assertEqual(completed.stderr.replace(b"\r\n", b"\n"), b"ERR\n")
        self.assertEqual(self.counter.read_bytes(), b"x")
        self.assertNotIn(b"[rtk raw:", completed.stdout)

    def test_exec_nonzero_status_and_stdin(self):
        failing = self.invoke(["exec", sys.executable, "-c", "import sys; sys.stdout.write(\"out\\n\"); sys.stderr.write(\"err\\n\"); sys.exit(7)"])
        self.assertEqual(failing.returncode, 7)
        self.assertEqual(failing.stdout.replace(b"\r\n", b"\n"), b"out\n")
        self.assertEqual(failing.stderr.replace(b"\r\n", b"\n"), b"err\n")
        echoed = self.invoke(["exec", sys.executable, "-c", "import sys; sys.stdout.buffer.write(b\"GOT:\"+sys.stdin.buffer.read())"], data=b"payload-xyz")
        self.assertEqual(echoed.returncode, 0)
        self.assertEqual(echoed.stdout, b"GOT:payload-xyz")

    def test_native_argument_roundtrip_and_filtered_failure(self):
        expected = ['', 'space here', 'a"b', "'single'", 'tail\\', 'проверка']
        script = 'import json,sys; print(json.dumps(sys.argv[1:]))'
        result = self.invoke(['compact', sys.executable, '-c', script, *expected])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), expected)
        raw = self.invoke(['exec', 'git', 'log'], cwd=self.root)
        compact = self.invoke(['compact', 'git', 'log'], cwd=self.root)
        self.assertEqual(raw.returncode, 128)
        self.assertEqual(compact.returncode, raw.returncode)
        self.assertEqual((compact.stdout, compact.stderr), (raw.stdout, raw.stderr))

    def test_hung_filter_and_capture_failure(self):
        rustc = shutil.which('rustc')
        if not rustc:
            self.skipTest('Rust compiler needed for the owned timeout executable')
        folder = self.root / 'hung-filter'
        folder.mkdir()
        shutil.copy2(ADAPTER_SRC, folder / 'harness-rtk.exe')
        source = folder / 'hung.rs'
        source.write_text('fn main(){std::thread::sleep(std::time::Duration::from_secs(8));}', encoding='utf-8')
        build = run([rustc, str(source), '-o', str(folder / 'rtk.exe')])
        self.assertEqual(build.returncode, 0, build.stderr)
        data = b'raw-evidence\n' * 100
        started = time.monotonic()
        result = self.invoke(['filter', 'git-log'], adapter=folder / 'harness-rtk.exe', data=data)
        self.assertLess(time.monotonic() - started, 5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, data)
        self.assertIn(b'filter timed out', result.stderr)
        blocked = self.root / 'blocked-capture'
        (blocked / 'harness' / 'rtk').mkdir(parents=True)
        (blocked / 'harness' / 'rtk' / 'raw').write_bytes(b'preserve foreign file')
        result = self.invoke(['filter', 'git-log'], data=data, env={'CODEX_HOME': str(blocked)})
        self.assertEqual(result.stdout, data)
        self.assertIn(b'raw capture unavailable', result.stderr)
        self.assertEqual((blocked / 'harness' / 'rtk' / 'raw').read_bytes(), b'preserve foreign file')

    def test_cargo_failure_retains_evidence(self):
        cargo = shutil.which('cargo')
        if not cargo:
            self.skipTest('Cargo required for real test-output acceptance')
        folder = self.root / 'cargo-fixture'
        (folder / 'src').mkdir(parents=True)
        (folder / 'Cargo.toml').write_text('[package]\nname="rtk-fixture"\nversion="0.1.0"\nedition="2021"\n', encoding='utf-8')
        tests = ['#[test] fn failure_marker(){assert_eq!(1,2,"owned-failure-detail");}']
        tests += [f'#[test] fn pass_{n:03d}(){{assert_eq!(1,1);}}' for n in range(59)]
        (folder / 'src' / 'lib.rs').write_text('\n'.join(tests), encoding='utf-8')
        result = self.invoke(['compact', cargo, 'test'], cwd=folder, env={'CARGO_TARGET_DIR':str(folder / 'target')}, timeout=60)
        self.assertEqual(result.returncode, 101, result.stderr)
        self.assertIn(b'failure_marker', result.stdout)
        retained = locator_path(result.stdout).read_bytes() if LOCATOR.search(result.stdout) else result.stdout
        self.assertIn(b'owned-failure-detail', retained)
        self.assertIn(b'59 passed; 1 failed', retained)

    def test_pytest_failure_retains_evidence(self):
        if not PYTEST_PYTHON:
            self.skipTest('Pass --pytest-python with an existing pytest environment')
        folder = self.root / 'pytest-fixture'
        folder.mkdir()
        fixture = 'import pytest\n@pytest.mark.parametrize("n", range(40))\ndef test_owned(n):\n    assert n != 39, "owned-pytest-failure"\n'
        (folder / 'test_owned.py').write_text(fixture, encoding='utf-8')
        result = self.invoke(['compact', str(PYTEST_PYTHON), '-m', 'pytest', '-v'], cwd=folder,
                             env={'PYTEST_DISABLE_PLUGIN_AUTOLOAD':'1'}, timeout=60)
        self.assertEqual(result.returncode, 1, result.stderr)
        retained = locator_path(result.stdout).read_bytes() if LOCATOR.search(result.stdout) else result.stdout
        self.assertIn(b'owned-pytest-failure', retained)
        self.assertIn(b'1 failed, 39 passed', retained)
        self.assertIn(b'1 failed', result.stdout.lower())
        self.assertIn(b'owned-pytest-failure', result.stdout)

    def test_exec_is_always_raw(self):
        completed = self.invoke(["exec", "git", "log", "-n", "80"])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertNotIn(b"[rtk raw:", completed.stdout)
        self.assertEqual(completed.stdout.replace(b"\r\n", b"\n").count(b"commit "), 80)
        self.assertIn(b"receipt-0079", completed.stdout)
        self.assertIn(b"receipt-0000", completed.stdout)

    def test_compact_python_passthrough(self):
        script = "print('PY' * 400)"
        raw = self.invoke(["exec", sys.executable, "-c", script])
        compact = self.invoke(["compact", sys.executable, "-c", script])
        self.assertEqual(raw.returncode, compact.returncode, compact.stderr)
        self.assertEqual(compact.stdout, raw.stdout)
        self.assertEqual(compact.stderr, raw.stderr)
        self.assertNotIn(b"[rtk raw:", compact.stdout)
        self.assertGreater(len(compact.stdout), 500)

    def test_compact_git_log_keeps_latest_and_raw_history(self):
        raw = self.invoke(["exec", "git", "log", "-n", "80"])
        compact = self.invoke(["compact", "git", "log", "-n", "80"])
        self.assertEqual(raw.returncode, compact.returncode, compact.stderr)
        self.assertEqual(compact.returncode, 0)
        self.assertEqual(compact.stderr, raw.stderr)
        self.assertIn(b"receipt-0079", compact.stdout)
        self.assertLess(len(compact.stdout), len(raw.stdout))
        retained = locator_path(compact.stdout).read_bytes()
        self.assertEqual(retained.replace(b"\r\n", b"\n"), raw.stdout.replace(b"\r\n", b"\n"))
        self.assertEqual(retained.count(b"commit "), 80)
        self.assertIn(b"receipt-0000", retained)

    def test_compact_git_c_and_disable_and_unsupported_format(self):
        other = self.root / "other-cwd"
        other.mkdir(exist_ok=True)
        via_c = self.invoke(["compact", "git", "-C", str(self.workspace), "log", "-n", "80"], cwd=other)
        self.assertEqual(via_c.returncode, 0, via_c.stderr)
        self.assertIn(b"receipt-0079", via_c.stdout)
        self.assertTrue(locator_path(via_c.stdout).is_file())
        raw = self.invoke(["exec", "git", "log", "-n", "80"])
        disabled = self.invoke(["compact", "git", "log", "-n", "80"], env={"HARNESS_RTK_DISABLE": "1"})
        self.assertEqual(disabled.stdout, raw.stdout)
        self.assertNotIn(b"[rtk raw:", disabled.stdout)
        pretty = ["git", "log", "-n", "80", "--pretty=format:%H"]
        raw_fmt = self.invoke(["exec", *pretty])
        compact_fmt = self.invoke(["compact", *pretty])
        self.assertEqual(compact_fmt.stdout, raw_fmt.stdout)
        self.assertNotIn(b"[rtk raw:", compact_fmt.stdout)
        self.assertEqual(raw_fmt.stdout.count(b"\n") + (0 if raw_fmt.stdout.endswith(b"\n") else 1), 80)

    def test_compact_status_is_byte_identical(self):
        for number in range(120):
            (self.workspace / f"owned-file-{number:04d}.txt").write_text(f"fixture {number}\n", encoding="utf-8")
        raw = self.invoke(["exec", "git", "status", "--short"])
        compact = self.invoke(["compact", "git", "status", "--short"])
        self.assertEqual(raw.returncode, compact.returncode, compact.stderr)
        self.assertEqual(compact.stdout, raw.stdout)
        self.assertEqual(compact.stderr, raw.stderr)
        self.assertEqual(raw.stdout.count(b"owned-file-"), 120)

    def test_hook_rewrites_only_literal_exec_prefix(self):
        quoted = "harness-rtk.exe exec git log -n 80 --pretty=format:%s"
        completed = self.hook(bash(quoted, login=False, cwd=str(self.workspace)))
        self.assertEqual(completed.returncode, 0, completed.stderr)
        response = json.loads(completed.stdout)
        specific = response["hookSpecificOutput"]
        self.assertEqual(specific["permissionDecision"], "allow")
        self.assertEqual(specific["hookEventName"], "PreToolUse")
        updated = specific["updatedInput"]
        self.assertEqual(updated["command"], "harness-rtk.exe compact git log -n 80 --pretty=format:%s")
        self.assertEqual(updated["login"], False)
        self.assertEqual(updated["cwd"], str(self.workspace))
        spaces = "harness-rtk.exe exec git -C \"owned repo\" log -n 80"
        rewritten = self.hook(bash(spaces))
        command = json.loads(rewritten.stdout)["hookSpecificOutput"]["updatedInput"]["command"]
        self.assertEqual(command, "harness-rtk.exe compact git -C \"owned repo\" log -n 80")

    def test_hook_ignores_malformed_other_tools_and_shell_control(self):
        silent = [
            b"not-json",
            b"{}",
            json.dumps(bash("git log -n 80")).encode(),
            json.dumps({"hook_event_name": "PreToolUse", "tool_name": "Write", "tool_input": {"command": "harness-rtk.exe exec git log -n 80"}}).encode(),
            json.dumps({"hook_event_name": "Stop", "tool_name": "Bash", "tool_input": {"command": "harness-rtk.exe exec git log -n 80"}}).encode(),
            json.dumps(bash("harness-rtk.exe compact git log -n 80")).encode(),
            json.dumps(bash("harness-rtk.exe exec git log -n 80 | cat")).encode(),
            json.dumps(bash("harness-rtk.exe exec echo $HOME")).encode(),
            json.dumps(bash("harness-rtk.exe exec echo " + chr(96) + "date" + chr(96))).encode(),
            json.dumps(bash("harness-rtk.exe exec echo a; echo b")).encode(),
            json.dumps(bash("harness-rtk.exe exec echo a & echo b")).encode(),
            json.dumps(bash("harness-rtk.exe exec echo <file")).encode(),
            json.dumps(bash("harness-rtk.exe exec echo >file")).encode(),
            json.dumps(bash("harness-rtk.exe exec echo #comment")).encode(),
            json.dumps(bash("harness-rtk.exe exec ")).encode(),
        ]
        for payload in silent:
            with self.subTest(payload=payload[:48]):
                completed = self.hook(payload)
                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertEqual(completed.stdout, b"")
        disabled = self.hook(bash("harness-rtk.exe exec git log -n 80"), env={"HARNESS_RTK_DISABLE": "1"})
        self.assertEqual(disabled.returncode, 0)
        self.assertEqual(disabled.stdout, b"")

    def test_missing_rtk_falls_back_raw(self):
        hooked = self.hook(bash("harness-rtk.exe exec git log -n 80"), adapter=self.adapter_missing)
        self.assertEqual(hooked.returncode, 0)
        self.assertEqual(hooked.stdout, b"")
        self.assertIn(b"rtk: dependency unavailable; original command unchanged", hooked.stderr.replace(b"\r\n", b"\n"))
        raw = self.invoke(["exec", "git", "log", "-n", "80"])
        compact = self.invoke(["compact", "git", "log", "-n", "80"], adapter=self.adapter_missing)
        self.assertEqual(compact.returncode, raw.returncode, compact.stderr)
        self.assertEqual(compact.stdout, raw.stdout)
        self.assertNotIn(b"[rtk raw:", compact.stdout)
        self.assertTrue(compact.stderr.startswith(b"rtk:"))
        self.assertIn(b"raw passthrough", compact.stderr)

    def test_filter_bounds_and_failure_stay_raw(self):
        small = self.invoke(["filter", "git-log"], data=b"tiny")
        self.assertEqual(small.returncode, 0, small.stderr)
        self.assertEqual(small.stdout, b"tiny")
        self.assertEqual(small.stderr, b"")
        binary = b"A" * 600 + bytes([0]) + b"B" * 10
        binary_out = self.invoke(["filter", "git-log"], data=binary)
        self.assertEqual(binary_out.stdout, binary)
        self.assertEqual(binary_out.stderr, b"")
        self.assertNotIn(b"[rtk raw:", binary_out.stdout)
        failed = self.invoke(["filter", "not-a-filter"], data=b"x" * 600)
        self.assertEqual(failed.returncode, 0, failed.stderr)
        self.assertEqual(failed.stdout, b"x" * 600)
        self.assertIn(b"rtk:", failed.stderr)
        self.assertIn(b"raw passthrough", failed.stderr)
        over = (RAW_LIMIT + 10) * b"x"
        oversized = self.invoke(["filter", "git-log"], data=over, timeout=60)
        self.assertEqual(oversized.returncode, 0, oversized.stderr)
        self.assertEqual(oversized.stdout, over)
        self.assertNotIn(b"[rtk raw:", oversized.stdout)
        self.assertIn(b"stdout exceeds 4 MiB; raw passthrough without retention", oversized.stderr)

    def test_optional_rg_passthrough_or_compact(self):
        rg = shutil.which("rg")
        if not rg:
            self.skipTest("rg is not on PATH")
        version = run([rg, "--version"])
        if version.returncode or b"ripgrep" not in version.stdout.lower() + version.stderr.lower():
            self.skipTest("PATH rg is not ripgrep")
        source = self.workspace / "rg-owned.txt"
        source.write_text("\n".join(f"fixture-line-{i:03d}" for i in range(200)) + "\n", encoding="utf-8")
        raw = self.invoke(["exec", rg, "-n", "fixture-line", str(source)])
        compact = self.invoke(["compact", rg, "-n", "fixture-line", str(source)])
        self.assertEqual(raw.returncode, compact.returncode, compact.stderr)
        self.assertEqual(raw.stdout.count(b"fixture-line-"), 200)
        if compact.stdout == raw.stdout:
            self.assertNotIn(b"[rtk raw:", compact.stdout)
        else:
            retained = locator_path(compact.stdout).read_bytes()
            self.assertEqual(retained.replace(b"\r\n", b"\n"), raw.stdout.replace(b"\r\n", b"\n"))
            self.assertIn(b"fixture-line-000", compact.stdout)
            self.assertLess(len(compact.stdout), len(raw.stdout))


def verify_rtk(path: Path) -> Path:
    resolved = path.resolve(strict=True)
    version = run([str(resolved), "--version"])
    text = (version.stdout + version.stderr).decode("utf-8", "replace")
    if version.returncode or "0.48" not in text:
        raise SystemExit(f"RTK 0.48 binary required: {resolved}: {text.strip()}")
    return resolved


def main(argv: list[str] | None = None) -> None:
    global ADAPTER_SRC, RTK_SRC, PYTEST_PYTHON
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter", required=True, type=Path)
    parser.add_argument("--rtk", type=Path)
    parser.add_argument("--pytest-python", type=Path, help='Existing pytest environment; this test never installs packages')
    args, rest = parser.parse_known_args(argv)
    ADAPTER_SRC = args.adapter.resolve(strict=True)
    rtk = args.rtk if args.rtk is not None else ADAPTER_SRC.parent / "rtk.exe"
    RTK_SRC = verify_rtk(rtk)
    PYTEST_PYTHON = args.pytest_python.resolve(strict=True) if args.pytest_python else None
    unittest.main(argv=[sys.argv[0], *rest], verbosity=2)


if __name__ == "__main__":
    main()
