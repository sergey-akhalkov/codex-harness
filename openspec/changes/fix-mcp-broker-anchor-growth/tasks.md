# Tasks

- [x] 1.1 Reproduce and pin the defect: retain the observed startup failures
       (`CodeGraph account record exceeds its bound`,
       `Serena broker location record exceeds its bound`) with the actual
       installed entrypoints and anchor sizes as the failing baseline.
- [x] 1.2 Share a fixed anchor read bound in `broker_state` and use it for the
       Serena broker, CodeGraph account and read-only CodeGraph location
       lookups.
- [x] 1.3 Make generation selection bounded: drop vanished locations, prune
       entries whose broker instance is free, preserve live older generations,
       reuse a free location (legacy root first) before preparing a new one,
       and persist any retained-set change under the existing admission and
       guarded-replacement path.
- [x] 2.1 Add unignored regression tests: a grown CodeGraph account record and
       a grown Serena broker record each resolve a location for a new source
       and are rewritten pruned and below the previous bound; both must fail on
       the pre-fix baseline.
- [x] 2.2 Add unignored chooser tests: dead-root reuse without preparing a new
       root, live-generation preservation, bounded retained records across
       successive sources.
- [x] 2.3 Extend adopted-package MCP acceptance with a pre-grown anchor: the
       published-package CodeGraph entrypoint and the real Serena proxy must
       complete initialize and tools/list after historical growth.
- [ ] 3.1 Run the applicable native checks (affected unit tests and focused
       MCP integration tests), then the installed-path verification: rebuild,
       reconnect the installation and confirm both previously failing MCP
       registrations complete a real handshake after the fix.
- [x] 3.2 Update the owning documentation for the bounded generation-record
       behavior and record the release-verification commands, keeping evidence
       in existing owners.
