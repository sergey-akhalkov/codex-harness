"""Opt-in native, subscription-backed acceptance; all work is in a new temp folder.

python tests/agent-delegation.py --run-model-probes --scenario benchmark
Uses the installed global launcher and existing Windows Job runner (2 GiB,
600 s/session). Does not stop/reconfigure the proxy or copy credentials. Raw
evidence is private; report.json includes only metadata, checks and usage.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[1]
CONTRACT = """Implement two independent standard-library Python modules in this directory.
selection.py: parse_selection(text, limit=10000) returns sorted distinct nonnegative
integers from comma-separated integers and inclusive a-b ranges. Strip whitespace
around comma tokens. Empty/whitespace text returns []. Reject empty internal tokens,
signs, descending ranges, non-ASCII digits and malformed syntax with ValueError.
limit is a nonnegative integer excluding bool; reject invalid limit with ValueError.
Reject a selection with more than limit DISTINCT values before expanding huge ranges.
intervals.py: merge_intervals(items) returns sorted tuples of merged closed intervals.
Each input is a pair of nonnegative integers excluding bool with start<=end.
Reject any invalid interval with ValueError; accept an iterable/generator of pairs.
Merge overlap and touching endpoints, not a one-unit gap. Never mutate inputs.
Only edit selection.py and intervals.py. acceptance.py is immutable, run it with
python after integration. No network, MCP or unrelated files. This is an explicitly
authorized disposable acceptance fixture outside OpenSpec. Finish the full result
without questions; give concise checks and any rework in the final answer."""

ACCEPTANCE = '''import unittest
from selection import parse_selection
from intervals import merge_intervals
class Selection(unittest.TestCase):
    def test_values(self):
        self.assertEqual(parse_selection(' 5-7,2,3,2,6 '), [2,3,5,6,7])
        self.assertEqual(parse_selection(''), [])
        self.assertEqual(parse_selection('   ', 0), [])
        self.assertEqual(parse_selection('0-2,0-2', 3), [0,1,2])
    def test_rejections(self):
        for text in ['1,,2', ',1', '2,', '-1', '+1', '3-1', '1-2-3', '١', '1.5']:
            with self.subTest(text=text), self.assertRaises(ValueError): parse_selection(text)
        for limit in [-1, True, 1.5]:
            with self.subTest(limit=limit), self.assertRaises(ValueError): parse_selection('1', limit)
        with self.assertRaises(ValueError): parse_selection('0-99999999999999', 5)
        with self.assertRaises(ValueError): parse_selection('0', 0)
class Intervals(unittest.TestCase):
    def test_merge(self):
        source = [[4,8], [1,3], [3,4], [10,10], [11,12]]
        original = [x[:] for x in source]
        self.assertEqual(merge_intervals(source), [(1,8),(10,10),(11,12)])
        self.assertEqual(source, original)
        self.assertEqual(merge_intervals(iter([(2,2),(2,2)])), [(2,2)])
        self.assertEqual(merge_intervals([]), [])
    def test_rejections(self):
        for pair in [(3,1),(-1,1),(True,2),(0,1.5),(0,), (0,1,2), '12', None]:
            with self.subTest(pair=pair), self.assertRaises(ValueError): merge_intervals([pair])
if __name__ == '__main__': unittest.main()
'''


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2), encoding="utf-8")


def events(path):
    with Path(path).open(encoding="utf-8-sig") as stream:
        return [json.loads(line) for line in stream if line.strip()]


def rollout(codex_home, thread_id):
    # Exact UUID suffix narrows files without reading unrelated conversations.
    found = list((codex_home / "sessions").rglob(f"*{thread_id}.jsonl"))
    if len(found) != 1:
        raise AssertionError("Expected exactly one persisted rollout for native thread")
    rows = events(found[0])
    meta = next(r["payload"] for r in rows if r.get("type") == "session_meta")
    assert meta["id"] == thread_id
    return found[0], rows


def run_case(root, name, prompt, codex_home, pwsh):
    case = root / name
    case.mkdir()
    (case / "acceptance.py").write_text(ACCEPTANCE, encoding="utf-8")
    (case / "task.txt").write_text(CONTRACT, encoding="utf-8")
    launcher = codex_home / "harness/bin/codex.ps1"
    request = dict(executable=pwsh, arguments=["-NoLogo", "-NoProfile", "-File",
        str(launcher), "exec", "--strict-config", "--skip-git-repo-check", "--json",
        "-C", str(case), "-m", "gpt-6-astra", "-c", 'model_reasoning_effort="xhigh"',
        "--output-last-message", str(case / "final.txt"), prompt +
        "\nWorking Python interpreter: " + str(sys.executable) + "; use its absolute path."],
        workingDirectory=str(case), stdoutPath=str(case / "events.jsonl"),
        stderrPath=str(case / "stderr.txt"), startedPath=str(case / "started.json"),
        memoryLimitMiB=2048, timeoutSeconds=600, environment={"CODEX_HOME": str(codex_home)})
    write_json(case / "request.json", request)
    try:
        with (case / "supervisor.txt").open("w", encoding="utf-8") as log:
            subprocess.run([pwsh,"-NoLogo","-NoProfile","-File",str(REPO / "tools/opencodex-process.ps1"),
                "-RequestPath",str(case / "request.json"),"-ResultPath",str(case / "result.json")],
                stdout=log, stderr=subprocess.STDOUT, check=False, timeout=630)
    except (OSError, subprocess.TimeoutExpired):
        # Verification retains whatever metadata/cost was written before failure.
        verify_case(case, codex_home)
        failed = read_json(case / "report.json")
        failed.update(status="failed", reason="Supervisor failed; see private raw evidence")
        write_json(case / "report.json", failed)
        raise
    return verify_case(case, codex_home)


def verify_case(case, codex_home):
    """Recheck private evidence without repeating a subscription-backed run."""
    report = dict(name=case.name, status="checking", checks=[], usage=None,
                  accounting_complete=False)
    paths = []
    try:
        return _verify_case(case, codex_home, report, paths)
    except Exception as error:
        report.update(status="failed", failure_type=type(error).__name__,
                      reason=str(error) if isinstance(error, AssertionError)
                      else "See private raw evidence")
        raise
    finally:
        module_spec = importlib.util.spec_from_file_location("delegation_usage", REPO / "tools/delegation-usage.py")
        assert module_spec and module_spec.loader
        usage_module = importlib.util.module_from_spec(module_spec)
        module_spec.loader.exec_module(usage_module)
        report["rollout_paths"] = [str(p) for p in paths]
        if paths:
            report["usage"] = usage_module.summarize_rollouts(paths)
            report["accounting_complete"] = (
                report.get("all_rollouts_found", False) and not report["usage"]["partial"])
        write_json(case / "report.json", report)


def _verify_case(case, codex_home, result_report, paths):
    name = case.name
    result = read_json(case / "result.json") if (case / "result.json").is_file() else None
    result_report["process"] = result
    rows = events(case / "events.jsonl")
    thread_id = next(r["thread_id"] for r in rows if r["type"] == "thread.started")
    parent_path, parent_rows = rollout(codex_home, thread_id)
    children = []
    for row in rows:
        item = row.get("item", {})
        if row.get("type") == "item.completed" and item.get("tool") == "spawn_agent":
            children.extend(item.get("receiver_thread_ids", []))
            children.extend(x["thread_id"] for x in item.get("receiver_threads", []) if "thread_id" in x)
    children = list(dict.fromkeys(children))
    paths.append(parent_path)
    result_report["thread_id"] = thread_id
    bindings = []
    result_report["bindings"] = bindings
    for child_id in children:
        child_path, child_rows = rollout(codex_home, child_id)
        paths.append(child_path)
        contexts = [r["payload"] for r in child_rows if r.get("type") == "turn_context"]
        assert contexts, "Child has no executed turn"
        meta = next(r["payload"] for r in child_rows if r.get("type") == "session_meta")
        assert meta["source"]["subagent"]["thread_spawn"]["parent_thread_id"] == thread_id
        bindings.append(dict(id=child_id, model=contexts[-1]["model"],
            effort=contexts[-1].get("effort"), start=child_rows[0]["timestamp"],
            end=child_rows[-1]["timestamp"],
            all_bindings=sorted({(c["model"], c.get("effort")) for c in contexts}),
            completed=any(r.get("type") == "event_msg" and r.get("payload", {}).get("type") == "task_complete" for r in child_rows)))
    completed_items = [r.get("item", {}) for r in rows if r.get("type") == "item.completed"]
    result_report.update(
        elapsed_seconds=round(result["ElapsedMilliseconds"]/1000,3) if result else None,
        bindings=bindings, rollout_paths=[str(p) for p in paths], process=result,
        contract_sha256=hashlib.sha256(CONTRACT.encode()).hexdigest(),
        checks=[], rework=dict(revision_requests=sum(i.get("tool") == "send_input" for i in completed_items)),
        parent_command_count=sum(i.get("type") == "command_execution" for i in completed_items),
        parent_file_changes=[c.get("path") for i in completed_items if i.get("type") == "file_change" for c in i.get("changes", [])])
    result_report["all_rollouts_found"] = True
    assert result and result["AssignedBeforeResume"] and result["ExitCode"] == 0, "Native process failed or containment was not established"
    parent_contexts = [r["payload"] for r in parent_rows if r.get("type") == "turn_context"]
    assert parent_contexts and all(c["model"] == "gpt-6-astra" and c.get("effort") == "xhigh"
                                  for c in parent_contexts), "Parent binding changed or absent"
    assert all(b["completed"] for b in bindings), "A child did not complete"
    if name in ("direct", "mixed"):
        assert (case / "acceptance.py").read_text(encoding="utf-8") == ACCEPTANCE, "Acceptance was modified"
        check = subprocess.run([sys.executable,"acceptance.py"], cwd=case,
            capture_output=True, text=True, timeout=30)
        (case / "acceptance-result.txt").write_text(check.stdout+check.stderr, encoding="utf-8")
        assert check.returncode == 0, "Fixture acceptance failed; inspect retained evidence"
        result_report["checks"].append("immutable acceptance passed")
        if name == "direct":
            assert not children, "Direct comparison unexpectedly delegated"
        else:
            assert len(bindings) == 2, "Mixed comparison needs exactly two workers"
            assert all(b["all_bindings"] == [("xai/grok-4.6", "xhigh")] for b in bindings), "Child binding changed"
            assert not result_report["parent_file_changes"], "Parent applied implementation patches"
            assert max(b["start"] for b in bindings) < min(b["end"] for b in bindings), "Workers did not overlap"
            result_report["checks"].append("two overlapping Grok xhigh child lifetimes")
    else:
        expected = {"high", "xhigh", "max"}
        assert len(bindings) == 3 and {b["effort"] for b in bindings} == expected
        assert all(b["model"] == "gpt-6-astra" for b in bindings)
        assert all(len(b["all_bindings"]) == 1 for b in bindings), "Level binding changed"
        final = (case / "final.txt").read_text(encoding="utf-8")
        assert all(marker in final for marker in ("BACKUP_OK", "SENIOR_OK", "PRINCIPAL_OK"))
        result_report["checks"].append("three native Astra level bindings and returned results")
    result_report["status"] = "accepted"
    return result_report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-model-probes", action="store_true")
    parser.add_argument("--revalidate", type=Path, help="Existing private evidence root; no model calls")
    parser.add_argument("--scenario", choices=["benchmark","levels","all"], default="benchmark")
    args = parser.parse_args()
    if args.revalidate:
        codex_home = Path(os.environ.get("CODEX_HOME", str(Path.home()/".codex"))).resolve()
        reports = []
        for name in ("direct", "mixed", "levels"):
            case = args.revalidate.resolve() / name
            if (case / "result.json").is_file():
                try:
                    reports.append(verify_case(case, codex_home))
                except Exception as error:
                    write_json(case / "failure.json", dict(status="failed", failure_type=type(error).__name__,
                        reason=str(error) if isinstance(error, AssertionError) else "See private raw evidence"))
                    raise
        assert reports, "No completed scenarios to revalidate"
        write_json(args.revalidate / "revalidated.json", dict(status="accepted", scenarios=reports))
        print(f"Revalidated {len(reports)} scenarios without model calls")
        return
    if not args.run_model_probes:
        print("SKIP: --run-model-probes explicitly spends subscription quota")
        return
    if os.name != "nt":
        raise SystemExit("Native Windows job containment is required")
    codex_home = Path(os.environ.get("CODEX_HOME", str(Path.home()/".codex"))).resolve()
    pwsh = shutil.which("pwsh")
    assert pwsh and (codex_home / "harness/bin/codex.ps1").is_file()
    root = Path(tempfile.mkdtemp(prefix="codex-agent-delegation-"))
    print(f"Private evidence: {root}", flush=True)
    report = dict(status="running", scenarios=[], limitation="One matched fixture, not weekly-quota or general performance proof")
    try:
        if args.scenario in ("benchmark", "all"):
            for name, instruction in [("direct", "Implement both modules yourself; do not spawn agents."),
                ("mixed", "Spawn exactly two named middle agents with fork_context=false before waiting. "
                 "Give one exclusive ownership of selection.py and the other intervals.py, each its "
                 "contract. They may read acceptance.py but must not edit it or spawn. Use no other "
                 "models. Integrate, inspect meaningful corners and run acceptance.py yourself.")]:
                print(f"Running {name}", flush=True)
                report["scenarios"].append(run_case(root,name,CONTRACT+"\n"+instruction,codex_home,pwsh))
                write_json(root / "report.json", report)
        if args.scenario in ("levels", "all"):
            prompt = ("Bounded native level configuration acceptance, not routine production escalation. "
                "Do not read or write files or use network/MCP. The fixture explicitly simulates "
                "Grok being unavailable: do not call it. Sequentially spawn, wait and close exactly "
                "one of each named agent_type middle_backup, senior, principal, fork_context=false, "
                "without model overrides. Ask middle_backup to compute 17*19 and return BACKUP_OK:323; "
                "ask senior whether replacing x with x+1 commutes with doubling x, return SENIOR_OK "
                "plus a counterexample; ask principal for a minimal interleaving explaining why "
                "two read-increment-write operations lose an update, return PRINCIPAL_OK with the "
                "interleaving. Tasks forbid tools/other agents. Check answers and include all three "
                "markers in one concise final. This single max call verifies the binding only.")
            report["scenarios"].append(run_case(root,"levels",prompt,codex_home,pwsh))
        report["status"] = "passed"
    except BaseException as error:
        report["status"] = "failed"
        report["failure_type"] = type(error).__name__
        report["reason"] = str(error) if isinstance(error, AssertionError) else "See private raw evidence"
        raise
    finally:
        write_json(root / "report.json", report)
        print(f"Report: {root / 'report.json'}", flush=True)


if __name__ == "__main__":
    main()
