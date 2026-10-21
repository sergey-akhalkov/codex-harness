## ADDED Requirements

### Requirement: Conditional CSharp semantic navigation

The managed connection SHALL support C# through an unambiguous reusable Roslyn installation and compatible existing .NET host. Discovery SHALL remain read-only and session startup SHALL use the adopted command without provisioning or updating dependencies. Missing, incompatible and ambiguous installations SHALL remain explicitly unavailable. Project SDK selection, other language selections and unrelated installations SHALL be preserved.

#### Scenario: Existing compatible CSharp installation

- **WHEN** explicit discovery observes the expected Roslyn server, matching metadata and compatible runtime
- **THEN** the normal code-tools lifecycle records its pinned launch command and actual solution navigation can be verified through Serena

#### Scenario: Missing or ambiguous CSharp dependency

- **WHEN** the server, matching metadata or runtime is missing, incompatible or ambiguous
- **THEN** discovery withholds adoption and activation refuses without downloading or selecting an arbitrary installation

#### Scenario: Actual semantic acceptance

- **WHEN** C# support is reported as working
- **THEN** actual project symbol discovery, a symbol body and references have succeeded and explicit diagnostic availability and freshness are reported separately from compilation acceptance

## MODIFIED Requirements

### Requirement: Selected languages

The support matrix SHALL record each candidate language and the accepted
selection separately: retained explicit operations, retained automatic
diagnostics, disabled, unavailable or retired. The accepted selection retains
Python and Rust, and includes C++, C# and Pascal/Delphi as reuse-only conditional rows
when the shared clangd installation, matching Roslyn server and compatible .NET host,
or verified pasls and matching FPC prerequisites are present. TypeScript,
JavaScript, PowerShell, JSON, Markdown,
TOML, XML, CMake, Bash and conditional YAML/QML/HTML/CSS remain evaluation
candidates rather than mandatory LSP installations. Any or all LSP support
SHALL be allowed to remain disabled or be retired when its outcome benefit is
absent or unproven.

Each retained operation SHALL have actual backend evidence and correct project inputs; an extension or installed package alone SHALL NOT count as working support. Ambiguous extensions such as Qt XML `.ts` SHALL use actual content/project identity. Supported definition, references, symbol discovery and other navigation operations SHALL be distinguished from unavailable ones. Automatic diagnostics SHALL obey the strict creation/content-modification-only contract and SHALL NOT become required merely because explicit navigation was retained. Equivalent alternatives SHALL NOT cause redundant installations. Unrelated pre-existing packages SHALL be preserved.

Serena MAY be the shared provider for accepted diagnostic operations as well as
navigation. The matrix SHALL distinguish the exposed interface from its underlying
language server and record the actual provider; using Serena SHALL NOT be reported
as eliminating LSP when its configured backend is LSP. A separate harness adapter
SHALL NOT remain mandatory when the accepted shared provider covers its useful
operations and passes the same freshness, isolation and delivery checks.

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
