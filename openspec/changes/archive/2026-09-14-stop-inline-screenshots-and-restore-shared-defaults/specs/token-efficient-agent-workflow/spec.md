## ADDED Requirements

### Requirement: Images stay images in Code Mode results

Code Mode and other result-shaping routes SHALL return visual captures as native image content or as a local file locator. They MUST NOT stringify image bytes, base64, data URLs or nested JSON image envelopes into the ordinary text aggregate. A screenshot needed only to confirm window identity SHALL be omitted; window list, title, bounds and state are sufficient. Failed, incomplete or oversized visual calls MUST remain explicit without dumping the image payload.

#### Scenario: Screenshot is part of an independent batch
- **WHEN** Code Mode receives a successful window screenshot together with other tool results
- **THEN** the compact result keeps the visual as an image or path, reports any sibling errors, and does not include megabyte-scale image text

#### Scenario: Screenshot was taken to prove a conversation is visible
- **WHEN** the only question is whether a Codex conversation window exists or has a title
- **THEN** the workflow uses list/title/state evidence and does not return a screenshot payload

