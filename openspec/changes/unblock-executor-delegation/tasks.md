## 1. Instruction fixes

- [x] 1.1 `global/principles-of-work.md`: make executor delegation the default for executor-suitable assigned slices; closed set of recorded solo reasons; no instruction wording withholds delegation; profile-fixed routing and verified-failure-only blocking.
- [x] 1.2 `docs/agent-delegation.md`: split selection paths (in-session explicit arguments vs kit profile dispatch); document the `ds`-only executor as full capacity; add the report-don't-block rule to orchestration configuration, utilization and selection sections.
- [x] 1.3 `.agents/skills/team-lead/SKILL.md`: user executor request activates the role; the configured profile is the complete selection; single profile is full capacity; only launcher failures block.
- [x] 1.4 `docs/global-instructions.md`: align activation wording with the user-request rule.

## 2. Records

- [x] 2.1 Record the durable decision (executor = `ds` / DeepSeek V4.1-Flash / `max` only; profile dispatch is the explicit selection; conflicts never block delegation) in `docs/project-decisions.md`.

## 3. Verification

- [x] 3.1 `openspec validate unblock-executor-delegation --strict` passes.
- [x] 3.2 Launcher probe `codex-harness executor --help` prints executor usage and `executor_profiles = ["ds"]` with the DeepSeek V4.1-Flash/`max` binding is confirmed from configuration.
- [x] 3.3 Local links in edited documents resolve; edited public files contain no private paths, tokens or consumer identity.
- [x] 3.4 Fresh instruction loading is checked model-free (global `AGENTS.md` link resolves to the edited principles text).
- [x] 3.5 Grep audit: no remaining instruction text prescribes in-session model/effort arguments or other model preferences for kit executor dispatch, and no text classifies routing conflicts as blockers.
