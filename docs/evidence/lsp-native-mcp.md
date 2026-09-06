# Native global Serena edits and automatic diagnostics

Observed on 2026-09-06 with native Codex CLI 0.153.4 app-server and the existing
global registrations. [lsp-native-mcp.Tests.ps1](../../tests/lsp-native-mcp.Tests.ps1)
passed **14 assertions**, followed by a separate process-identity check showing
no remaining owned MCP descendants after native shutdown.

The real model activated an owned TypeScript project outside this checkout,
whose path contained spaces and Cyrillic characters. It used exactly two
successful Serena replace_in_files calls: number 1 to string "wrong", then to
number 2. Both original MCP results remained successful and reported one
replacement in index.ts. There were no shell/native-patch edits, manual
diagnostic calls or reads of diagnostic report files.

Native PostToolUse delivered code **2322** for revision prefix **352061291060**,
then an authoritative empty diagnostic set with status **clean** for revision
prefix **072d0f94863f**. The model quoted both prefixes from the automatic
feedback. Neither hash appeared in its prompt, which distinguishes received
hook feedback from an inferred TypeScript error.

The test read only its native session's diagnostic directory. Existing Serena
configuration, MCP registration metadata, dependency registry and hook source
retained exact bytes. Native thread/start added only its own directory-trust
entry; every pre-existing TOML setting was preserved. No additional MCP
registration or private substitute Serena server was used.

The first actual attempt exposed UTF-8 hook stdin decoded as Windows OEM866,
which corrupted the Cyrillic workspace before baseline recording. The model
correctly reported diagnostics unavailable. [hook.ps1](../../tools/hook.ps1)
now selects UTF-8 before its first console read and explicitly sets Python I/O
encoding. [hook-encoding.Tests.ps1](../../tests/hook-encoding.Tests.ps1) passed
three additional actual checks under a deliberately initial OEM866 console:
baseline, TypeScript error, and clearance in the Unicode workspace.

Private native evidence remains in the machine's temporary directory:
lsp-native-mcp-56d60cdc5f074f6d8842bf476661d6d7. Its native thread is
01a077f0-4ecc-7e81-b6d8-4a5f9cadfbb9. This final rerun passed after the native
startup-readiness registration and all six diagnostic boundary corrections,
including warm dependency buffers. The earlier successful run remains at
lsp-native-mcp-22a2eeadadbb4b08b337b32918d5c94a. The failed Unicode counterexample is retained
separately as lsp-native-mcp-c733aa1cde6e4f2caeb4a5414eb3fcf0.

    pwsh -NoLogo -NoProfile -File tests/hook-encoding.Tests.ps1
    pwsh -NoLogo -NoProfile -File tests/lsp-native-mcp.Tests.ps1 -RunAgent -KeepProbe
