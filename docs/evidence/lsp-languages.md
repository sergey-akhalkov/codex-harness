# Language acceptance evidence

Observed on Windows x64, 2026-09-06, with Codex CLI 0.153.4. The required
scope is the fourteen languages below; YAML, QML, HTML and CSS are conditional.
The authoritative installation catalogue is [code-tools.json](../../global/code-tools.json).
An installed binary or a configured registry row is not an operation test.

[lsp-languages.py](../../tests/lsp-languages.py) exercises actual diagnostic,
clearance and document-symbol requests in disposable projects.
[lsp-navigation.py](../../tests/lsp-navigation.py) independently exercises
advertised navigation through the ordinary generated global registry, with
separate project files, imports and build inputs. Its final fourteen-language
run passed after the ordinary global Install refresh. No source edits returned
by rename were applied, and fixture source bytes remained unchanged.

[lsp-evidence.py](../../tests/lsp-evidence.py) validates retained native Codex
events: two actual patches, completed automatic PostToolUse hooks, the real
error report, authoritative empty clearance matching the current source SHA256,
and model-visible feedback. Its sixteen records pass: fourteen required
languages plus the available HTML/CSS backends. These native checks are
separate from direct language-server tests.

## Verified matrix

Every required row produced actual document symbols. “Cross-file” below means
the returned location matched the expected other project file. Versions are
observed executable/package versions, not inferred from directory presence.

| Language | Selected shared backend/runtime | Navigation and project evidence | Automatic error → current empty |
|---|---|---|---|
| Rust | rustup Rust Analyzer 1.97.1, existing compiler cohort | Cross-file definition/references; Cargo.toml and offline lock; hover, workspace symbols, declaration, rename preview, incoming calls | E0308 → clean |
| TypeScript | typescript-language-server 6.0.0 / TypeScript 5.9.3 / existing Node | Cross-file definition/references; tsconfig strict; hover, type/implementation, workspace symbols, rename preview | 2322 → clean |
| JavaScript | Same shared TLS/TypeScript installation | Cross-file definition/references; jsconfig checkJs; hover, type/implementation, workspace symbols, rename preview | 2322 → clean |
| PowerShell | PSES 4.4.0 / PSScriptAnalyzer 1.25.0 / PowerShell 7.6.5 | Dot-sourced function definition/references; hover and workspace symbols | ExpectedValueExpression → clean |
| Python | basedpyright 1.39.10 / existing Node | Cross-file import definition/references; strict pyrightconfig; hover, type/declaration, workspace symbols, rename preview, incoming calls | reportReturnType → clean |
| Delphi | pasls 0.2.0; minimal FPC 3.2.2 win32/i386 prerequisite; installed RAD Studio 2010 SDK | .dproj, unit/include paths and defines; cross-unit definition/three references; actual SDK SysUtils.pas resolution; hover, implementation/declaration, workspace symbols, rename preview | DCC32 E2010, dependent F2063 → clean |
| C++ | Existing clangd 22.1.6 | Header/source definition/references with compile_commands.json and include flags; hover, declaration, workspace symbols, rename preview, incoming calls | init_conversion_failed → clean |
| C# | Roslyn 5.11.0; runtime-only .NET 10.0.11; existing project SDK 8.0.424 | Offline SDK8 project restore; cross-file definition/references; hover, implementation, workspace symbols, rename preview, incoming calls | CS0029 → clean |
| JSON | vscode-langservers-extracted 4.10.0 / existing Node | Actual symbols and local schema validation; definition/references are not advertised | Invalid JSON/schema fixture → clean |
| Markdown | Existing shared VS Code server 4.10.0 + markdown-it 15.0.1 | Heading/link definition and references across files; workspace symbols and rename preview | link.no-such-header-in-own-file → clean |
| TOML | Native Taplo 0.10.0 with LSP | Symbols and local schema fixture; definition/references are not advertised | Invalid TOML / expected value → clean |
| XML | Native LemMinX from vscode-xml 0.29.3; existing PowerShell/.NET XML validator | Definition across included XSDs; document-local XSD references; hover and rename preview; local schema resolution | XMLSchema → clean |
| CMake | Native neocmakelsp 0.11.1 | Included .cmake function definition/references; hover and rename preview; server cache kept outside the project | Documented grammar-error fixture → clean |
| Bash | bash-language-server 5.6.0 + ShellCheck 0.11.0 / existing Node | Sourced function definition/references; hover, workspace symbols and rename preview; analyzed script is not executed | SC1009/SC1072/SC1073 → clean |
| HTML, conditional | Same shared VS Code package 4.10.0 | Actual document symbols; embedded JavaScript/CSS validation | Broken embedded script → clean |
| CSS, conditional | Same shared VS Code package 4.10.0 | Actual document symbols and CSS parser diagnostics | css-propertyvalueexpected → clean |
| YAML, conditional | No ready compatible backend discovered | Explicit unavailable; no mandatory new package installed | Not claimed |
| QML, conditional | No ready compatible backend discovered | Explicit unavailable; no mandatory new package installed | Not claimed |

