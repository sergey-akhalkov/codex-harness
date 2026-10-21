## 1. Verified preview reuse through the existing owner

- [x] 1.1 Reuse eligible verified existing build selection for implicit-build deployment previews through the existing owner; retain explicit selection and the ordinary preparation/error path when reuse is unavailable.
- [x] 1.2 Report the selected build and requested source relationship without implying fresh compilation or installation; preserve component planning, reset preview and applying source/ownership gates.
- [x] 1.3 Exercise the real native preview with owned isolated installation state and a compilation marker, including explicit, missing, altered and older-build cases and unchanged installation state; run affected native deployment checks, formatting and scoped clippy.

## 2. Guidance, integration and delivery

- [x] 2.1 Update the authoritative installation guidance and `harness-deploy` procedure for the verified reuse behavior and its first-use/applying limitations; pass source and local-link checks.
- [x] 2.2 Recover applicable retained implementation, integrate it against current source and verify the native scenarios from 1.3, formatting, scoped clippy, source/local-link hygiene and strict OpenSpec validation. Preserve historical evidence and implement only remaining gaps.
- [x] 2.3 Deliver through the existing immutable-build installation lifecycle and verify actual installed preview consumption outside the checkout, with unchanged installation state. Record the bounded behavior and current acceptance in the existing owner without claiming a measured end-to-end efficiency benefit.

The user decision of 2026-10-21 replaces the mandatory A-on-B comparison and benefit gate for this change with direct behavior acceptance and ordinary delivery. Tasks 1.1–2.1 record the prior experimental work; 2.2 verifies its current integration. Other workload and parent-loop requirements remain unchanged.

Current integration acceptance: 39 passing native tests across build selection,
installation state, manager delivery, core preview and install CLI; formatting,
source/local-link audit, skill validation and strict change validation pass.
Scoped Clippy completes with existing warnings confined to unchanged modules;
the stricter warning-as-error invocation remains blocked by those baseline lints.
The retained experimental patch was not available in the inspected worktrees and
records; the integrated implementation reuses the current native selection owner.
Installed acceptance: the delivery receipt reports `deployed`, a passing executor
usage probe and `semantic-ready` Serena. Both ordinary preview and combined
`--all --reset --preview`, invoked outside the checkout, consume the delivered
build with `verified-existing`, never start the Cargo marker and preserve all
49 compared installation entries. All four component plans succeed.
