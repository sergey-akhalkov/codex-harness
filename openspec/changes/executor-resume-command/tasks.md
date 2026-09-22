## 1. Pool adoption

- [x] 1.1 `task_worktree`: add `adopt_slot` (same-source record, existing
      directory, recorded base, no-owner-or-same-owner rule, occupied rewrite,
      no reset) and expose the upstream lookup for the resume binding.
- [x] 1.2 `executor_cli`: `executor resume` parses exact session/slot/owner,
      refuses a live owner, adopts the slot, reuses the spawn launch path and
      records the lease.

## 2. Diagnosability

- [x] 2.1 `record_lease` refusal for an unowned slot record names the pooled
      spawn/resume remedy.

## 3. Checks

- [x] 3.1 Unit checks: resume child arguments, adoption rules and the
      actionable refusal message.
- [x] 3.2 Integration check: after an interrupted dispatch, resume rebinds the
      same slot, preserves partial work without reset, is repeatable for the
      same owner and refuses another owner.
- [x] 3.3 `cargo fmt --all -- --check`, targeted clippy and crate tests pass.
- [x] 3.4 `openspec validate executor-resume-command --strict` passes.

## 4. Docs and delivery

- [x] 4.1 Update `docs/agent-delegation.md` and the live team-lead skill to
      route interrupted executors through `executor resume`.
- [ ] 4.2 Update the installed harness from this checkout and verify the
      subcommand is available outside the checkout.
