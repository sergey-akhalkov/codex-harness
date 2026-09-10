# Repository guidance

This repository is becoming `coding-agents-harness-pack`: a public, reusable source of coding-agent configuration, instructions, skills and MCP connections. The currently delivered entry point is Codex CLI on Windows. Keep existing `codex-harness` commands and package identifiers compatible until a separate migration changes them. Public presentation must distinguish delivered support from future plans.

Follow the [portable working principles](global/principles-of-work.md). If their full text is already in global instructions, do not reload it. Otherwise read it before substantive work. Apply their publication boundary to the entire tracked repository, including source, tests, docs and OpenSpec archives; private consumer details and machine state belong outside the shared source.

Keep development convenient: verify the affected language support, actual source navigation and relevant native checks before dependent implementation. Before graph-backed claims, verify the selected provider's current coverage of relied-on sources, including `tools/` and `crates/`; a known-file Serena operation does not require a separate graph build. Follow the portable task-based retrieval budgets. Resolve concrete setup friction and reuse verified setup. Summarized exclusions do not prove that a whole directory's symbols are absent.

Use Rust for new maintained executable code. Existing PowerShell/Python entry points remain transitional until the separate Rust migration preserves their consumers and acceptance. Use PowerShell for shell work. See [native commands and prerequisites](docs/rust-native.md).

Capabilities developed here are globally delivered by default. Adding an MCP, skill, agent or integration includes connecting it through the kit's installation lifecycle and verifying it outside this checkout. Preserve unrelated tools, local settings and recovery. Repository-only delivery requires explicit user scope. See [project decisions](docs/project-decisions.md).

Start with [docs/README.md](docs/README.md) for the relevant configuration, installation or development route. At substantive task start and after context loss, consult [project memory](docs/memory/README.md), then open only relevant owning records. Keep confirmed product decisions concise in [docs/project-decisions.md](docs/project-decisions.md), distinguish tentative proposals, replace superseded facts, and avoid session histories.

Retain only documentation needed for supported operation, future development, recovery, current constraints or unfinished accepted work. Update the owning guide or specification instead of adding another report. Before deleting obsolete material, preserve necessary facts and retarget links. Private evidence is not made public by moving it into an archive or an `internal` directory.

Use OpenSpec for development by default. Current requirements and active tasks remain authoritative; cleanup must not weaken acceptance, replace a required real consumer with a toy fixture, or mark unfinished work complete. Private acceptance inputs are supplied locally, with only general requirements in the pack.

Check current official documentation and the installed CLI help/schema for runtime contracts. Preserve accurate dependency attribution; copied upstream manuals and the history of source-kit research are not maintained product documentation.

Validate actual changed behavior through its entry point with proportionate checks. [Native checks](docs/rust-native.md) and the transitional tests cover launcher forwarding, installation lifecycle, consumer discovery and Windows TUI. Read a check's parameters and effects before execution; model-backed probes are opt-in. Documentation changes need applicable source hygiene, local-link and factual checks. A clean working tree audit makes no claim about older Git commits or previously published copies.
