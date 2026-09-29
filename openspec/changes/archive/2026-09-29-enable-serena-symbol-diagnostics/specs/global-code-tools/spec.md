## MODIFIED Requirements

### Requirement: MCP operations preserve useful source capabilities

Each retained integration SHALL preserve its accepted explicit source
operations through a bounded model-facing surface: suitable Serena
navigation, references, semantic edits and explicit diagnostics; Nuphus
authorized desktop/browser inspection and interaction. The Serena
model-facing tool list SHALL include Serena's symbol-scoped diagnostics
tool alongside file diagnostics while a supported backend delivers it, and
SHALL exclude memory, onboarding, configuration-introspection tools,
`restart_language_server` and `search_for_pattern`; native Git
records remain the authoritative memory route, an explicit escape hatch
SHALL keep an unfiltered debugging view possible, and literal text, regex,
configuration and document search SHALL route to scoped native search
(`rg`). Diagnostic retention SHALL follow the
subscription-efficiency assessment and SHALL distinguish a returned empty
object from authoritative current completion. Tool filtering and
unavailable operations SHALL remain explicit. Representative actual calls,
rather than a tool list, SHALL establish delivered operation scope.
Acceptance mutations SHALL use owned targets. Required exact-reference and
refactoring claims SHALL be checked with current Serena or source evidence.

#### Scenario: Each MCP is exercised by its consumer
- **WHEN** acceptance performs a meaningful read and applicable bounded mutation through each retained MCP
- **THEN** the actual result or owned-target effect and server identity are recorded, with diagnostic uncertainty preserved

#### Scenario: Symbol-scoped diagnostics are delivered
- **WHEN** a fresh managed session edits a symbol and requests diagnostics for that changed symbol, optionally with its direct referencers
- **THEN** the managed Serena catalogue serves the symbol-scoped result with the same freshness uncertainty as file diagnostics, bounded answer limits and no automatic diagnostic activation

#### Scenario: Maintenance moves outside the model session
- **WHEN** any retired integration's host residue needs maintenance or deletion
- **THEN** it is handled outside the kit with explicit user action, and no managed MCP catalogue, registration or kit CLI route is restored

#### Scenario: Filtered Serena surface stays debuggable
- **WHEN** a debugging session needs the unfiltered Serena tool list
- **THEN** an explicit escape hatch restores it for that client without changing the default model-facing surface

#### Scenario: A required MCP operation fails
- **WHEN** discovery succeeds but an operation selected for delivery fails
- **THEN** that operation remains unverified and its original cause is retained; an unrelated passing call does not establish its support

#### Scenario: Index excludes a maintained input
- **WHEN** a needed source, configuration or specification file is unsupported, oversized, ignored or only partly extracted by any indexed or semantic route
- **THEN** coverage reports that limit and the appropriate current semantic or text tool remains usable; a successful exit code does not certify complete source coverage

#### Scenario: Literal text search stays native
- **WHEN** a session needs literal text, regex, configuration or document matches
- **THEN** the managed Serena catalogue offers no `search_for_pattern`, the call is rejected explicitly if attempted, and scoped `rg` remains the documented route
