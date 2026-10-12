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
