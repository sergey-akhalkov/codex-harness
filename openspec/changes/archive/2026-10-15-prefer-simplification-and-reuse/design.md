## Context

See [proposal.md](proposal.md) for motivation. The canonical instruction owner is `global/principles-of-work.md`, connected to global initial instructions through the existing filesystem link. Its current size is 25,038 bytes against a 25,088-byte limit. The existing research requirement already owns source evaluation and bounded investigation. The self-improvement skill already prioritizes removing complexity and retains its more specific acceptance constraints.

This cross-cutting policy needs an explicit distinction between reuse enabled by refactoring and speculative generalization; the [delta specification](specs/global-working-principles/spec.md) defines the required decisions.

## Goals / Non-Goals

**Goals:** Change the existing decision order with one canonical policy owner, make refactoring-enabled reuse explicit, and retain current quality, trust and delivery boundaries within the existing instruction-size budget.

**Non-Goals:** No new skill, executable enforcement, dependency, reusable framework, model-backed benchmark or externally maintained OpenSpec workflow edit. This change does not refactor unrelated product code or promise measured speed, token or success-rate improvements.

## Decisions

1. **Rewrite the existing simplicity/reuse section.** Consolidate its wording instead of appending a second policy or raising the size limit. Update the existing research requirement and the existing engineering-judgment decision section. Individual skills continue to inherit the common policy; copying it into each skill would introduce competing owners.
2. **Require a concrete reuse path.** Assess removing complexity first, then direct reuse and reuse through adaptation/refactoring. A shared helper, module, library or tool is justified by an actual consumer and a smaller total solution. Extraction includes migrating affected consumers and removing superseded duplication; a copied implementation or unused library does not complete reuse.
3. **Keep research bounded and suitability explicit.** Reuse current findings. Compare nearby project capabilities and credible external foundations against actual requirements, including trust, license, compatibility and maintenance. Reject an unsuitable foundation for a specific reason. Neither an unbounded search for universal nonexistence nor mandatory reuse of an oversized dependency serves the goal.
4. **Reuse established guidance.** The authors' [DRY explanation](https://www.artima.com/articles/orthogonality-and-the-dry-principle) treats knowledge, documentation and build systems as requiring authoritative representations. [Google's code-review guidance](https://google.github.io/eng-practices/review/reviewer/looking-for.html#complexity) rejects unnecessary generality and speculative functionality. These primary sources support the existing pack approach; the user's confirmed extension explicitly includes reuse enabled by adaptation and refactoring.
5. **Verify documentation and loading through existing entry points.** Use OpenSpec strict validation, the installed `harness-source-check` for hygiene/links/size, and installed `codex debug prompt-input` from an owned temporary consumer outside the checkout. Compare effective instruction content with the canonical source without printing private prompt material. Manually review the specified counterexamples; textual loading is not evidence of future model compliance or measured benefits.

## Risks / Trade-offs

- Research or refactoring becoming a ritual → stop when decision-relevant evidence suffices; confine changes to the affected outcome and reuse valid findings.
- Forced reuse or weakened acceptance → judge total lifecycle cost, preserve required behavior and quality, and retain the existing external trust policy.
- Stronger instructions exceeding the context budget → consolidate the owning section and check the unchanged byte limit.
- A running session retaining old initial instructions → verify a fresh model-free prompt assembly outside the checkout and state that existing sessions need restarting for refreshed initial context.

## Migration Plan

Create and validate this change, revise the canonical section and decision record, and synchronize only the modified requirement into the existing main specification. Verify the already connected global instructions rather than rebuilding unchanged executables. Keep the change available for review without automatically archiving it. Rollback consists of reverting these scoped text changes through the same canonical source; no installation-state migration is introduced.
