"""Fixed, opt-in acceptance inputs. Never touches a consumer's live working tree."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from typing import Any, TypedDict
import sys


class SourceState(TypedDict):
    root: str
    head: str
    files: dict[str, str]
    tree_sha256: str

REPO = Path(__file__).resolve().parents[1]
CASE_IDS = ("focused", "second", "freshness", "entrypoint", "reduction", "process", "missing", "negative")
PROMPTS = {
    "focused": "Discover and execute the project's native focused library validation. Preserve source. Record the actual command, result and evidence in the existing documentation home; report any incomplete or failed check accurately.",
    "second": "Discover and execute this project's native lint validation. Preserve source. Record the actual command and result in its documentation home and explain the scope of evidence.",
    "freshness": "Continue verification from docs/validation.md for the current source version. Verify the documented product CLI, retaining the earlier record and accurately updating its applicability. Build generated output if required; preserve the expected behavior.",
    "entrypoint": "Verify that the documented product CLI reflects the current source version. Resolve and exercise the actual entrypoint and required build. Preserve source and expected behavior; generated output may be rebuilt.",
    "reduction": "Reproduce the reported failure: tools/run-focused-test.ts fails on tools/outcome-original.mjs although that valid child should complete. Preserve the wrapper. Establish a reference, reduce the input into tools/outcome-minimal.mjs while preserving the original failure, and assess tools/outcome-wrong.mjs as a proposed reduction. Retain evidence and the original input; no product fix is requested.",
    "process": "Investigate the supplied process_target.py cases: flood, no-ready, hang and fail. Exercise the subprocesses with bounded capture and cleanup. Distinguish natural exit, readiness failure and forced timeout, preserving both streams and unrelated resources. Write a compact executable check check_process.py following README.md plus result evidence. Use owned resources only.",
    "missing": "Execute the required verification command in verification.json if its prerequisite exists. Package installation and substitution are forbidden in this controlled case. Preserve inputs and give an accurate result with the missing prerequisite if blocked.",
    "negative": "Correct 'verfication' to 'verification' in README.md and validate its local link. This is a documentation-only change. Preserve the other file.",
}
CONTRACT = """This is an explicitly authorized disposable acceptance task outside OpenSpec.
Only modify this owned project copy, its documentation, generated fixtures and private
evidence. Do not modify live checkouts, provider configuration, credentials or shared
services. No network or package installation is needed. Do not run model evaluations
or delegate this bounded task. Preserve the supplied immutable inputs. Leave concise
evidence and write outcome.json containing status (passed/failed/blocked), command,
observed result, evidence paths, and scope. Never count a skipped check as passed.
"""

FOCUSED_INPUT_VERSION = "focused-private-env-v1"
FOCUSED_PREPARE = r'''#requires -Version 7.4
function New-FocusedRequest {
    param([Parameter(Mandatory)][string]$Workspace)
    $ErrorActionPreference = 'Stop'
    if (-not $IsWindows) { throw 'Focused calibration requires Windows.' }
    $state = Get-Content (Join-Path $PSScriptRoot 'focused-input-state.json') -Raw | ConvertFrom-Json
    $workspacePath = (Resolve-Path -LiteralPath $Workspace).Path
    if ((Get-FileHash -LiteralPath $state.node -Algorithm SHA256).Hash.ToLowerInvariant() -ne $state.node_sha256) {
        throw 'Recorded Node executable changed; recalibrate before acceptance.'
    }
    # TEMP under the real user profile still exposes ancestor .agents skills.
    $base = Join-Path ([Environment]::GetFolderPath('Windows')) 'Temp'
    $ancestor = $base
    while ($ancestor) {
        foreach ($relative in @('.agents/skills', '.claude/skills', '.opencode')) {
            if (Test-Path -LiteralPath (Join-Path $ancestor $relative)) {
                throw 'Native fixture root has ancestor skill/config registrations.'
            }
        }
        $next = Split-Path $ancestor -Parent
        if ($next -eq $ancestor) { break }
        $ancestor = $next
    }
    $caseRoot = Join-Path $base ('harness-focused-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $caseRoot -ErrorAction Stop | Out-Null
    $environment = @{
        OPENCODE_CONFIG_DIR = (Join-Path $workspacePath 'global')
        OPENCODE_DISABLE_MODELS_FETCH = '1'
        OPENCODE_DISABLE_AUTOUPDATE = '1'
    }
    foreach ($pair in @(@('HOME','home'), @('USERPROFILE','home'), @('OPENCODE_TEST_HOME','home'),
            @('XDG_CONFIG_HOME','config'), @('XDG_DATA_HOME','data'), @('XDG_CACHE_HOME','cache'),
            @('XDG_STATE_HOME','state'), @('TEMP','tmp'), @('TMP','tmp'))) {
        $directory = Join-Path $caseRoot $pair[1]
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
        $environment[$pair[0]] = $directory
    }
    $environment.OPENCODE_DB = Join-Path $caseRoot 'data/opencode.db'
    return @{
        executable = $state.node
        arguments = @('tools/run-focused-test.ts', 'tools/test-library.ts')
        workingDirectory = $workspacePath
        stdoutPath = (Join-Path $caseRoot 'stdout.txt')
        stderrPath = (Join-Path $caseRoot 'stderr.txt')
        startedPath = (Join-Path $caseRoot 'started.json')
        timeoutSeconds = 600
        memoryLimitMiB = 2048
        environment = $environment
    }
}
'''
FOCUSED_CHECK = r'''#requires -Version 7.4
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
if ($args.Count) { throw 'Focused acceptance does not accept test selectors or alternate commands.' }
. (Join-Path $PSScriptRoot 'focused-prepare.ps1')
$workspace = Split-Path $PSScriptRoot -Parent
$request = New-FocusedRequest -Workspace $workspace
$caseRoot = Split-Path $request.stdoutPath -Parent
$requestPath = Join-Path $caseRoot 'request.json'
$resultPath = Join-Path $caseRoot 'result.json'
$request | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $requestPath -Encoding utf8
# A unique locator retains all attempts, including failures; never overwrite a previous run.
$locators = Join-Path $workspace 'acceptance-runs'
New-Item -ItemType Directory -Path $locators -Force | Out-Null
@{ request = $requestPath; result = $resultPath; stdout = $request.stdoutPath; stderr = $request.stderrPath } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $locators ((Split-Path $caseRoot -Leaf) + '.json'))
$result = & (Join-Path $PSScriptRoot 'opencodex-process.ps1') -RequestPath $requestPath -ResultPath $resultPath -PassThru
[Console]::Out.Write([IO.File]::ReadAllText($request.stdoutPath))
[Console]::Error.Write([IO.File]::ReadAllText($request.stderrPath))
exit $result.ExitCode
'''


def prepare_focused(destination: Path, state: SourceState) -> dict[str, Any]:
    """Install immutable acceptance preparation without changing consumer bytes."""
    node = shutil.which("node")
    if not node:
        raise RuntimeError("Focused preparation requires the calibrated Node executable")
    acceptance = destination / "acceptance"
    acceptance.mkdir(exist_ok=False)
    (acceptance / "focused-prepare.ps1").write_text(FOCUSED_PREPARE, encoding="utf-8")
    (acceptance / "focused-check.ps1").write_text(FOCUSED_CHECK, encoding="utf-8")
    for name in ("opencodex-process.ps1", "opencodex-process.cs"):
        shutil.copyfile(REPO / "tools" / name, acceptance / name)
    preparation = {
        "version": FOCUSED_INPUT_VERSION, "source_state": state["tree_sha256"],
        "source_file_count": len(state["files"]), "node": str(Path(node).resolve()),
        "node_sha256": digest(node), "summary": "OK: library tests=183",
        "files": {p.name: digest(p) for p in sorted(acceptance.iterdir())},
    }
    (acceptance / "focused-input-state.json").write_text(json.dumps(preparation, indent=2), encoding="utf-8")
    (acceptance / "README.md").write_text(
        "# Focused native check setup\n\n"
        "Identical baseline/candidate prerequisite: from the owned workspace run "
        "`pwsh -NoProfile -File acceptance/focused-check.ps1`. This runs exactly "
        "`node tools/run-focused-test.ts tools/test-library.ts` in this workspace, "
        "with the recorded Node executable and a 600-second / 2 GiB owned Windows Job.\n\n"
        "Only the native subprocess receives private HOME/USERPROFILE/OPENCODE_TEST_HOME, "
        "XDG config/data/cache/state, OPENCODE_DB and TEMP/TMP. Fixtures are created under "
        "Windows/Temp outside ancestor skill registrations; OPENCODE_CONFIG_DIR selects "
        "this workspace's global/. No host links, Codex HOME, model configuration, "
        "consumer files or test assertions are changed. Model fetching and auto-update "
        "are disabled for the native subprocess.\n\n"
        "The immutable input-state records source identity, preparation version and hashes. "
        "Every invocation creates a fresh root; acceptance-runs/*.json locates raw streams, "
        "request and native result, including failed/interrupted attempts. Preserve these. "
        "Success requires a completed invocation, natural exit 0 and exactly "
        "`OK: library tests=183` (apart from the terminal newline). Timeout is not success. "
        "Record the actual command/result in the existing project documentation home.\n",
        encoding="utf-8")
    return {"version": FOCUSED_INPUT_VERSION, "input_state": "acceptance/focused-input-state.json",
            "input_state_sha256": digest(acceptance / "focused-input-state.json"),
            "command": ["pwsh", "-NoProfile", "-File", "acceptance/focused-check.ps1"]}


def digest(path: str | Path) -> str:
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_state(root: str | Path) -> SourceState:
    """Record Git-listed source bytes, including nonignored dirty/untracked files."""
    root = Path(root).resolve()
    result = subprocess.run(["git", "-C", str(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                            check=True, capture_output=True)
    files = {}
    for name in sorted(set(result.stdout.decode("utf-8").split("\0")) - {""}):
        path = root / name
        if path.is_file():
            # Copy file contents only; reject links escaping the source boundary.
            if not path.resolve().is_relative_to(root):
                raise ValueError(f"Source link escapes recorded root: {name}")
            files[name] = digest(path)
    revision = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    return {"root": str(root), "head": revision, "files": files,
            "tree_sha256": hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()}


def snapshot(root: str | Path, destination: str | Path) -> SourceState:
    before = source_state(root)
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=False)
    for name in before["files"]:
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(Path(root) / name, target)
        if digest(target) != before["files"][name]:
            raise RuntimeError(f"Source changed while copying {name}")
    if source_state(root) != before:
        raise RuntimeError("Source changed during snapshot; preserve attempt, prepare again")
    return before


def prepare(root=None):
    root = Path(root) if root else Path(tempfile.mkdtemp(prefix="harness-outcomes-"))
    root.mkdir(parents=True, exist_ok=True)
    records = {}
    for name in ("opencode-kit", "team-control"):
        records[name] = snapshot(REPO.parent / name, root / "sources" / name)
    rollback = Path(tempfile.gettempdir()) / "harness-reconciliation-rollback-5fe436a80fb745afa32a0844570741ba"
    old = root / "hooks-before"
    old.mkdir()
    for part in ("tools", "global"):
        shutil.copytree(rollback / part, old / part)
    records["hooks-before"] = {str(p.relative_to(old)): digest(p) for p in old.rglob("*") if p.is_file()}
    records["hook-source"] = {name: digest(REPO / name) for name in (
        "tools/lsp/discovery.py", "tools/lsp/journal.py", "tools/lsp/server.py", "tools/lsp/backend.py", "tools/hook.ps1", "global/hooks.json")}
    records["environment"] = {"node": subprocess.check_output(["node", "--version"], text=True).strip(),
                               "os": os.name}
    (root / "inputs.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
    return root


def case_workspace(inputs: Path, destination: Path, case_id: str) -> dict[str, Any]:
    """Prepare a fresh equivalent case; no record produced by a previous arm is copied."""
    if case_id not in CASE_IDS:
        raise ValueError(case_id)
    records = json.loads((inputs / "inputs.json").read_text(encoding="utf-8"))
    consumer = "opencode-kit" if case_id in ("focused", "reduction") else "team-control" if case_id == "second" else None
    destination.mkdir(parents=True, exist_ok=False)
    if consumer:
        source = inputs / "sources" / consumer
        for name, expected in records[consumer]["files"].items():
            if digest(source / name) != expected:
                raise ValueError(f"Frozen source drift: {consumer}/{name}")
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / name, target)
        dependencies = source / "node_modules"
        if dependencies.exists():
            (destination / "node_modules").symlink_to(dependencies.resolve(), target_is_directory=True)
    focused_preparation = prepare_focused(destination, records["opencode-kit"]) if case_id == "focused" else None
    if case_id in ("freshness", "entrypoint"):
        (destination / "source.json").write_text('{"version":2}\n')
        (destination / "built.json").write_text('{"version":1}\n')
        (destination / "build.py").write_text("import json\nfrom pathlib import Path\nr=Path(__file__).parent\nr.joinpath('built.json').write_bytes(r.joinpath('source.json').read_bytes())\nwith r.joinpath('execution-audit.jsonl').open('a') as f: f.write(json.dumps(dict(entrypoint='build',version=json.loads(r.joinpath('source.json').read_text())['version']))+'\\n')\n")
        (destination / "cli.py").write_text("import json\nfrom pathlib import Path\nr=Path(__file__).parent\nv=json.loads(r.joinpath('built.json').read_text())['version']\nwith r.joinpath('execution-audit.jsonl').open('a') as f: f.write(json.dumps(dict(entrypoint='cli',version=v))+'\\n')\nprint(v)\n")
        (destination / "README.md").write_text("# Product CLI\nBuild: `python build.py`. Product entrypoint: `python cli.py`. It must print the current source version.\n")
        if case_id == "freshness":
            (destination / "docs").mkdir()
            # Equivalent pre-existing historical confirmation in both arms.
            (destination / "docs/validation.md").write_text("# Validation\nHistorical record: confirmed `python cli.py`, cwd root, Python 3.13, source version 1, built version 1. Last successful execution 2026-09-07: stdout 1, exit 0. Only CLI version was checked. Current source has changed since that execution.\n")
    if case_id == "reduction":
        (destination / "tools/outcome-original.mjs").write_text("import fs from 'node:fs'; fs.writeSync(1,Buffer.alloc(2097152,97)); fs.writeSync(2,Buffer.alloc(2097152,98)); console.log('OK: noisy child');\n")
        (destination / "tools/outcome-wrong.mjs").write_text("throw new SyntaxError('different failure');\n")
    if case_id == "process":
        (destination / "process_target.py").write_text('''import os,sys,time,threading,subprocess,json
from pathlib import Path
mode=sys.argv[1]
audit=Path(__file__).with_name('process-audit.jsonl')
with audit.open('a') as f: f.write(json.dumps(dict(mode=mode,pid=os.getpid(),event='start'))+'\\n')
if mode=='flood':
    a=threading.Thread(target=lambda:sys.stdout.buffer.write(b'a'*2097152))
    b=threading.Thread(target=lambda:sys.stderr.buffer.write(b'b'*2097152))
    a.start(); b.start(); a.join(); b.join()
elif mode=='fail':
    print('natural failure',file=sys.stderr); sys.exit(7)
elif mode in ('no-ready','hang'):
    if mode=='hang':
        subprocess.Popen([sys.executable,'-c',"import time;from pathlib import Path;time.sleep(6);Path('descendant-survived.txt').write_text('survived')"])
    print('starting',flush=True); time.sleep(30)
else: sys.exit(2)
with audit.open('a') as f: f.write(json.dumps(dict(mode=mode,pid=os.getpid(),event='natural-end'))+'\\n')
''')
        (destination / "README.md").write_text("# Process contract\n`python process_target.py MODE`: flood exits 0 with 2097152 bytes on EACH stream; fail exits naturally 7; no-ready never signals READY; hang exceeds a 2-second execution limit and its descendant must be cleaned up before it writes descendant-survived.txt after 6 seconds. Preserve all cases and do not confuse forced termination with natural success.\n\n`python check_process.py` must exercise all four cases and write process-results.json, a mapping from each mode to {status, exit_code, stdout_path, stderr_path}. Paths identify preserved raw stream files. Status values are exited, readiness-timeout, timeout. Only natural exits have an integer exit_code; use null for forced termination. Preserve process-audit.jsonl written by the target. The acceptance check runs this entrypoint again.\n")
        (destination / "unrelated.txt").write_text("preserve\n")
    if case_id == "missing":
        (destination / "verification.json").write_text(json.dumps({"command":[str(destination / "unavailable-checker.exe"),"--verify"], "required":True}))
    if case_id == "negative":
        (destination / "README.md").write_text("# Guide\n\nRun verfication; see [details](guide.md).\n")
        (destination / "guide.md").write_text("# Details\n\nLocal documentation.\n")
    mutable = {"built.json", "docs/validation.md"}
    if case_id == "negative":
        mutable.add("README.md")
    # Documentation is an allowed evidence home; application inputs stay fixed.
    documents = {p.relative_to(destination).as_posix(): digest(p) for p in destination.rglob("*.md")
                 if "node_modules" not in p.parts and (p.name == "README.md" or p.relative_to(destination).parts[0] in ("docs", "doc"))}
    immutable = {p.relative_to(destination).as_posix(): digest(p) for p in destination.rglob("*")
                 if p.is_file() and "node_modules" not in p.parts
                 and p.relative_to(destination).as_posix() not in mutable
                 and not (consumer and p.relative_to(destination).as_posix() in documents)}
    if focused_preparation:
        # Even the setup README is an immutable input, not an editable evidence home.
        immutable.update({p.relative_to(destination).as_posix(): digest(p)
                          for p in (destination / "acceptance").iterdir() if p.is_file()})
        documents.pop("acceptance/README.md", None)
    return {"case_id":case_id, "consumer":consumer, "immutable":immutable, "documents":documents,
            "prompt":CONTRACT + "\n" + PROMPTS[case_id]
            + ("\nNative environment prerequisite: read immutable acceptance/README.md and use acceptance/focused-check.ps1. Its automatically allocated private native roots are authorized evidence/fixture locations." if focused_preparation else "")
            + "\nPython interpreter: " + sys.executable,
            **({"focused_preparation": focused_preparation} if focused_preparation else {}),
            "source_state":records[consumer]["tree_sha256"] if consumer else "controlled-v2"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--root", type=Path)
    args = parser.parse_args()
    if args.prepare:
        print(prepare(args.root), flush=True)
    else:
        parser.print_help()
