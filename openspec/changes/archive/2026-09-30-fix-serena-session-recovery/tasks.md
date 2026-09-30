## 1. Reproduce and recover

- [x] 1.1 Add an owned TCP-reset integration reproducer with real Serena and demonstrate the baseline failure, retaining local evidence.
- [x] 1.2 Implement one bounded safe-read recovery and verify reset, persistent failure, cancellation, and non-replayed mutation behavior.

## 2. Enforce real acceptance

- [x] 2.1 Add a native semantic verification path consumed by delivery; verify navigation, edit readback, missing dependency failure and failed delivery status.
- [x] 2.2 Run real Serena integration automatically in CI with explicit prerequisite setup and ignored-test selection; validate the workflow contract.

## 3. Deliver and document

- [x] 3.1 Complete affected tests, warning-as-error lint, formatting, ownership and source checks; validate OpenSpec artifacts.
- [x] 3.2 Deploy through the immutable lifecycle, verify the installed path outside the checkout, and update the owning operating guide with coverage and limits.
