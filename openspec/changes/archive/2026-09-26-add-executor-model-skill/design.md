## Context

Executor routing is already profile-based: `global/orchestration.toml` names an
installed Codex profile, and that profile owns the model, provider and effort.
Before this change the kit and one consumer pinned `xai`, the switch procedure
lived only in conversation history, and kit tests/docs still described that old
binding after the configuration moved to `ds`.

## Goals / Non-Goals

**Goals:**

- Make an executor-model switch one repeatable kit-owned action that validates
  the selected installed profile, changes only `executor_profiles`, prints the
  resolved binding and reads every named checkout back.
- Keep reusable skill source in this repository and deliver it through the
  existing kit link lifecycle without adding an executable.
- Keep the committed kit tests and current-routing guidance aligned with the
  configured `ds` executor profile.

**Non-Goals:**

- No change to lead or successor profiles, provider authentication, model
  catalogues, per-assignment routing arguments or executor lifecycle behavior.
- No generic model-name parser or new runtime dependency.

## Decisions

- Switch by profile id, not model slug. A profile already binds model,
  provider and effort as one native configuration; rewriting those fields or
  inferring a profile from a model name would create a second routing source.
  The skill helps identify a matching installed profile when a user names a
  model, but the committed switch remains profile-id based.
- Ship the procedure as a kit-owned `SKILL.md` with one canonical PowerShell
  block and no executable. This preserves the Rust executable-ownership
  boundary and lets the existing skill-link lifecycle deliver the same source
  to user homes. A helper binary or machine-local script would add lifecycle
  and ownership without new capability.
- Read configuration live. The installed launcher resolves the checkout's
  orchestration file at dispatch, so an ordinary switch needs no build or
  reinstall; deployment is only needed when kit-owned skill/source files change.
- Treat tests and current-routing statements as part of the accepted switch
  when the user authorizes the full change. They are synchronized explicitly
  rather than edited silently by the one-action profile switch.

## Risks / Trade-offs

- [Profile absent or target has no routing line] -> The canonical block refuses
  before or at that target, names installed profiles/missing input, and leaves
  unrelated routing fields untouched.
- [Checkout tests or current-state docs pin the old id] -> The skill reports
  that follow-up instead of silently broadening its edit; this change performs
  the authorized kit synchronization.
- [A future switch changes only configuration] -> Executor dispatch follows it
  live, while any checkout-specific tests/docs remain that checkout's explicit
  follow-up.

## Migration Plan

Set the kit and named consumer `executor_profiles` line to the validated
profile, deliver the kit-owned skill through links, and synchronize authorized
tests/current guidance. Switching back is the same one-line profile change; no
data migration or conversation state is involved.
