## 1. Stable manager link in registrations

- [x] 1.1 Generate the retained MCP registrations with the stable
  `<CODEX_HOME>/harness/bin/codex-harness.exe` command instead of a frozen build
  path, and refuse a link that does not resolve into an integrity-verified
  build. Verify with an isolated fixture that code-tools Install/Update records
  the stable command and that the recorded path still resolves after a link is
  re-pointed.
- [x] 1.2 Verify a Codex CLI session started before delivery keeps its manager
  and that a new spawn of the registered command uses the freshly delivered
  build.

## 2. Freshest verified build delivery

- [x] 2.1 Resolve the delivered build for core Install/Update as the explicit
  `--build` when present, otherwise the freshest integrity-verified build in
  the running manager's owned state; keep `--build` required when the manager
  is not inside an owned state. Verify the resolution with an isolated fixture
  holding two published builds.
- [x] 2.2 Deliver while an earlier build is held open: the operation must
  succeed, the held file must keep its identity and hash, and a new spawn of
  the managed link must report the freshly delivered build. Verify with the
  native suite.

## 3. Documentation

- [x] 3.1 Update the installation and native-command documentation so the
  documented delivery matches the stable link, the optional `--build` and the
  restart boundary for running sessions; keep private paths out.

## 4. Broker generations

- [x] 4.1 Resolve the Serena and CodeGraph broker locations per delivered build
  generation: reuse this generation's location, adopt a free legacy location,
  otherwise prepare one; never retire another live generation. Verify with an
  isolated account fixture that two generations receive distinct locations and
  that the same generation reopens its own.
- [x] 4.2 Verify live that a new session of the delivered build and a session of
  the previous build both complete MCP initialize, and that the previous
  generation keeps serving until its consumers finish.
