## ADDED Requirements

### Requirement: Subscription profiles without OpenCodex
The linked kit SHALL connect Grok and Z.AI as native Codex profiles from checkout source without installing OpenCodex, copying a proxy config body, or injecting `openai_base_url` for ordinary sessions. Grok profile files, catalogs and helper registrations SHALL be references or generated host-local wiring that contain no secrets. Discovery SHALL report OpenCodex as retired after the Responses spike, not as a live capability. Ordinary `codex` SHALL keep live shared defaults and native GPT without a localhost model proxy.

#### Scenario: Fresh consumer uses GPT
- **WHEN** a new terminal starts ordinary `codex` in another repository after this connection
- **THEN** shared kit defaults load, traffic does not go to `127.0.0.1:10100`, and Grok is unused unless the xAI profile or an explicit override is selected

#### Scenario: Grok profile is selected
- **WHEN** the user starts `codex --profile xai` after login
- **THEN** the session uses the native xAI provider wiring from the kit without an OpenCodex process

#### Scenario: OpenCodex leftovers are checked
- **WHEN** Check runs after successful retirement
- **THEN** it reports the OpenCodex proxy/task/package as absent or retired and does not treat a leftover localhost injection as healthy kit routing
