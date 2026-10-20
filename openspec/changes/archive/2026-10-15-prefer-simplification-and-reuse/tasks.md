## 1. Canonical policy and specification

- [x] 1.1 Rewrite the existing simplicity/reuse section in `global/principles-of-work.md` with the required decision order, refactoring-enabled reuse and preserved safeguards; review it against every delta scenario and keep the existing 25,088-byte limit.
- [x] 1.2 Update the existing engineering-judgment section in `docs/project-decisions.md` with the confirmed policy and synchronize the modified requirement into `openspec/specs/global-working-principles/spec.md`; verify the delta matches that requirement and unrelated requirements remain intact.

## 2. Integrated verification and delivery

- [x] 2.1 Run strict OpenSpec change and main-spec validation, `harness-source-check --root <checkout>` and `git diff --check`; resolve failures attributable to these changes and retain concise results with their limits. Passed: change valid, 37 main specs valid, 998 working-tree files with zero findings, no whitespace errors; instruction size 25,067 bytes (25,076 with CRLF), below 25,088. Git history was not inspected.
- [x] 2.2 Verify the global live instruction connection and assemble a fresh model-free `codex debug prompt-input` from an owned temporary directory outside the checkout; confirm the canonical policy is included without truncation, preserve unrelated local state, and report that loading does not prove model behavior or performance gains. Passed: canonical link resolves correctly, exit 0, empty stderr and the entire canonical text present in parsed prompt input. No model was invoked; raw prompt evidence remains in local temporary storage outside the repository.