## Boundaries and corrected counterexamples

- A C++ fixture exposed missing current diagnostics after repeated same-text
  synchronization. The selected clangd's documented `wantDiagnostics` and
  `forceRebuild` extensions now request a current publication for diagnostic
  resynchronization; navigation avoids the forced rebuild. The corrected actual
  probe is clean with the current numeric document version. Earlier pending
  results remain evidence of the defect, not clean acceptance.
- LemMinX XSD references are document-local. Cross-schema definitions pass,
  but cross-schema reference enumeration is not promised. The actual result
  agrees with the upstream [XSD reference implementation](https://github.com/eclipse-lemminx/lemminx/blob/main/org.eclipse.lemminx/src/main/java/org/eclipse/lemminx/extensions/xsd/utils/XSDUtils.java).
- CMake's supported grammar-error fixture is `include( (`. The selected upstream
  server does not report every malformed form, including the tested missing
  closing parenthesis in `set(EXAMPLE`. Its source limitations remain visible.
- HTML validation covers embedded JavaScript/CSS; HTML grammar validation is
  not claimed. Qt translation `.ts` files are distinguished from TypeScript by
  source identification, with routing regression checks.
- Delphi evidence applies to the installed RAD Studio 2010/BDS 7.0 SDK and
  supported project properties. A missing licensed SDK or unsupported project
  condition is reported explicitly. FPC-only compilation is not used as proof
  of Delphi diagnostic compatibility. Discovery queries the actual FPC driver
  for target OS/CPU: dropping win32/i386 from the generated registry caused a
  real navigation failure, and the corrected ordinary global registry passes.
- The Windows Bash adapter repairs the upstream source-path URI conversion in
  a process-local source shim. It preserves source scoping and shared package
  bytes; enabling all workspace symbols was not used to hide resolution errors.
- Extended requests record empty results as `semantic_evidence: false`.
  For example, outgoing-call requests on the leaf function, implementation/type
  queries without a relevant fixture, and hover without documentation do not
  establish those semantic capabilities. Unsupported operations are recorded
  from negotiated capabilities instead of synthesized empty results.
- The native TOML probe's original wrapper report was absent after an overly
  strict multiline-output assertion. The separate evidence checker validated
  its actual patches, hook events, full diagnostic/clearance reports, current
  source hash and model-visible result. It does not claim that wrapper passed.

## Reproduction and retained evidence

Run the existing selected runtimes; these commands do not install dependencies:

```powershell
& $ExistingSerenaPython tests/lsp-navigation.py --registry "$env:CODEX_HOME/harness/lsp-servers.json" --report "$env:TEMP/harness-navigation-global14.json"
& $ExistingSerenaPython tests/lsp-languages.py typescript javascript rust powershell python pascal cpp csharp json markdown toml xml cmake bash html css --registry "$env:CODEX_HOME/harness/lsp-servers.json" --report "$env:TEMP/harness-languages-repeat.json"
```

The final navigation artifact is `%TEMP%/harness-navigation-global14.json`.
Its earlier C++ readiness field is superseded by the corrected actual run
`%TEMP%/harness-navigation-global-cpp-current.json` (current versioned clean).
The native aggregate is `%TEMP%/harness-lsp-native-evidence.json`; each record
names the retained native probe directory and its underlying events and reports.
Earlier failing probe artifacts remain for diagnosis and are not substituted
for the subsequent passing records. The corrected Delphi global navigation
result is included in the fourteen-language aggregate; its earlier native SDK
probe used the equivalent explicit target configuration.

The native hook path and global activation are covered separately by
[native evidence](code-tools-native.md) and [activation evidence](code-tools-activation.md).
