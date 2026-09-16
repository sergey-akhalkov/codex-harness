# Bounded inspection helper

During the kit's Rust migration, an explicitly prepared native candidate also
provides `harness-inspect.exe` with the flags below. Use its verified absolute
path instead of `$pythonExe $runScript`; the oracle remains a caller-selected
command array. It uses Windows Job containment directly, with no PowerShell or
Python prerequisite for the helper itself. It reads the schema from its verified
build's source checkout. For a development binary without a build receipt, pass
`--schema <absolute installed skill asset path>` explicitly. It passes that
source path directly to Codex, records its hash and rejects changes during the
run. Unsupported schema extensions fail before launch; they are never silently
ignored. Global native command registration is still pending, so do not assume
this executable is already on PATH.

`harness-inspect.exe` is the current helper. Pass a JSON array for the launch prefix: either an absolute native Codex `.exe`, or an absolute `pwsh.exe` plus `-NoProfile -File` and the installed launcher path. There is no shell-command string parsing. Resolve the original CLI through the kit installation receipt when PATH points to a wrapper; record its version with `--version` before the run. `scripts/run.py` remains a transitional caller of the same contract.

Example with caller-resolved paths and an already authorized route:

```powershell
$commandJson = ConvertTo-Json -Compress -InputObject @($nativeCodexExe)
$oracleJson = ConvertTo-Json -Compress -InputObject @($pythonExe, $oracleScript)
& $inspectExe --cwd $fixtureRoot --prompt-file $promptFile `
    --schema $schemaPath --command-json $commandJson --oracle-json $oracleJson `
    --model $approvedModel --provider $approvedProvider --subscription $subscriptionLabel `
    --input src/check.py --timeout 180 --output-limit 1048576
```

The inspect executable, schema, launch prefix and oracle command are resolved by the caller, not literal names. A development binary without a build receipt needs `--schema` pointing at the installed skill asset. `--codex-home` optionally selects an already prepared owned consumer home; it never copies authentication. Model/provider/subscription identify the requested route, not proven runtime eligibility. Supply only nonsecret labels; don't put credentials in arguments, prompts or receipts.

The trusted oracle command receives the absolute final JSON path as its last argument and runs with cwd set to the target. Exit 0 means its independent checks passed; exit 1 means a wrong answer; other exit codes or observer failures mean oracle failure. Keep expected answers out of the model prompt. The schema requires `run_id`, `findings` (path, integer line, description, evidence) and `unresolved_issues`. The helper appends its run identity to stdin; the model must echo that identity. Use line 1 or greater and project-relative paths in findings; the oracle checks their semantic validity and completeness.

Each invocation returns `status` and `evidence_root`. Inspect `acceptance.json`, `contract.json`, `process.json`, `events.jsonl`, `stderr.txt`, `final.json` when present, and the linked observer/oracle roots. The contract records Git HEAD/status and content hashes for explicitly selected input files; this is scoped input evidence, not a claim to hash every source. Add all source files relevant to the oracle. Reusing an old `run_id` fails even if the file was copied with a new timestamp. A no-output run never falls back to another output path.

Limits are per captured stream/final file, checked by the existing observer and the stdin bridge while running and again at exit. Polling can overshoot a limit before termination; this is a bounded inspection helper, not a hard disk quota. The process observer owns its child job, including the stdin bridge and Codex descendants. Evidence remains outside the inspected checkout. A cancelled or interrupted caller must not treat surviving partial files as accepted output.
