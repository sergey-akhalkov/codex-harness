## 1. Baseline and acceptance thresholds

- [x] 1.1 Freeze the current source, toolchain, target and heavy budget; capture fresh fat-LTO release phase times, the second-phase compile set, peak memory, queue wait and delivered artifact sizes in local evidence. Verify the inputs are matched and the retained log supports the stated scope.
- [x] 1.2 Capture the fast-profile candidate on the same machine at two and four Cargo jobs for cold, warm-dependency, edited-source and no-op workloads; retain wall time, payload time, queue wait, peak memory, artifact size and startup. Verify no source bytes are changed by the measurement.
- [x] 1.3 Record the predeclared acceptance policy: unchanged protected source reuses without Cargo in at most five seconds; a leaf-package fast rebuild targets at most two minutes, with 20-60 seconds as the stretch goal for a single small binary or measured artifact-level reuse; a cold fast build targets at most six minutes; ordinary release publication retains its accepted profile and checks. Verify every later comparison uses the same thresholds and reports missing evidence rather than substituting a prediction.

## 2. Fast profile and explicit command contract

- [x] 2.1 Add the maintained `delivery-fast` Cargo profile with release optimizations, stripping, fat LTO disabled and the measured parallel codegen setting; verify `cargo build --profile delivery-fast` and the effective compiler flags, and verify the release profile is unchanged.
- [x] 2.2 Add the explicit `deploy --fast` selector, help text, option validation and receipt fields; verify fast and release selectors are mutually coherent, unsupported combinations fail before effects, and no ambient profile/compiler override can select fast mode.
- [x] 2.3 Extend the build-record schema and profile validation so a legacy record without a profile remains release, a fast record is identifiable, and ordinary release selection, preview and fresh-manager delivery cannot select a fast candidate. Verify with both profiles present and with a newer fast build.

## 3. Profile-aware native build and installation plumbing

- [x] 3.1 Thread the selected profile through candidate preparation, handoff, finalization, build identity and dep-info paths; verify a fast candidate uses its own profile output and no hardcoded release path or profile check remains on that route.
- [x] 3.2 Update candidate selection, preview, reuse and core installation to filter by the requested profile; verify fast preview does not select release, release preview does not select fast, and explicit build selection remains authoritative.
- [x] 3.3 Publish fast candidates as immutable builds with the same binary hashes, ownership, recovery and no-rewrite guarantees as release; verify an altered fast candidate is rejected and the prior working installation is preserved.

## 4. Owned fast target and freshness guard

- [x] 4.1 Implement the persistent fast target under an owned short compiler-cache root with a key, ownership record and lease covering toolchain, target, profile, lockfile, manifests, Cargo host configuration and declared build inputs. Verify a foreign or ambiguous target is preserved and an owned target can be retired only after inactivity.
- [x] 4.2 Implement pre-build source comparison and the conservative freshness guard: when a changed compiled input has an mtime that is not newer than the last successful build epoch, use a fresh target; verify changed bytes with a restored timestamp never reuse the old artifact.
- [x] 4.3 Verify ordinary release publication always uses its fresh target and never inherits the fast target's eligibility, even after a successful fast build and an interrupted fast build.

## 5. Source-owned delivery declaration and one Cargo invocation

- [x] 5.1 Add one source-owned delivery-program declaration in the root Cargo metadata and a runtime-list validation that fails on drift; verify fixtures and other test helpers are not selected.
- [x] 5.2 Read the declaration before compilation and build the complete delivery set in one Cargo invocation; verify a source that adds a delivery program is handled without trusting an older producer, and a missing or invalid declaration uses the existing two-phase source-manager fallback.
- [x] 5.3 Compare the one-invocation fast build with the existing two-phase build on the same source and budget; verify the complete binary set, final dep-info coverage, immutable publication and recovery remain identical apart from the declared profile.

## 6. Compiled-input identity and test-only reuse

- [x] 6.1 Extend the build record with the union of actual compiled inputs from every delivered binary's dep-info plus build-affecting identities; verify missing, changed or ambiguous evidence cannot authorize reuse.
- [x] 6.2 Permit fast reuse when every compiled input and build-affecting input matches while only noncompiled test or documentation files changed; verify a new selection record references the same verified binary hashes and reports the current checkout relationship without rewriting the prior build.
- [x] 6.3 Verify a fixture or documentation-like file that is actually compiled invalidates the candidate, an unrelated new file does not by itself force a rebuild, and any lockfile, manifest, toolchain or Cargo configuration change requires compatible new evidence.

## 7. Release concurrency and test feedback

- [x] 7.1 Run a matched fat-LTO comparison of the existing release default and two or four Cargo jobs inside the unchanged heavy budget; adopt a larger release job count only if payload improvement is material, peak memory stays safe and required checks pass.
- [x] 7.2 Measure test compilation and execution separately from delivery. Evaluate scoped serialization or an optional test runner only where shared-resource tests require it, preserving every selected case, ignored-condition and failure report; do not adopt a runner or consolidation without measured benefit.
- [x] 7.3 Record thin LTO as unadopted unless a new matched hypothesis reverses the retained slower-and-larger result; verify the release profile remains fat LTO.

## 8. Documentation, verification and delivery

- [x] 8.1 Update the installation/native-command guides, project decisions, `cargo-fast` and `harness-deploy` skills with the explicit fast/release choice, measured expectations, cache lifecycle, no-deploy-for-test-only guidance and limitations; verify local links and source/publication hygiene.
- [x] 8.2 Run formatting, scoped and full applicable Rust checks, affected native build/identity/installation tests, ownership checks and strict OpenSpec validation; retain actual executed scope and leave unavailable or model-dependent checks open rather than inferring success.
- [x] 8.3 Exercise fast preparation, preview, applying delivery, interruption, recovery and a release deployment with both profile candidates present in an owned isolated installation state; verify profile selection, integrity, installed runtime, executor probe and Serena acceptance.
- [x] 8.4 Deliver the accepted change through the existing global lifecycle and verify ordinary operation outside the checkout, including fast consumption, release selection, unrelated settings preservation and rollback to the verified release path.
- [x] 8.5 Run final acceptance against tasks 1.3: unchanged source, leaf fast rebuild, single-binary stretch case where available, cold fast build, representative runtime/startup and release publication; record measured scope, queue conditions, deviations and unresolved limits without claiming seconds for an arbitrary fat-LTO change.
