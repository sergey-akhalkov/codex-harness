"""Deterministic owned-process checks; no models or live service control."""
from __future__ import annotations
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from typing import ClassVar, Protocol, TypeGuard, override, runtime_checkable
from collections.abc import Mapping

REPO = Path(__file__).resolve().parents[1]
@runtime_checkable
class ProcessModule(Protocol):
    def run_case(self, argv: list[str], cwd: Path, timeout: int = 10,
                 ready_timeout: int | None = None, output_limit: int = 16 * 1024 * 1024) -> object: ...


def load_process_case() -> ProcessModule:
    spec = importlib.util.spec_from_file_location("process_case", REPO / ".agents/skills/reproduce-regression/scripts/process_case.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert isinstance(module, ProcessModule)
    return module


process_case = load_process_case()


def is_mapping(value: object) -> TypeGuard[Mapping[object, object]]:
    return isinstance(value, Mapping)


def object_map(value: object) -> Mapping[object, object]:
    assert is_mapping(value), "Expected process result object"
    return value


class ProcessChecks(unittest.TestCase):
    root: ClassVar[Path]
    sentinel: ClassVar[Path]
    unrelated: ClassVar[subprocess.Popen[bytes]]
    records: ClassVar[list[Mapping[object, object]]]

    @classmethod
    @override
    def setUpClass(cls):
        cls.root = Path(tempfile.mkdtemp(prefix="harness-outcome-process-checks-"))
        cls.sentinel = cls.root / "unrelated.txt"
        _ = cls.sentinel.write_text("keep", encoding="utf-8")
        cls.unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(120)"],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        cls.records = []

    @classmethod
    @override
    def tearDownClass(cls):
        try:
            assert cls.unrelated.poll() is None, "Case killed unrelated owned sentinel process"
            assert cls.sentinel.read_text() == "keep"
        finally:
            cls.unrelated.terminate()
            _ = cls.unrelated.wait(timeout=5)
            _ = (cls.root / "reports.json").write_text(json.dumps(cls.records, indent=2), encoding="utf-8")
            print(f"Private process evidence: {cls.root}")

    def run_child(self, code: str, *, timeout: int = 10, ready_timeout: int | None = None, output_limit: int = 16 * 1024 * 1024):
        result = object_map(process_case.run_case([sys.executable, "-c", code], self.root, timeout=timeout, ready_timeout=ready_timeout, output_limit=output_limit))
        self.records.append(result)
        self.assertIsNone(self.unrelated.poll())
        self.assertEqual(self.sentinel.read_text(), "keep")
        return result

    def test_concurrent_streams_and_ready(self):
        result = self.run_child("import os,sys,threading; from pathlib import Path; "
            + "Path(os.environ['PROCESS_CASE_ROOT'],'ready.txt').write_text('READY'); "
            + "a=threading.Thread(target=lambda:sys.stdout.buffer.write(b'a'*2097152)); "
            + "b=threading.Thread(target=lambda:sys.stderr.buffer.write(b'b'*2097152)); "
            + "a.start(); b.start(); a.join(); b.join()", ready_timeout=2)
        self.assertEqual(result["status"], "exited", result)
        self.assertTrue(result["ready"])
        self.assertEqual(object_map(result["native"])["ExitCode"], 0)
        self.assertTrue(object_map(result["native"])["AssignedBeforeResume"])
        for value in object_map(result["streams"]).values():
            self.assertEqual(object_map(value)["bytes"], 2097152)

    def test_natural_nonzero(self):
        result = self.run_child("import sys; print('original failure',file=sys.stderr); sys.exit(7)")
        self.assertEqual(result["status"], "exited")
        self.assertEqual(object_map(result["native"])["ExitCode"], 7)

    def test_short_process_output_limit(self):
        result = self.run_child("print('x'*8192)", output_limit=1024)
        self.assertEqual(result["status"], "output-limit")
        self.assertTrue(result["output_limit_reached"])

    def test_readiness_deadline(self):
        result = self.run_child("import time; print('starting',flush=True); time.sleep(30)", timeout=8, ready_timeout=2)
        self.assertEqual(result["status"], "readiness-timeout", result)
        elapsed = result["elapsed_seconds"]
        assert isinstance(elapsed, (int, float)), "Expected numeric elapsed time"
        self.assertLess(elapsed, 15)

    def test_natural_exit_without_ready(self):
        result = self.run_child("print('no readiness')", ready_timeout=2)
        self.assertEqual(result["status"], "readiness-failure")
        self.assertEqual(object_map(result["native"])["ExitCode"], 0)

    def test_execution_timeout_reaps_descendant(self):
        marker = self.root / "descendant-after-timeout.txt"
        descendant = f"import time; from pathlib import Path; time.sleep(5); Path({str(marker)!r}).write_text('leaked')"
        code = f"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{descendant!r}]); time.sleep(30)"
        result = self.run_child(code, timeout=1)
        self.assertEqual(result["status"], "timeout", result)
        self.assertEqual(object_map(result["native"])["ExitCode"], 124)
        # A second bounded process provides the observation interval.
        _ = self.run_child("import time; time.sleep(5)")
        self.assertFalse(marker.exists(), "Descendant survived Job close")

    def test_infrastructure_failure(self):
        result = object_map(process_case.run_case([str(self.root / "missing.exe")], self.root))
        self.records.append(result)
        self.assertEqual(result["status"], "infrastructure-failure")

    def test_ambiguous_executable_and_unbounded_request_rejected(self):
        for argv, settings in ((["python"], {}), ([sys.executable], {"timeout":True}),
                               ([sys.executable], {"timeout":601}), ([sys.executable], {"ready_timeout":11}),
                               ([sys.executable], {"output_limit":0})):
            with self.subTest(argv=argv, settings=settings), self.assertRaises(ValueError):
                _ = process_case.run_case(argv, self.root, **settings)


if __name__ == "__main__":
    _ = unittest.main(verbosity=2)
