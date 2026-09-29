## 1. Worker configuration

- [x] 1.1 Add the managed optional-tools inclusion (`get_diagnostics_for_symbol`) next to `EXCLUDED_TOOLS` in `crates/harness-core/src/serena_configuration.rs` and render it as `included_optional_tools` in the generated worker home; verify the pinned-render tests cover both lists and `cargo test -p harness-core serena_configuration` passes
- [x] 1.2 Keep the managed connection prompt and rejection behavior unchanged; verify the existing `serena_stdio` broker tests still pass with `cargo test -p harness-core serena_stdio`

## 2. Guidance

- [x] 2.1 Add the symbol-scoped diagnostics route and the protocol sharpening (`replace_in_files` dry-run/occurrence selection, `find_symbol` `include_info`, bounded `substring_matching`, parallel independent reads) to `.agents/skills/token-efficient-workflow/references/code-retrieval.md`; verify each row matches the installed Serena 1.7.0 tool schemas and adds no measured-savings claim
- [x] 2.2 Add one connecting sentence to `docs/code-tools.md` so the everyday-work section names symbol-scoped diagnostics as part of the explicit diagnostic route; verify local links and the stated facts against the rendered configuration

## 3. Lifecycle and acceptance

- [x] 3.1 Run the explicit `install --code-tools-only` lifecycle (preview, then apply) and verify the regenerated `serena-home` worker configuration contains the new `included_optional_tools` entry while all ten exclusions remain
- [x] 3.2 In a fresh managed session, verify the advertised Serena catalogue includes `get_diagnostics_for_symbol` and still rejects `search_for_pattern`, and exercise a real symbol-diagnostics call on the Rust backend with a known diagnostic and its clearance on an owned target
- [x] 3.3 Exercise a real symbol-diagnostics call on the Python basedpyright backend in a disposable owned project, including a known diagnostic and its clearance, and record any backend limit as unverified rather than delivered
- [x] 3.4 Reconcile acceptance results in the owning docs/spec records (no new report protocol) and confirm no token or subscription saving is claimed without measurement
