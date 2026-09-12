## ADDED Requirements

### Requirement: Subscription-backed Z.AI GLM access
The kit SHALL make the user's Z.AI GLM Coding Plan available through the
existing OpenCodex proxy in ordinary global Codex sessions. The ordinary
`/model` catalogue SHALL list exactly `gpt-6-astra`, `xai/grok-4.6`, and
`zai/glm-5.3` after the provider is connected and authorized. A request for
`zai/glm-5.3` SHALL use
the Z.AI Coding Plan Chat route, not an xAI route and not a different GLM
identifier. GPT-6 Astra SHALL remain the kit default model. The Grok middle
role SHALL remain assigned to `xai/grok-4.6`. No GLM named role is required.
Credentials MUST remain outside tracked reusable artifacts and MUST NOT be
printed in diagnostics, process request files, or public reports.

#### Scenario: GLM appears in the ordinary model picker
- **WHEN** the authenticated user opens `/model` in an ordinary global Codex
  session outside this checkout after Z.AI is connected
- **THEN** the list is exactly `gpt-6-astra`, `xai/grok-4.6`, and
  `zai/glm-5.3`, and does not require `--profile zai`

#### Scenario: GLM runs as the selected main model
- **WHEN** the user selects `zai/glm-5.3` in that ordinary session and sends a
  request
- **THEN** the request completes through the local OpenCodex proxy to the Z.AI
  Coding Plan Chat endpoint and the actual provider and model can be verified
  without disclosing credentials

#### Scenario: Z.AI access is unavailable
- **WHEN** the Z.AI key, quota, or endpoint prevents a `zai/glm-5.3` request
- **THEN** the failure is visible and the request is not silently sent to
  another provider, another GLM id, a paid xAI API, or the local `zai`
  profile route

#### Scenario: Default and middle assignments stay unchanged
- **WHEN** a new ordinary Codex session starts without an explicit model
  override after Z.AI is connected
- **THEN** the session does not default to GLM, and a Grok middle delegation
  still uses `xai/grok-4.6`

### Requirement: Ordinary model picker allowlist
The ordinary global Codex `/model` list SHALL contain only `gpt-6-astra`,
`xai/grok-4.6`, and `zai/glm-5.3` after OpenCodex routing is connected.
Other native GPT identifiers, other Grok identifiers, other GLM family
identifiers, and synthetic `--fast` rows MUST NOT appear in that list. The
local `codex --profile zai` catalogue remains independent and MAY keep its
bare `glm-5.3` slug.

#### Scenario: Extra catalogue rows stay hidden
- **WHEN** the authenticated user opens `/model` in an ordinary global Codex
  session after OpenCodex routing is connected
- **THEN** the picker does not list GPT-5.x natives, other `xai/grok-*` ids,
  other GLM ids, or `--fast` variants

### Requirement: Local Z.AI profile remains independent
The existing user-owned local Codex file profile invoked as
`codex --profile zai` SHALL keep its current Responses route, catalogue, and
files. Installation, login, catalogue sync, and disconnect of OpenCodex Z.AI
routing MUST NOT create, edit, replace, or delete that profile. Both paths
SHALL remain independently usable: the local profile without the OpenCodex
proxy, and `zai/glm-5.3` through the proxy in ordinary sessions.

#### Scenario: Explicit profile launch is unchanged
- **WHEN** the user runs `codex --profile zai` after OpenCodex Z.AI routing is
  installed
- **THEN** the session still uses the local profile's Z.AI Responses
  configuration and catalogue, not the OpenCodex Chat route or
  `zai/glm-5.3` slug

#### Scenario: OpenCodex lifecycle leaves the profile files intact
- **WHEN** subscription Install, Update, login, Check, Recover, or Disconnect
  runs
- **THEN** the local `zai` profile files are not modified and the profile
  launch still works after the operation

## MODIFIED Requirements

### Requirement: Direct reusable configuration and private state
Reusable proxy settings and agent definitions SHALL be read directly from
repository sources through supported references or links. OAuth tokens, API
keys, catalogs, runtime logs, process state and installation recovery records
SHALL remain on the host. Source links SHALL survive supported proxy
configuration writes. Tracked OpenCodex configuration MAY name an environment
or keychain reference for a key-auth provider, but MUST NOT contain credential
material. Stock OpenCodex key-login or provider-add commands that persist a
plaintext API key into the linked configuration MUST NOT be the delivered
authorization path.

#### Scenario: Configuration is edited through the proxy
- **WHEN** a supported non-secret proxy setting is saved
- **THEN** the active configuration still resolves to the repository source
  and credentials remain outside that source

#### Scenario: Z.AI key is stored for the proxy
- **WHEN** the user authorizes the OpenCodex Z.AI provider with the Coding
  Plan API key
- **THEN** the key is stored only in host-private state, tracked source keeps
  at most a non-secret reference, and the local `zai` profile is not read or
  copied as the secret source
