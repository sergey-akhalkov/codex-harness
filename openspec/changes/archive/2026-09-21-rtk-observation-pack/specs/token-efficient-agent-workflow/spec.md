## ADDED Requirements

### Requirement: Stable observation handles with integrity digests

When the adapter compresses a supported command and retains its original output, the compact result SHALL also present a stable observation handle and a content digest of the retained bytes in the same compact footer. The existing raw-path locator SHALL remain present and unchanged. Handle creation SHALL be local-only: it MUST NOT start models, make network calls, or alter the command's arguments, working directory, environment, exit status or output ordering. Bypass paths that today return raw or compact output without retention (unsupported command, filter failure, oversize output, explicit disable, terminal stdout) MUST NOT present a handle.

#### Scenario: Compressed run presents handle and digest

- **WHEN** a supported command runs through the adapter's compact mode and its original output is retained
- **THEN** the compact footer names a stable observation handle and a digest of the retained bytes next to the existing raw-path locator

#### Scenario: Bypass paths emit no handle

- **WHEN** a command bypasses compression or retention through an explicit disable, unsupported form, filter failure or oversize output
- **THEN** the result stays byte-for-byte on the existing raw or compact path and contains no observation handle

### Requirement: Digest-verified paged recall

The adapter SHALL provide a recall operation that returns an exact, bounded line window of a packed observation without rerunning the source command. Recall MUST verify the retained content against the recorded digest before emitting any content and MUST fail closed with an integrity error when verification fails. Recall output SHALL include a compact provenance header (handle, source command, digest status, total lines and bytes) and stable line coordinates for the returned slice. Default and maximum slice sizes SHALL be bounded and explicit; oversized requests SHALL be clamped or rejected, never silently streamed. Unknown or evicted handles SHALL return an explicit not-found error naming the remedy (rerun the command or use the raw path) instead of fabricated or partial content.

#### Scenario: Recall returns an exact line window

- **WHEN** an agent needs specific lines of a previously packed observation
- **THEN** recall verifies the digest and returns only the requested bounded window with provenance and line coordinates, without executing the source command again

#### Scenario: Corrupted archive fails closed

- **WHEN** retained content no longer matches the recorded digest
- **THEN** recall emits no observation content and reports an integrity failure with the handle and remedy

#### Scenario: Evicted or unknown handle is explicit

- **WHEN** recall is asked for a handle outside the retention window or never issued
- **THEN** it returns an explicit not-found error with the remedy and no content

### Requirement: Bounded pack retention outliving the session

Packed observations SHALL be stored only in the kit's local RTK storage area, separate from the existing raw-file keep window, with a bounded retention that is not tied to session end. Eviction SHALL remove the oldest packed observations first and the retention limit SHALL be documented and observable. No packed observation content SHALL be uploaded, sent to a model, or deleted merely because the creating session ended.

#### Scenario: Retention evicts oldest first

- **WHEN** packing exceeds its documented retention limit
- **THEN** the oldest packed observations are evicted and later recall of an evicted handle returns the explicit not-found error

#### Scenario: Session end preserves packed observations

- **WHEN** the session that created a packed observation ends while it is inside the retention limit
- **THEN** the packed observation remains locally available for recall by a later session

### Requirement: Workflow prefers paged recall for detail recovery

The token-efficient workflow guidance SHALL direct agents to recover needed detail from packed observations through bounded recall windows before whole-file raw reads or command reruns, while preserving the raw path when complete content is genuinely required. The guidance MUST NOT use recall to hide errors, weaken acceptance checks, or suppress evidence, and adoption claims SHALL follow the existing scoped efficiency reporting: measured bytes and avoided reruns only, with no session or weekly-quota savings claim.

#### Scenario: Bulky compressed output needs a few lines

- **WHEN** a decision depends on specific lines of a previously packed bulky observation
- **THEN** the agent recalls that bounded window instead of re-emitting the whole archive or rerunning the command

#### Scenario: Benefit evidence stays scoped

- **WHEN** the packed-recall route is compared with the previous whole-file path
- **THEN** the recorded benefit distinguishes measured output bytes and avoided reruns from unmeasured token or quota effects