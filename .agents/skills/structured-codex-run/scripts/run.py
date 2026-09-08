"""Bounded native inspection and independent acceptance; no model calls on import."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

SCRIPT = Path(__file__).resolve()
SCHEMA = SCRIPT.parent.parent / "assets/inspection.schema.json"


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False), encoding="utf-8")


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def observer():
    path = SCRIPT.parents[2] / "reproduce-regression/scripts/process_case.py"
    spec = importlib.util.spec_from_file_location("structured_process_case", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("Installed process observer unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.run_case


def validate(value, schema):
    """Validate only the types/keywords used by the bundled, fixed schema."""
    kind = schema["type"]
    expected = {"object": dict, "array": list, "string": str, "integer": int}[kind]
    if type(value) is not expected:
        return False
    if kind == "object":
        return (set(value) == set(schema["required"]) == set(schema["properties"])
                and all(validate(value[key], child) for key, child in schema["properties"].items()))
    if kind == "array":
        return all(validate(item, schema["items"]) for item in value)
    return True


def fingerprint(cwd, inputs):
    result = {}
    for name in inputs:
        path = (cwd / name).resolve(strict=True)
        if not path.is_relative_to(cwd) or not path.is_file():
            raise ValueError("Inputs must be files inside the target checkout")
        result[name] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def command(value):
    if (not isinstance(value, list) or not value or
            not all(isinstance(arg, str) and "\0" not in arg for arg in value) or
            not Path(value[0]).is_absolute()):
        raise ValueError("Require an absolute executable and tokenized argument array")
    return value


def process_status(receipt, stderr):
    if receipt.get("status") != "exited":
        return receipt.get("status", "infrastructure-failure")
    code = (receipt.get("native") or {}).get("ExitCode")
    if code == 0:
        return None
    if isinstance(code, int) and (code < 0 or code >= 0x80000000):
        return "terminated"
    if re.search(r"(?i)\b(unauthorized|authentication failed|invalid api key|401)\b", stderr):
        return "auth-failure"
    return "process-failure"


def inspect_result(root, run_id, receipt, output_limit):
    stderr = (root / "stderr.txt").read_text(encoding="utf-8", errors="replace") if (root / "stderr.txt").exists() else ""
    failure = process_status(receipt, stderr)
    if failure:
        return failure
    paths = [root / name for name in ("final.json", "events.jsonl", "stderr.txt")]
    if (root / "output-limit.txt").exists() or any(p.exists() and p.stat().st_size > output_limit for p in paths):
        return "output-limit"
    final = root / "final.json"
    if not final.is_file():
        return "missing-json"
    try:
        value = read_json(final)
    except (ValueError, UnicodeError):
        return "malformed-json"
    if not validate(value, read_json(SCHEMA)):
        return "schema-invalid"
    if value["run_id"] != run_id:
        return "stale-json"
    try:
        events = [json.loads(line) for line in (root / "events.jsonl").read_text(encoding="utf-8").splitlines() if line.strip()]
        if not events or not all(isinstance(e, dict) for e in events):
            return "task-incomplete"
    except (OSError, ValueError, UnicodeError):
        return "malformed-events"
    if any(event.get("type") in ("error", "turn.failed") for event in events):
        return "task-failure"
    if events[-1].get("type") != "turn.completed":
        return "task-incomplete"
    if value["unresolved_issues"]:
        return "unresolved-issues"
    return None


def run_inspection(*, launch, cwd, prompt, oracle, model, provider, subscription,
                   inputs, timeout=180, output_limit=1048576, codex_home=None):
    command(launch)
    command(oracle)
    if type(timeout) is not int or not 1 <= timeout <= 600 or type(output_limit) is not int or output_limit < 1:
        raise ValueError("Require timeout 1..600 and positive output limit")
    if not all(isinstance(v, str) and v.strip() for v in (model, provider, subscription, prompt)):
        raise ValueError("Explicit route and nonempty prompt required")
    if len(prompt.encode("utf-8")) > 262144:
        raise ValueError("Prompt exceeds 256 KiB")
    if not inputs:
        raise ValueError("Record at least one relevant source input")
    cwd = Path(cwd).resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="structured-codex-")).resolve()
    run_id = uuid.uuid4().hex
    result = {"run_id": run_id, "status": "incomplete", "evidence_root": str(root)}
    write_json(root / "acceptance.json", result)
    try:
        before = fingerprint(cwd, inputs)
        git = {}
        for key, args in (("head", ["rev-parse", "HEAD"]), ("dirty", ["status", "--porcelain=v1", "--untracked-files=all"])):
            completed = subprocess.run(["git", "-C", str(cwd), *args], capture_output=True, timeout=10, check=True)
            git[key] = completed.stdout.decode("utf-8", errors="replace").strip()
        argv = [*launch, "exec", "--model", model, "-c", "model_provider=" + json.dumps(provider),
                "-c", "approval_policy=\"never\"", "--sandbox", "read-only", "--ephemeral", "--json",
                "-C", str(cwd), "--output-schema", str(SCHEMA), "--output-last-message", str(root / "final.json"), "-"]
        text = prompt + "\n\nReturn the contracted inspection JSON with run_id exactly " + run_id + ". Do not modify files or delegate."
        (root / "stdin.txt").write_bytes(text.encode("utf-8"))
        write_json(root / "contract.json", dict(run_id=run_id, cwd=str(cwd), git=git, inputs=before,
                   model=model, provider=provider, subscription=subscription, allowed_effects="read-only",
                   argv=argv, timeout=timeout, output_limit=output_limit, oracle=oracle,
                   executable_sha256=hashlib.sha256(Path(launch[0]).read_bytes()).hexdigest(),
                   python=sys.executable, python_version=sys.version, codex_home=str(codex_home) if codex_home else "inherited",
                   schema_sha256=hashlib.sha256(SCHEMA.read_bytes()).hexdigest(), started_at=time.time()))
        write_json(root / "launch.json", dict(argv=argv, stdin=str(root / "stdin.txt"),
                   final=str(root / "final.json"), output_limit=output_limit,
                   limit_marker=str(root / "output-limit.txt"), codex_home=str(Path(codex_home).resolve()) if codex_home else None))
        observe = observer()
        receipt = observe([sys.executable, str(SCRIPT.with_name("stdin_bridge.py")), str(root / "launch.json")],
                          cwd, timeout=timeout, output_limit=output_limit)
        write_json(root / "process.json", receipt)
        for source, dest in (("stdout", "events.jsonl"), ("stderr", "stderr.txt")):
            path = Path(receipt["streams"][source]["path"])
            if path.is_file():
                shutil.copyfile(path, root / dest)
        # The bridge's final limit stop is observer-owned, even though its exit is nonzero.
        if (root / "output-limit.txt").exists() and receipt.get("status") == "exited":
            result["status"] = "output-limit"
        else:
            result["status"] = inspect_result(root, run_id, receipt, output_limit) or "oracle-pending"
        after = fingerprint(cwd, inputs)
        write_json(root / "inputs-after.json", after)
        if before != after:
            result["inputs_changed"] = True
            if result["status"] == "oracle-pending":
                result["status"] = "inputs-changed"
        if result["status"] == "oracle-pending":
            oracle_receipt = observe([*oracle, str(root / "final.json")], cwd, timeout=min(timeout, 30), output_limit=output_limit)
            write_json(root / "oracle.json", oracle_receipt)
            code = (oracle_receipt.get("native") or {}).get("ExitCode")
            result["status"] = ("success" if code == 0 else "wrong-answer" if code == 1 else "oracle-failure") if oracle_receipt.get("status") == "exited" else "oracle-failure"
    except Exception as error:
        result.update(status="infrastructure-failure", error_type=type(error).__name__, error=str(error))
    finally:
        write_json(root / "acceptance.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("cwd", "prompt-file", "command-json", "oracle-json", "model", "provider", "subscription"):
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--input", action="append", default=[])
    parser.add_argument("--codex-home")
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--output-limit", type=int, default=1048576)
    args = parser.parse_args()
    result = run_inspection(launch=json.loads(args.command_json), cwd=args.cwd,
        prompt=Path(args.prompt_file).read_text(encoding="utf-8-sig"), oracle=json.loads(args.oracle_json),
        model=args.model, provider=args.provider, subscription=args.subscription, inputs=args.input,
        timeout=args.timeout, output_limit=args.output_limit, codex_home=args.codex_home)
    print(json.dumps(result))
    return 0 if result["status"] == "success" else 1


if __name__ == "__main__":
    raise SystemExit(main())
