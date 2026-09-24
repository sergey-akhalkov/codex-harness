## ADDED Requirements

### Requirement: Effective tool selection and guidance agree

The connected consumer SHALL expose the accepted tool set through supported native client/server selection controls wherever they preserve that selection. Tool discovery and server guidance SHALL agree: instructions emitted by the managed connection SHALL NOT require hidden or unavailable tools. Replacing catalogue filtering SHALL preserve semantic operations, diagnostics coverage reporting, per-client project selection, shared-service isolation, resource bounds and installation recovery. A selection setting alone SHALL NOT be treated as proof that multi-client service reuse or process ownership has been preserved.

#### Scenario: Native configuration replaces catalogue filtering
- **WHEN** the managed server connects through the installed configuration
- **THEN** the actual discovered tools match the accepted selection, representative retained operations work, and both initialization and project-activation guidance avoid calling excluded tools

#### Scenario: Two projects share the managed service
- **WHEN** clients from different projects perform retained semantic operations after the selection change
- **THEN** each operation uses its client's project, one client's exit leaves the other's service usable, and the existing resource and recovery guarantees still hold
