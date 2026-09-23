## 1. Research

- [x] Collect 1.88-1.98 language, standard-library and Cargo stabilizations
      from official release notes
- [x] Confirm `cargo script` status (nightly-only) and stable experiment
      alternatives
- [x] Review the Cargo book build-performance guidance and the Rust book
      edition-2024 basis

## 2. Artifacts and skills

- [x] Create change artifacts (proposal, design, spec delta, tasks)
- [x] Add `.agents/skills/rust-modern` with a dated feature reference
- [x] Add `.agents/skills/cargo-fast` with a workflow reference

## 3. MSRV and documentation

- [x] Raise the workspace `rust-version` to 1.98 and update
      `docs/rust-native.md`
- [x] Record the current-stable MSRV policy in `docs/project-decisions.md`

## 4. Verification and delivery

- [x] `openspec validate` for the change (valid, 4/4 artifacts)
- [x] `cargo check --workspace --locked` through the heavy queue (passed);
      `cargo fmt --all -- --check` reports only files owned by the in-flight
      `retire-codegraph-sharpen-serena` change
- [x] `harness-source-check --root .` and `ownership-check` (ownership clean;
      the single source-check finding is the in-flight
      `global/principles-of-work.md` size limit, unrelated to this change)
- [x] Reconcile skill registration via the core lifecycle: the update preview
       refuses because no integrity-verified build matches the current source,
       and building now would ship the unrelated in-flight change. Follow-up
       once that change lands (or by explicit user choice to ship this tree):
       `codex-harness deploy --source <ABSOLUTE-KIT-CHECKOUT>`
       — the blocking change landed and the whole tree (skills, MSRV 1.98,
       docs) was delivered twice through `deploy --source <checkout> --all`
       (`eeed235…`, then `9da1919…`), both with `executor_probe_ok: true`;
       the installed build matches this source and the current session
       already resolves `cargo-fast` and `rust-modern` from the kit skills.
- [x] Report deploy deferral with its exact follow-up action
