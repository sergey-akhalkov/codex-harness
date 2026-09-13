#!/usr/bin/env python3
"""One native baseline/candidate pair for each controlled outcome case.

Explicit --run-model-probes spends ChatGPT quota. External consumer cases are
out of scope for this driver. Evidence stays in a private temporary root.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any

CASES = ("freshness", "entrypoint", "process", "missing", "negative")
ARMS = ("baseline", "candidate")
MODEL = "gpt-6-astra"
EFFORT = "xhigh"


def disk_path(path: Path) -> Path:
    text = str(path)
    if text.startswith("\\\\?\\"):
        text = text[4:]
    return Path(text)


def write_json(path: Path, value: Any) -> None:
    staged = path.with_suffix(path.suffix + ".tmp")
    staged.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    staged.replace(path)


def run_cli(exe: Path, args: list[str], cwd: Path, timeout: int = 120) -> dict[str, Any]:
    completed = subprocess.run(
        [str(exe), *args],
        cwd=cwd,
        capture_output=True,
        timeout=timeout,
        check=False,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    stdout = completed.stdout.decode("utf-8", errors="replace")
    stderr = completed.stderr.decode("utf-8", errors="replace")
    payload: Any = None
    try:
        payload = json.loads(stdout)
    except ValueError:
        payload = None
    return {
        "argv": [str(exe), *args],
        "cwd": str(cwd),
        "exit_code": completed.returncode,
        "stdout": stdout,
        "stderr": stderr,
        "payload": payload,
    }


def seed_home(base_home: Path, case_root: Path, auth: Path) -> None:
    config = (
        f'approval_policy = "never"\n'
        f'sandbox_mode = "danger-full-access"\n'
        f'model = "{MODEL}"\n'
        f'model_reasoning_effort = "{EFFORT}"\n'
        f'check_for_update_on_startup = false\n'
        f'approvals_reviewer = "user"\n'
        f'suppress_unstable_features_warning = true\n'
        f'\n[features]\nhooks = false\ncode_mode = true\n'
        f'\n[projects.{json.dumps(str(case_root))}]\ntrust_level = "trusted"\n'
    )
    (base_home / "config.toml").write_text(config, encoding="utf-8")
    profile_src = Path(__file__).resolve().parents[1] / "global" / "harness.config.toml"
    shutil.copyfile(profile_src, base_home / "harness.config.toml")
    hooks = base_home / "hooks.json"
    if not hooks.exists():
        hooks.write_text('{"hooks":{}}\n', encoding="utf-8")
    target = base_home / "auth.json"
    if target.exists() or target.is_symlink():
        target.unlink()
    target.symlink_to(auth)
    skills = base_home / "skills"
    skills.mkdir(exist_ok=True)
    user_skills = base_home.parent / ".agents" / "skills"
    if user_skills.is_dir():
        for entry in user_skills.iterdir():
            dest = skills / entry.name
            if not dest.exists():
                dest.symlink_to(entry, target_is_directory=entry.is_dir())


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-model-probes", action="store_true")
    parser.add_argument("--harness", type=Path, required=True)
    parser.add_argument("--observer", type=Path, required=True)
    parser.add_argument("--upstream", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--base-home", type=Path, required=True)
    parser.add_argument("--base-user", type=Path, required=True)
    parser.add_argument("--auth", type=Path, required=True)
    parser.add_argument("--cases", nargs="+", default=list(CASES))
    parser.add_argument("--timeout", type=int, default=600)
    args = parser.parse_args(argv)
    if not args.run_model_probes:
        print("SKIP: --run-model-probes required")
        return 0
    unknown = [c for c in args.cases if c not in CASES]
    if unknown:
        raise SystemExit("unsupported cases: " + ", ".join(unknown))
    root = Path(tempfile.mkdtemp(prefix="codex-outcome-5_3-"))
    report: dict[str, Any] = {
        "status": "running",
        "evidence_root": str(root),
        "model_calls": 0,
        "started_at": time.time(),
        "attempts": [],
        "excluded": {
            "focused": "historical source-kit consumer; no further run",
            "second": "real-consumer benefit pairs remain user-gated",
            "reduction": "uses withdrawn source-kit wrapper; not resumed",
        },
        "capability_selection": {
            "hooks": "off",
            "fast": False,
            "native_memories": False,
            "model": MODEL,
            "effort": EFFORT,
        },
    }
    write_json(root / "suite.json", report)

    def save() -> None:
        report["ended_at"] = time.time()
        write_json(root / "suite.json", report)

    try:
        for case_id in args.cases:
            pair_dir = root / case_id
            pair_dir.mkdir()
            for arm in ARMS:
                attempt_id = f"{case_id}-1-{arm}"
                folder = pair_dir / arm
                folder.mkdir()
                row: dict[str, Any] = {
                    "attempt_id": attempt_id,
                    "case_id": case_id,
                    "arm": arm,
                    "repeat": 1,
                    "status": "incomplete",
                    "started_at": time.time(),
                    "excluded_reasons": [],
                    "evidence_root": str(folder),
                }
                report["attempts"].append(row)
                save()
                try:
                    home = disk_path(args.base_home.resolve())
                    user = disk_path(args.base_user.resolve())
                    prepare_args = ["outcome-prepare", "--case", case_id]
                    if case_id == "process":
                        prepare_args += ["--observer", str(args.observer.resolve())]
                    prepared = run_cli(args.harness, prepare_args, folder, timeout=180)
                    write_json(folder / "prepare.json", prepared)
                    payload = prepared["payload"] or {}
                    if prepared["exit_code"] != 0 or payload.get("status") != "passed":
                        row["status"] = "blocked"
                        row["excluded_reasons"].append("preparation_failed")
                        continue
                    setup = payload["setup"]
                    write_json(folder / "setup.json", setup)
                    owned_case = disk_path(Path(payload["case_root"]))
                    seed_home(home, owned_case, disk_path(args.auth.resolve()))
                    arm_request = {
                        "case_root": str(owned_case),
                        "codex_home": str(home),
                        "user_home": str(user),
                        "dependency_user_home": str(user),
                        "source_root": str(args.source_root.resolve()),
                        "upstream": str(args.upstream.resolve()),
                        "arm": arm,
                        "timeout": 30,
                    }
                    arm_path = folder / "arm-request.json"
                    write_json(arm_path, arm_request)
                    armed = run_cli(args.harness, ["outcome-arm", "--request", str(arm_path)], folder, timeout=90)
                    write_json(folder / "arm.json", armed)
                    if armed["exit_code"] != 0 or not (armed.get("payload") or {}).get("discovery_verified"):
                        row["status"] = "blocked"
                        row["excluded_reasons"].append("arm_preparation_failed")
                        continue
                    run_request = {
                        "case_root": str(owned_case),
                        "codex_home": str(home),
                        "launcher": str(args.upstream.resolve()),
                        "prompt": setup["prompt"],
                        "timeout": args.timeout,
                    }
                    run_path = folder / "run-request.json"
                    write_json(run_path, run_request)
                    report["model_calls"] += 1
                    save()
                    native = run_cli(
                        args.harness,
                        ["outcome-run", "--request", str(run_path), "--run-model-probes"],
                        folder,
                        timeout=args.timeout + 90,
                    )
                    write_json(folder / "run.json", native)
                    native_payload = native["payload"] or {}
                    row["native"] = {
                        "status": native_payload.get("status"),
                        "evidence_root": native_payload.get("evidence_root"),
                        "elapsed_seconds": native_payload.get("elapsed_seconds"),
                        "usage": native_payload.get("usage"),
                    }
                    execution = Path(native_payload.get("evidence_root") or folder) / "native.json"
                    oracle_request = {
                        "case_root": str(owned_case),
                        "setup": setup,
                        "execution": str(execution),
                        "arm": arm,
                    }
                    oracle_path = folder / "oracle-request.json"
                    write_json(oracle_path, oracle_request)
                    oracle = run_cli(args.harness, ["outcome-oracle", "--request", str(oracle_path)], folder, timeout=180)
                    write_json(folder / "oracle.json", oracle)
                    oracle_payload = oracle["payload"] or {}
                    row["oracle"] = {
                        "passed": oracle_payload.get("passed"),
                        "exit_code": oracle_payload.get("exit_code"),
                        "checks": (oracle_payload.get("details") or {}).get("checks"),
                        "skill_use": (oracle_payload.get("details") or {}).get("skill_use"),
                    }
                    if oracle_payload.get("passed") is True and native_payload.get("status") == "completed":
                        row["status"] = "accepted"
                    elif oracle_payload.get("passed") is False:
                        row["status"] = "failed"
                    else:
                        row["status"] = native_payload.get("status") or "incomplete"
                except Exception as error:
                    row["status"] = "failed"
                    row["error_type"] = type(error).__name__
                    (folder / "failure.txt").write_text(str(error), encoding="utf-8")
                finally:
                    row["ended_at"] = time.time()
                    save()
        statuses = {a.get("status") for a in report["attempts"]}
        report["status"] = "accepted" if statuses == {"accepted"} and report["attempts"] else "incomplete"
    except BaseException as error:
        report["status"] = "blocked"
        report["error_type"] = type(error).__name__
        (root / "failure.txt").write_text(str(error), encoding="utf-8")
        raise
    finally:
        save()
        print(json.dumps({k: report[k] for k in ("status", "model_calls", "evidence_root")}))
    return 0 if report["status"] == "accepted" else 1


if __name__ == "__main__":
    raise SystemExit(main())
