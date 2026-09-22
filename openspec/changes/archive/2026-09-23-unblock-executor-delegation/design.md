## Context

The failing case was an instruction-level false blocker, not a launcher
defect: `executor spawn` takes `--profile` and no model/effort overrides, the
kit configures `executor_profiles = ["ds"]`, and the `ds` profile already
binds `deepseek-flash` (DeepSeek V4.1-Flash) with `model_reasoning_effort =
"max"`. A session instruction demanding explicit per-assignment model and
effort was read as incompatible with that interface, so the lead continued
solo. The user requires the executor to be only DeepSeek V4.1-Flash at `max`
and requires that this restriction never interfere with delegation.

## Goals / Non-Goals

Goals:

- Resolve the selection-rule conflict in the instructions themselves, at
  every layer a session loads: portable principles (global `AGENTS.md`
  symlink), the delegation guide, the `team-lead` skill and the
  global-instructions guide.
- Make executor delegation the documented default for executor-suitable
  assigned work, with a closed set of recorded reasons for staying solo.
- Record the activation change: a user request to use executors activates the
  lead role for that work without the skill name.

Non-Goals:

- No launcher, CLI or configuration behavior change; `executor spawn` keeps
  profile-only dispatch and `orchestration.toml` keeps `executor_profiles =
  ["ds"]`.
- No change to ordinary in-session subagent selection, which keeps explicit
  per-task model and effort arguments.
- No manufactured delegation quota; utilization discipline stays bounded by
  worthwhile work.

## Decisions

1. **The configured profile is the explicit selection.** Model/effort
   explicitness is a property of the dispatch path: ordinary in-session
   agents take explicit arguments; kit executor dispatch takes the configured
   profile, whose native config already fixes model and effort. This is stated
   as a rule in the principles and the delegation guide, so a higher-priority
   session instruction can no longer be read as requiring per-assignment
   arguments for executor dispatch.
2. **One executor profile is full capacity.** `ds` (DeepSeek V4.1-Flash,
   `max`) being the only executor is normal routing, not a capability ceiling
   that justifies solo work. Substitution stays forbidden.
3. **Conflicts resolve toward dispatch.** Apparent routing conflicts are
   reported as discrepancies while dispatch proceeds; only verified
   launcher/installation-check failures block dispatch and are reported with
   cause and remedy. This closes the observed failure mode where an
   interpretation disabled delegation.
4. **Default-on with a closed exception set.** Delegation is the default for
   executor-suitable slices; solo/sequential requires a recorded concrete
   reason (no suitable slice; capability exceeded with no useful
   decomposition; verified dispatch failure). The previous open-ended
   "coordination cost" escape and "decomposition does not require delegation"
   are removed because they enabled the silent-solo failure mode.
5. **User request activates.** A user request to use executors for the
   current work (including "why are executors unused?") activates the lead
   role for that work; ordinary sessions without such a request still spawn
   nothing.

## Risks / Trade-offs

- Principles size: `global/principles-of-work.md` is 26,481 bytes, already
  above its 24 KiB spec floor before this change. This change keeps additions
  bounded and net-neutral wording where practical but does not attempt the
  full lossless compression here; restoring the floor is recorded as follow-up
  work rather than silently weakening the floor requirement.
- Stronger delegation default could increase coordination overhead for small
  tasks; the closed exception set (small/coupled slices are not
  executor-suitable, overhead-costed direct completion stays in the
  delegation spec) bounds that risk without reopening silent solo work.

## Migration Plan

- Principles and skills are live-linked into installations, so new sessions
  load the corrected text without a reinstall; already-running sessions keep
  their loaded context, and the succession path refreshes an active executor
  when needed.
- No data or configuration migration; the executor profile and pool state are
  unchanged.

## Open Questions

- None blocking; the DeepSeek-only executor binding is a user decision
  recorded in the project decision record.
