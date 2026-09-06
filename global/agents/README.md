# Personal agents

Put repository-owned standalone Codex agent definitions (`*.toml`) here. The
installer links this whole directory under the global `agents/codex-harness`
namespace, so additions and edits are read on subsequent Codex launches without
reinstalling. This kit currently ships no production agent roles.

Use a unique `name`, a `description`, and self-contained `developer_instructions`.
Keep machine-specific paths and secrets outside these files. Required supporting
resources must belong to the repository or a documented external dependency.

Contract: [Custom agents](https://learn.chatgpt.com/docs/agent-configuration/subagents#custom-agents).
