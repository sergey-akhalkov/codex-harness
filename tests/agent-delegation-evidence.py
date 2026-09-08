"""Offline regressions for failed native acceptance evidence. No model calls."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from dataclasses import dataclass
from typing import override, Protocol, runtime_checkable, TypeGuard
from collections.abc import Mapping

@runtime_checkable
class Probe(Protocol):
    def write_json(self, path: Path, value: object) -> None: ...
    def verify_case(self, case: Path, codex_home: Path) -> object: ...
    def read_json(self, path: Path) -> object: ...


def load_probe() -> Probe:
    spec = importlib.util.spec_from_file_location("delegation_probe", Path(__file__).with_name("agent-delegation.py"))
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert isinstance(module, Probe)
    return module


probe = load_probe()


def is_mapping(value: object) -> TypeGuard[Mapping[object, object]]:
    return isinstance(value, Mapping)


def object_map(value: object) -> Mapping[object, object]:
    assert is_mapping(value), "Expected an object"
    return value


@dataclass
class _Fixture:
    root: Path
    case: Path
    native_events: list[dict[str, object]]
    parent: list[dict[str, object]]
    result: dict[str, int | bool]


class EvidenceTests(unittest.TestCase):
    _fixture: _Fixture | None = None

    @property
    def fixture(self) -> _Fixture:
        assert self._fixture is not None, "setUp has not completed"
        return self._fixture


    @override
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        case = root / "direct"
        case.mkdir()
        (root / "sessions").mkdir()
        native_events: list[dict[str, object]] = [{"type": "thread.started", "thread_id": "parent"}]
        parent: list[dict[str, object]] = [
            {"type": "session_meta", "payload": {"id": "parent", "model_provider": "openai"}},
            {"type": "turn_context", "payload": {"model": "gpt-6-astra", "effort": "xhigh"}},
            {"type": "event_msg", "payload": {"type": "token_count", "info": {
                "total_token_usage": dict(input_tokens=20, cached_input_tokens=10,
                    output_tokens=5, reasoning_output_tokens=2, total_tokens=25)}}},
        ]
        result = dict(AssignedBeforeResume=True, ExitCode=0, ElapsedMilliseconds=100)
        self._fixture = _Fixture(root, case, native_events, parent, result)

    def verify_failure(self):
        probe.write_json(self.fixture.case / "result.json", self.fixture.result)
        for path, rows in [(self.fixture.case / "events.jsonl", self.fixture.native_events),
                           (self.fixture.root / "sessions/rollout-parent.jsonl", self.fixture.parent)]:
            _ = path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
        with self.assertRaises(Exception):
            _ = probe.verify_case(self.fixture.case, self.fixture.root)
        report = object_map(probe.read_json(self.fixture.case / "report.json"))
        self.assertEqual(report["status"], "failed")
        return report

    def test_failed_process_keeps_available_cost(self):
        self.fixture.result["ExitCode"] = 125
        report = self.verify_failure()
        self.assertEqual(object_map(object_map(report["usage"])["totals"])["total_tokens"], 25)
        self.assertEqual(object_map(report["process"])["ExitCode"], 125)

    def test_missing_thread_is_unknown_not_zero(self):
        self.fixture.native_events = []
        report = self.verify_failure()
        self.assertIsNone(report["usage"])
        self.assertFalse(report["accounting_complete"])

    def test_missing_child_keeps_parent_cost_but_incomplete_accounting(self):
        self.fixture.native_events.append({"type": "item.completed", "item": {
            "tool": "spawn_agent", "receiver_thread_ids": ["missing-child"]}})
        report = self.verify_failure()
        self.assertEqual(object_map(object_map(report["usage"])["totals"])["total_tokens"], 25)
        self.assertFalse(report["accounting_complete"])

    def test_earlier_wrong_binding_cannot_hide_behind_final_binding(self):
        self.fixture.parent.insert(1, {"type": "turn_context", "payload": {
            "model": "gpt-6-astra", "effort": "high"}})
        report = self.verify_failure()
        self.assertEqual(report["reason"], "Parent binding changed or absent")
        self.assertEqual(object_map(object_map(report["usage"])["totals"])["total_tokens"], 25)


if __name__ == "__main__":
    _ = unittest.main()
