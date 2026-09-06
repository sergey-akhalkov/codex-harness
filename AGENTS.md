# Repository guidance

This repository is being developed into a reusable global configuration and capability kit for Codex CLI, informed by `opencode-kit`. Optimize for fast delivery of verified, high-quality outcomes across projects.

Everything developed here is intended for use outside this repository by default. A request to add a capability (for example, MCP, LSP, a skill, an agent, or a hook) means delivering and connecting it globally so it is available in Codex sessions across all projects. Completion includes global activation and verification from outside this repository, with reusable sources and setup maintained here through the kit's installation lifecycle. Files or configuration that work only inside this checkout do not satisfy that outcome. Limit delivery to this repository only when the user explicitly requests that scope. See [the confirmed delivery decision](docs/project-decisions.md#глобальная-поставка-по-умолчанию).

Follow [global/principles-of-work.md](global/principles-of-work.md), the canonical portable philosophy. If its full text is already present through global instructions, do not load it again. Otherwise read it before substantive work. Read [docs/project-decisions.md](docs/project-decisions.md) for confirmed project decisions and the user's definition of completion.

Persist durable user goals, constraints, preferences, and decisions as the discussion progresses in `docs/project-decisions.md`. The user has requested this ongoing recording. Label tentative ideas and agent proposals accurately, keep related documents consistent, and remove resolved questions. Keep this file concise by referencing the canonical record.

Start with [docs/README.md](docs/README.md) when researching or changing Codex configuration, instructions, skills, agents, plugins, MCP integrations, hooks, or automation. Follow the task route and read only the relevant sources.

- [docs/best-practices.md](docs/best-practices.md) records official guidance and proposed applications.
- [docs/opencode-kit-map.md](docs/opencode-kit-map.md) records observed source-kit capabilities, candidate mappings, and unresolved design choices.
- [docs/principles-port.md](docs/principles-port.md) explains the completed adaptation of the source philosophy.
- [docs/global-instructions.md](docs/global-instructions.md) records global activation, verification, updates, and rollback.
- [docs/installation.md](docs/installation.md) describes the linked CLI kit and its connection lifecycle.
- `openspec/` holds specification workflow artifacts; `.agents/skills/` currently provides its skills.

Use current official documentation for runtime contracts. Check the installed CLI version and relevant help/schema before relying on a setting. Distinguish CLI, app, SDK, and API capabilities; label version differences and uncertain support.

Treat source-kit behavior and documentation examples as material to evaluate. Keep reusable artifacts project-neutral and load detailed instructions only when relevant. Keep machine-local credentials and runtime state outside tracked reusable configuration.

Validate the actual changed behavior with proportionate checks. Standalone PowerShell checks live under `tests/`: launcher forwarding, installer lifecycle, native consumer discovery and Windows TUI. Read each script's parameters and effects before running it; model-backed agent probes are opt-in. There is no application build or package test runner. For documentation changes, check local links and factual source attribution.

This file guides maintenance of this repository. The portable global principles have their own source under `global/`; other capabilities in the research catalogue retain their documented implementation status.
