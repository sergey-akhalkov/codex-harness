# Serena: existing installation, runtime guard and two projects

Observed on 2026-09-06 with Serena **1.7.0**, its existing uv Python environment and the existing TypeScript language server cache. No package installation, update, global MCP registration or model request was part of this probe.

## Source and executable checks

[serena_entry.py](../../tools/code-tools/serena_entry.py) runs the installed Serena CLI and Codex context. It reads the discovered dependency registry and preserves the existing shared Serena configuration. No upstream files are patched. The compatibility seam is deliberately pinned to the source version that was audited; another Serena version must be reviewed and verified before activation.

The installed `solidlsp/dependency_provider.py` supports `ls_base_cmd`, `ls_path` and `ls_args`; using these bypasses its managed installation. The guard supplies existing commands for TypeScript, Rust and Python. Existing project semantic settings remain available. Unmapped backend constructors and missing or unverified dependency records are refused before construction.

Two legacy backends require additional treatment:

- Installed `powershell_language_server.py` otherwise downloads PSES and invokes `Save-Module` for a missing analyzer. The guarded setup requires existing PowerShell, PSES and PSScriptAnalyzer. Its fixed temporary log/session filenames are placed under an owned `{CODEX_HOME}/harness/runtime/serena/{pid_guid}` directory, with exit cleanup that does not traverse directory links. Forced termination can leave this owned directory for lifecycle recovery.
- Installed `pascal_server.py` otherwise performs release checks and can replace an existing Pascal executable during activation. Guarded setup returns the registry's existing executable without invoking that routine.

`RuntimeDependencyCollection.install` is also refused before it can create its destination or run package commands. These are process-local compatibility overrides; the installed OpenCode/Serena binaries and package sources remain unchanged. This is not a general sandbox for arbitrary project code or a claim that every Serena backend has been audited.

## Reproduction and results

```powershell
./tests/mcp-serena.Tests.ps1 -Registry "$env:TEMP/harness-discovery.json"
```

The registry is machine-local discovery output; it is not a reusable repository configuration. The [test](../../tests/mcp-serena.Tests.ps1) starts two real stdio Serena servers directly through the repository entry point in disposable project directories, including spaces and Cyrillic characters. It does not use a mock MCP server.

**28 assertions passed** in the recorded run:

- Real MCP initialization and native Codex tool selection.
- Document symbols, reference search, symbol body reads and explicitly empty initial diagnostics in both projects.
- Same symbol name with different bodies stays associated with the correct root.
- Real `replace_symbol_body` introduces TypeScript error 2322 in each root; correction produces explicitly empty diagnostics in each root.
- The other concurrent root stays clean and its body unchanged; shutting down one server preserves the other.
- Shared `serena_config.yml` and adopted entrypoint fingerprints are unchanged.
- Installed upstream provisioning seams reject collection installation, an unmapped constructor, a missing Pascal executable and a missing analyzer before destination creation. See [guard checks](../../tests/fixtures/code-tools-native/serena-guard-check.py).

The recorded 28-assertion run preceded the narrow PowerShell temp-directory isolation addition. That addition is separately exercised by the guard check's two-process temporary-path and cleanup assertions. The two-project semantic proof uses TypeScript; it does not establish PowerShell or Delphi language correctness, every-file package integrity, global activation, native automatic-hook delivery, or Desktop/IDE discovery. Those remain separate acceptance checks.

The default test closes its own processes and removes its owned disposable root. Two explicitly retained troubleshooting roots remain under the machine's TEMP directory: `harness-serena-6e484fb7015e44ce9c3a458812e94fe2` and `harness-serena-guard-eccb5e9c5e0345c3ab13a65f4bd125ff`. Automatic approval review rejected their path-validated PowerShell cleanup with the reason `blocked by policy`; no alternative deletion route was attempted. Their test processes have exited. Runtime logs created normally by upstream Serena are outside the test root; package resources and the shared user configuration are preserved.
