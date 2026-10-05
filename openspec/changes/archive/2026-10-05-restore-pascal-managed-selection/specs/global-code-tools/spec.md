## MODIFIED Requirements

### Requirement: Selected languages

The support matrix SHALL record each candidate language and the accepted
selection separately: retained explicit operations, retained automatic
diagnostics, disabled, unavailable or retired. The accepted selection retains
Python and Rust, and includes Pascal/Delphi as a reuse-only conditional row
when the verified shared pasls installation and its matching FPC prerequisites
are present. TypeScript, JavaScript, PowerShell, C++, C#, JSON, Markdown,
TOML, XML, CMake, Bash and conditional YAML/QML/HTML/CSS remain evaluation
candidates rather than mandatory LSP installations. Any or all LSP support
SHALL be allowed to remain disabled or be retired when its outcome benefit is
absent or unproven.

Each retained operation SHALL have actual backend evidence and correct project
inputs; an extension or installed package alone SHALL NOT count as working
support. Ambiguous extensions such as Qt XML `.ts` SHALL use actual
content/project identity. Supported definition, references, symbol discovery
and other navigation operations SHALL be distinguished from unavailable ones.
Automatic diagnostics SHALL obey the strict creation/content-modification-only
contract and SHALL NOT become required merely because explicit navigation was
retained. Equivalent alternatives SHALL NOT cause redundant installations.
Unrelated pre-existing packages SHALL be preserved.

Serena MAY be the shared provider for accepted diagnostic operations as well as
navigation. The matrix SHALL distinguish the exposed interface from its
underlying language server and record the actual provider; using Serena SHALL
NOT be reported as eliminating LSP when its configured backend is LSP. A
separate harness adapter SHALL NOT remain mandatory when the accepted shared
provider covers its useful operations and passes the same freshness, isolation
and delivery checks.

#### Scenario: Shared Serena replaces an overlapping harness provider
- **WHEN** retained operations pass their acceptance checks through the existing Serena service
- **THEN** duplicate managed diagnostic registrations/process ownership may be retired while useful Serena operations and unrelated consumers are preserved

#### Scenario: Explicit navigation is useful but automatic diagnostics are not
- **WHEN** only navigation passes the benefit gate
- **THEN** navigation remains explicitly callable and no automatic diagnostic handler is activated

#### Scenario: No language server is selected
- **WHEN** all LSP candidates are rejected or inconclusive
- **THEN** the matrix reports that outcome honestly and the kit completes with project-native verification rather than compulsory LSP provisioning

#### Scenario: A language is declared delivered
- **WHEN** a retained operation is marked available
- **THEN** its backend, version, inputs, result and limits have representative evidence

#### Scenario: A multi-language project has no Codex-specific setup
- **WHEN** a consumer opens a normally configured project containing retained languages and available dependencies
- **THEN** retained explicit operations use the correct roots and inputs without local kit configuration, while any accepted automatic diagnostics remain restricted to proven supported-file creation/content changes

#### Scenario: The source kit supports an additional language
- **WHEN** discovery encounters a backend outside the accepted language selection
- **THEN** it is not provisioned or updated merely because the source kit supports it, and an unrelated existing installation is preserved

## REMOVED Requirements

### Requirement: Delphi support is verified on Delphi source
**Reason**: A retained Delphi LSP based on the user's Delphi SDK compiler was never delivered through the native harness; the restored managed row is FPC/CodeTools-based navigation, which cannot honestly verify Delphi compiler behavior.
**Migration**: Delphi SDK compilation, project options and diagnostics remain user-owned; managed Pascal navigation is re-selected reuse-only from the verified shared pasls installation under the replacement requirement below.

## ADDED Requirements

### Requirement: Managed Pascal navigation reuses the verified shared pasls installation

Where Pascal/Delphi navigation is retained, it SHALL use the shared pasls
installation that discovery verified - its `pasls.exe`, installer version
record, and matching FPC compiler driver plus source tree - pinned explicitly
for the session. Session startup SHALL NOT download, install or update the
backend or its prerequisites. Discovery SHALL leave the row unavailable when
`pasls.exe` is absent, when the FPC driver/source pairing is missing or ambiguous,
or when the installer version record is absent or not a stable dotted version;
startup SHALL refuse projects without an adopted row or with missing recorded
paths before a worker starts. Discovery SHALL record SHA-256 fingerprints of the
observed executables as observations and state that no independent upstream
per-binary integrity guarantee is available; startup SHALL check that the
recorded paths exist rather than assert arbitrary content integrity. Free
Pascal/CodeTools evidence SHALL NOT be reported as Delphi SDK support or clean
Delphi compilation, and the user's Delphi project options, unit/include paths
and SDK information SHALL NOT be rewritten.

#### Scenario: A Pascal project is served from the shared installation
- **WHEN** a retained Pascal operation is exercised on representative legacy Pascal units with their declared compiler inputs
- **THEN** evidence records the observed pasls version, the observed prerequisite fingerprints with their stated limits, and the FPC/CodeTools limits, and verifies the promised cross-unit navigation through the managed session without provisioning

#### Scenario: Prerequisites are missing or ambiguous, or the version record is unusable
- **WHEN** discovery observes an absent `pasls.exe`, a missing or ambiguous FPC driver/source pairing, or an absent or unusable installer version record
- **THEN** the language is reported unavailable, affected projects are refused before a worker starts, and nothing is downloaded or substituted

#### Scenario: An existing installation changes after discovery
- **WHEN** an observed executable or prerequisite is modified while its recorded paths still exist
- **THEN** startup checks only that the recorded paths exist and does not claim content integrity; the observed fingerprints remain the recorded identity
