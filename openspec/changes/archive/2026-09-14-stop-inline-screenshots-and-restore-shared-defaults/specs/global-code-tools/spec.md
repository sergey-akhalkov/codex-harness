## ADDED Requirements

### Requirement: Bounded visual desktop captures

Nuphus desktop and window screenshot operations SHALL deliver a local file path or a native image content block. They MUST NOT return image bytes, base64, data URLs or nested JSON text as model-visible text. A managed adapter MAY rewrite an upstream text-wrapped image into a native image block or a local file; it MUST NOT re-encode the same bytes as additional JSON text. Screenshot calls without an owned path SHALL still produce a bounded visual result or an explicit bounded refusal. Browser snapshots and element references remain the preferred web inspection path; desktop list, title and state operations remain the preferred window-identity path.

#### Scenario: Window capture without a caller path
- **WHEN** a consumer requests a desktop or window screenshot and omits a destination path
- **THEN** the delivered MCP result contains a native image block or a local file reference, not PNG or base64 inside `type: text`

#### Scenario: Capture is written to an owned path
- **WHEN** a consumer supplies an owned destination path
- **THEN** the model-visible result identifies that path and omits the image bytes

#### Scenario: Upstream wraps image bytes as JSON text
- **WHEN** the audited Nuphus executable returns a text block whose payload is image data
- **THEN** the managed adapter converts or stores that payload before it enters model context and does not emit a second nested JSON string of the same bytes

