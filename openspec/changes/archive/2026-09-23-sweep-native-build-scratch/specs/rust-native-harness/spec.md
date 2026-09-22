## ADDED Requirements

### Requirement: Native build scratch reclamation

The explicit native build lifecycle SHALL reclaim its own abandoned
process-temp scratch. Before compiling a candidate, the manager SHALL remove
direct children of the process temp root whose names use the harness scratch
prefixes (`hcb-`, `hcc-`, `hca-`) when they are ordinary directories whose
last write is older than the documented sweep age. The sweep SHALL skip
reparse points and never follow them, SHALL NOT touch entries outside those
prefixes, and removal failures SHALL NOT fail the build.

#### Scenario: Abandoned scratch is reclaimed
- **WHEN** a previous interrupted explicit management operation left
  `hcb-`/`hcc-`/`hca-` prefixed scratch directories older than the sweep age
  in the process temp root
- **THEN** the next explicit native build removes them before compiling its
  candidate

#### Scenario: Fresh and foreign entries survive
- **WHEN** the process temp root also contains a fresh prefixed scratch
  directory, a reparse point or unrelated directories
- **THEN** only stale prefixed ordinary directories are removed and every
  other entry remains
