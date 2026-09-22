## 1. Executor launch configuration

- [x] 1.1 `orchestration_config`: expose the executor session marker and `executor_session_args` (profile flags plus `-c agents.enabled=false`).
- [x] 1.2 `executor_cli`: use the executor session arguments for `exec` and `tui` child args and for resume; mark the owned console, tab host and successor environments.
- [x] 1.3 `task_succession`: the successor plan uses the same executor session arguments.
- [x] 1.4 `launcher`/`native_launcher`: add the agent-capability-off configuration to any forward whose environment carries the executor marker.
- [x] 1.5 `executor_cli`: refuse `spawn`, `resume`, `run` and `succeed` inside an executor session with an actionable error naming the lead.

## 2. Checks

- [x] 2.1 Unit checks: argument oracles for exec, tui, resume and succession; launcher marker injection; dispatch refusal; executor environment marker.
- [x] 2.2 Offline native probe on the installed CLI: the provider request of an executor-profile `codex exec` with `-c agents.enabled=false` contains no collaboration/agent tools, while the same request without it does.
- [x] 2.3 `cargo fmt --all -- --check`, targeted clippy and crate tests pass.
- [x] 2.4 `openspec validate deny-executor-agent-spawning --strict` passes.

## 3. Docs and delivery

- [x] 3.1 `docs/agent-delegation.md` and `.agents/skills/team-lead/SKILL.md`: executors run single-agent, nested dispatch is refused, helpers stay lead-only.
- [x] 3.2 Update the installed harness from this checkout and verify the refusal and launcher behavior outside the checkout.
