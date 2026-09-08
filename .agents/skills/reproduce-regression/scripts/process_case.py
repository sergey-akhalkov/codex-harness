"""Run an owned Windows process case using the kit's existing Job containment.

No shell expansion. The child receives PROCESS_CASE_ROOT; readiness is the exact
text READY in its new ready.txt. Logs and receipts are retained in a new temp root.
Native project test helpers remain preferable when they already meet the contract.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
from typing import Any


def run_case(argv: list[str], cwd: str | Path, timeout: int = 10,
             ready_timeout: int | None = None, output_limit: int = 16 * 1024 * 1024) -> dict[str, Any]:
    if not argv or not all(isinstance(arg, str) for arg in argv) or not Path(argv[0]).is_absolute():
        raise ValueError("Require an absolute executable and separately tokenized string arguments")
    if type(timeout) is not int or not 1 <= timeout <= 600 or (ready_timeout is not None and (type(ready_timeout) is not int or not 1 <= ready_timeout <= timeout)):
        raise ValueError("Require argv, timeout 1..600 and readiness within execution deadline")
    if type(output_limit) is not int or output_limit < 1:
        raise ValueError("output_limit must be positive")
    # resolve follows the normal installed skill directory link to authoritative sources.
    script = Path(__file__).resolve()
    supervisor = script.parents[4] / "tools" / "opencodex-process.ps1"
    pwsh = shutil.which("pwsh")
    if not pwsh or not supervisor.is_file():
        raise RuntimeError("Requires linked Windows harness and PowerShell 7.4+")
    root = Path(tempfile.mkdtemp(prefix="harness-process-case-")).resolve()
    request = dict(executable=str(Path(argv[0]).resolve()), arguments=argv[1:],
        workingDirectory=str(Path(cwd).resolve()), stdoutPath=str(root / "stdout.txt"),
        stderrPath=str(root / "stderr.txt"), startedPath=str(root / "started.json"),
        timeoutSeconds=timeout, memoryLimitMiB=512, environment={"PROCESS_CASE_ROOT": str(root)})
    (root / "request.json").write_text(json.dumps(request), encoding="utf-8")
    start = time.monotonic()
    infrastructure_error = None
    completed = None
    try:
        with (root / "supervisor.txt").open("w", encoding="utf-8") as log:
            completed = subprocess.run([pwsh, "-NoProfile", "-File", str(script.with_name("observe.ps1")),
                "-Supervisor", str(supervisor), "-CaseRoot", str(root),
                "-ReadySeconds", str(ready_timeout or 0), "-OutputLimit", str(output_limit)],
                stdout=log, stderr=subprocess.STDOUT, timeout=timeout + 45, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        infrastructure_error = type(error).__name__
    receipt = root / "observed.json"
    result = json.loads(receipt.read_text(encoding="utf-8-sig")) if receipt.is_file() else {
        "status": "infrastructure-failure", "supervisor_exit": completed.returncode if completed else None,
        "error": infrastructure_error}
    result.update(root=str(root), elapsed_seconds=time.monotonic() - start)
    result["streams"] = {name: {"path": str(root / f"{name}.txt"),
        "bytes": (root / f"{name}.txt").stat().st_size if (root / f"{name}.txt").exists() else None}
        for name in ("stdout", "stderr")}
    result["output_limit_reached"] = any((stream["bytes"] or 0) > output_limit for stream in result["streams"].values())
    if result["output_limit_reached"] and result["status"] == "exited":
        result["status"] = "output-limit"
    (root / "report.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cwd", type=Path, default=Path.cwd())
    parser.add_argument("--timeout", type=int, default=10)
    parser.add_argument("--ready-timeout", type=int)
    parser.add_argument("argv", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    argv = args.argv[1:] if args.argv[:1] == ["--"] else args.argv
    result = run_case(argv, args.cwd, args.timeout, args.ready_timeout)
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result["status"] == "exited" and result["native"]["ExitCode"] == 0 else 1)
