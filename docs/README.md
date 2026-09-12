# coding-agents-harness-pack documentation

This repository is a public, reusable source of coding-agent configuration.
The currently delivered entry point is Codex CLI on Windows. Compatible
`codex-harness` command and package identifiers remain until a separate
migration changes them.

Start here, then open only the owning guide for the work at hand. Historical
research, closed incident reports and obsolete inventories are not maintained
product documentation. Before relying on a technical command, recheck it against
current source, the installed CLI and the relevant local inputs.

## Maintained routes

| Need | Start here |
| --- | --- |
| Confirmed product decisions and completion rules | [Project decisions](project-decisions.md) |
| Portable working principles | [global/principles-of-work.md](../global/principles-of-work.md) |
| Global instruction connection, update and rollback | [Global instructions](global-instructions.md) |
| Install, update, disconnect and recovery | [Installation](installation.md) |
| MCP, language tools and resource policy | [Code tools](code-tools.md) |
| Subscription routing and Grok | [Subscription models](subscription-models.md) |
| Direct agent selection, visible conversations and model routing | [Agent delegation](agent-delegation.md) |
| RTK exception, Code Mode and task effort | [Token workflow](token-workflow.md) |
| Source/link diagnostics | [Source diagnostics](source-diagnostics.md) |
| Native Rust commands and unfinished cutover | [Rust native](rust-native.md) |
| Current migration map and remaining work | [Rust migration](evidence/rust-migration.md) |
| Project memory index | [docs/memory](memory/README.md) |

Archived [CodeGraph replacement](../openspec/changes/archive/2026-09-11-replace-cbm-with-codegraph/proposal.md)
recorded the accepted bounded automatic refresh and measured retrieval policy.
Active OpenSpec work remains authoritative until its own tasks close:

- [migrate-harness-to-rust](../openspec/changes/migrate-harness-to-rust/proposal.md)
- [improve-installed-tool-workflows](../openspec/changes/improve-installed-tool-workflows/proposal.md)
- [autonomous-skill-evolution](../openspec/changes/autonomous-skill-evolution/proposal.md)
- [accelerate-verified-delivery](../openspec/changes/accelerate-verified-delivery/proposal.md)

Use `openspec list` and the owning tasks for current progress. Publication and
retention requirements live in [public-source-hygiene](../openspec/specs/public-source-hygiene/spec.md).

Cleanup of this documentation must not close, drop or weaken those remaining
requirements. Real-consumer acceptance continues to use a locally selected large
repository; a toy fixture does not replace it. The local path is supplied outside
Git.

## Official contracts

Check the installed CLI version, help and schema before relying on a setting.
Documentation and the binary can diverge.

- [Codex CLI](https://learn.chatgpt.com/docs/codex/cli)
- [Config basics](https://learn.chatgpt.com/docs/config-file/config-basic)
- [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
- [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
- [MCP in Codex](https://learn.chatgpt.com/docs/extend/mcp)
- [Hooks](https://learn.chatgpt.com/docs/hooks)
- [Windows sandbox](https://learn.chatgpt.com/docs/windows/windows-sandbox)
- [Documentation index](https://learn.chatgpt.com/docs/llms.txt)

## Open product questions

- Whether WSL, Linux or macOS support follows the current Windows native delivery
- How to measure later capability gains on comparable tasks; level and spend
  policy is already recorded in [agent delegation](agent-delegation.md)
- Whether two-consumer quantitative speed comparison remains required after
  withdrawing ongoing source-kit support; remaining
  [accelerate-verified-delivery](../openspec/changes/accelerate-verified-delivery/proposal.md)
  benefit tasks stay open
