# Nuphus original reuse and owned desktop/browser operations

Observed on 2026-09-06 using the existing npm `@nuphus/nuphus-mcp` **0.2.2**, the existing Windows x64 companion package, and existing Microsoft Edge **152.0.4191.66**. No upstream package was installed, updated or overwritten by this probe.

## Original and local variant

The installed Node wrapper selects `nuphus-mcp-schema-fixed.exe` ahead of the preserved original executable. Its wrapper differs from the official npm wrapper. Targeted source-kit searches did not locate the patch's provenance.

Both existing native executables completed real MCP initialization and returned **38 tools**. Comparing their entire `tools/list` responses showed exactly six additions: `type: "object"` in the two `inputSchema.anyOf` branches of `browser_click`, `browser_type` and `browser_drag_files`. No other published schema/description/tool field differed. This comparison does not establish that all binary behavior is equivalent.

A separate integrity audit compared all **five files** packaged in the official Windows companion 0.2.2 tarball with the installed originals; all matched, including the original executable and DLLs. The tarball's published SHA-512 integrity was checked. Original executable SHA-256:

```text
9a07112f17a964d9c0b1a54653af95559d7de33cce1cb3dffd60dfc4c85ccfb0
```

The [repository proxy](../../tools/code-tools/nuphus_proxy.py) therefore runs that audited original, checks its hash at startup, and makes only those six schema additions. The locally patched binary and modified Node wrapper are preserved. A changed executable/version requires a new compatibility and integrity review. Package metadata: [official npm companion 0.2.2](https://registry.npmjs.org/@nuphus%2fnuphus-mcp-win32-x64/0.2.2).

## Runtime contract and ownership

All 38 tools remain exposed. The proxy always sets native `NUPHUS_MCP_NO_MODEL_DOWNLOAD=1`; upstream documents and implements this as skipping OCR and optional YOLO downloads. Missing prerequisites produce the native explicit error. See [official model setup](https://github.com/mrpulor-gh/nuphus-mcp/blob/master/TOOLS.md#desktop_perceive--local-ocr--yolo-models) and [upstream models.rs](https://github.com/mrpulor-gh/nuphus-mcp/blob/master/crates/nuphus-mcp/src/models.rs).

An explicitly configured `NUPHUS_MCP_BROWSER_CDP_URL` is preserved. Otherwise the first browser tool call lazily launches the existing Chrome/Edge executable headlessly with a process-owned profile and private loopback CDP endpoint. Startup and tools/list do not launch a browser. The announced websocket identity is checked before attaching; failure cannot fall back to a shared browser. Native browser discovery uses installed executables, not browser downloads. See [upstream browser client](https://github.com/mrpulor-gh/nuphus-mcp/blob/master/crates/nuphus-browser/src/client.rs) and [browser discovery](https://github.com/mrpulor-gh/nuphus-mcp/blob/master/crates/nuphus-browser/src/chrome_finder.rs).

The first test found that Windows Edge re-executed after de-elevation, detaching from the launch handle. Passing `--do-not-de-elevate` preserves ownership and stderr; the corrected launch/cleanup was exercised. The two earlier owned test browser trees were identified by their unique TEMP profile and exact PID, then terminated. A subsequent process inspection found no remaining browser process belonging to these probes.

A separate [real browser isolation check](../../tests/fixtures/code-tools-native/nuphus-browser-isolation.py) passed concurrent first-call reuse, distinct profile/process/endpoint ownership, removal of an owned directory link while preserving its foreign sentinel target, and shutdown of one browser while the second still answered CDP. Both owned profiles were removed. Cleanup checks each lexical and resolved target against the exact owned directory; a changed parent or locked file produces a cleanup-pending message instead of traversing or silently hiding it.

Upstream still computes a shared Nuphus downloads location even for an external CDP connection. This change isolates the managed browser profile and process; it does not claim to isolate every native Nuphus download listing across sessions.

## Actual operation evidence

```powershell
./tests/mcp-nuphus.Tests.ps1 -Registry "$env:TEMP/harness-discovery.json"
```

The [standalone test](../../tests/mcp-nuphus.Tests.ps1) uses the [real operation fixture](../../tests/fixtures/code-tools-native/nuphus-operations.py). **12 assertions passed** in the final recorded run, including the tightened cleanup guards:

- All 38 original tools exposed, three repaired schemas, no eager browser startup.
- Native desktop info reads an owned off-screen Windows Forms window by exact handle; native resize is verified through a second info read. No user window is selected or activated.
- Native browser snapshot reads a locally served owned page; `browser_type` plus `browser_click` changes its DOM and native evaluate confirms the expected value.
- Native screenshot produces the owned page image; it was visually inspected.
- Native local OCR returns four text elements with center coordinates using the existing ONNX models and explicitly provisioned dictionary.
- Closing the proxy shuts down its owned browser and removes its private profile.

The test enables `NUPHUS_MCP_ALLOW_PRIVATE_NAV=1` **only in its own process** because the native tool rejects private/loopback navigation by default. The native tool also rejects `file://`, so the fixture serves one page on loopback. The proxy preserves the user's upstream navigation policy.

The first actual `desktop_perceive` call fast-failed without downloading because `ch_PP-OCR_keys_v1.txt` was absent from `%APPDATA%/Nuphus/models`. The dependency lifecycle then explicitly provisioned that dictionary from upstream PaddleOCR, preserving the existing detector/recognizer models. Both a focused repeat and the final full operation test succeeded with downloads still disabled. OCR text is approximate; no desktop click was based on its output. Optional YOLO remains unavailable (`yolo_available: false`) because `icon_detect.onnx` is not installed; the result reports OCR-only operation explicitly.

Private artifacts for the final successful run are under TEMP `harness-nuphus-operations-4693c1f2e89b487b8c3d1e971c807143`; earlier bounded troubleshooting runs and schema comparison are also retained there under uniquely named `harness-nuphus-*` directories. They contain only owned fixtures/schema data. No credentials, user cookies or user desktop screenshot was captured. Separate Serena cleanup denials remain recorded in [its evidence](mcp-serena.md); no alternative deletion route was attempted.

This proves direct repository-source MCP behavior. It does not by itself prove global native Codex activation, every browser operation, optional YOLO detection, or every consumer surface.
