## Context

Research on 2026-09-23 over official sources only: the Rust release notes
for 1.88 through 1.98 (github.com/rust-lang/rust RELEASES.md plus per-version
pages), the Rust blog announcements, the Cargo book (index, command surface,
the "Optimizing Build Performance" chapter added in 1.92) and the Rust book
edition-2024 basis. `cargo script` remains nightly-only (`-Zscript`); the
stable single-file experiment path is an in-crate example or test. The local
toolchain is already 1.98.1, so the MSRV bump is declarative, not a build
change.

## Goals / Non-goals

Goals: reusable global guidance for modern Rust and fast Cargo loops;
version-gated feature adoption; a toolchain-bump checklist; kit MSRV on the
current stable family.

Non-goals: repo-wide API migration, nightly features, copied upstream
manuals (official links instead), new build infrastructure, replacing
project-native completion gates.

## Decisions

1. Two skills, not one. Authoring-time feature routing and build/test-time
   workflow tuning trigger at different moments and load independently; each
   keeps one reference file.
2. Hybrid anti-rot content. Each reference is a dated, distilled snapshot
   plus a mandatory re-verification rule against current official release
   notes or std docs, because stabilizations keep arriving.
3. MSRV target 1.98 at minor granularity. The installed stable toolchain is
   1.98.1; patch releases such as 1.96.1/1.97.1 carried miscompilation and
   Cargo CVE fixes, so staying on the current patch is part of the policy.
4. No migration sweep. The working tree carries an unrelated in-flight
   change (`retire-codegraph-sharpen-serena`); broad rewrites would entangle
   it and add no verified value. New APIs enter touched code opportunistically.
5. Verification set: `openspec validate`, `cargo fmt --all -- --check`,
   `cargo check --workspace --locked`, installed `harness-source-check`,
   `ownership-check` against its documented known-open baseline, skill
   identity, and manual link/fact checks. Full test-suite execution belongs
   to the in-flight change's completion; this change is metadata,
   documentation and skill content.
6. Delivery. `update --core-only` reconciles skill registrations only from an
   integrity-verified build matching the current source; the preview refused
   because the tree also carries the unrelated in-flight change. Building or
   deploying now would ship that unfinished work, and the `skills publish`
   primitive is rejected for kit skills because it installs non-canonical
   package copies instead of checkout links. Global activation therefore
   follows the documented deploy once the in-flight change lands or the user
   explicitly chooses to ship the current tree.
7. Solo execution. The work is research-synthesis-heavy with a tiny disjoint
   write set; delegation overhead would exceed the benefit.

## Risks / Trade-offs

Snapshot staleness is mitigated by the re-verification rule. Catalogue growth
is bounded by two narrowly described entries. Consumers must use a
current-stable toolchain, accepted with the MSRV policy decision. Running
`update` on a dirty tree is previewed first and aborted if it would move
binaries unexpectedly.
