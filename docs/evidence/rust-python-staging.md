# Native empty Python environment preparation

This is a prerequisite within Rust migration task 5.2, which remains open.
The [Rust implementation](../../crates/harness-core/src/dependency_python_stage.rs)
prepares a new empty UV environment under native-owned
`state/python-venv-staging/<unique>/candidate-venv` and writes a manifest.
It does not install wheels, select/activate the environment, relocate it or
register Serena/Graphify. The current shared installations remain adopted.

The explicit CLI is:

```text
codex-harness dependencies stage-python --uv FILE --uv-sha256 DIGEST --python FILE --python-sha256 DIGEST --state DIRECTORY
```

Expected executable hashes must come from trusted selection. The empty venv is
created offline with an explicit base interpreter, private cwd/cache/tool roots,
configuration and project discovery disabled, and no Python downloads or seed
packages. Native Windows Jobs bound child memory/CPU, execution and cleanup.
The API also accepts caller cancellation. Store aliases, wrong hashes, foreign
state, an existing candidate and incompatible `pyvenv.cfg` observations are
rejected. The candidate stays at its original absolute path.

The report is `staged-unverified`; `activation_allowed`, `ready_for_activation`,
`package_install` and `runtime_tree_verified` are false. SHA-256 observations
cover named UV/Python/launcher executables and recorded candidate files. They
do not authenticate the full installed Python/DLL tree or establish wheel
provenance. Version observation failures are explicit and cannot establish
readiness. UV-generated activation resources remain third-party runtime files.

## Actual CLI observation, 2026-09-10

The new CLI ran from outside the checkout with UV 0.11.32 and Python 3.13.14.
The exact selected executables were pinned to:

- UV: `23cf0f8194ff576562646a1a2950c6826249c8806cd1547debd24db77eb68f58`.
- Python: `3a47f5c22ef7b1777587c15598ad1392329ff3c3a656b142263e834d54642827`.

It returned successfully in 2.91 seconds, producing 16 recorded files in
`python-venv-staging/15684-1789019341568155800-0/candidate-venv` under the retained
native dependency state. The manifest SHA-256 is
`b3277913b333be86a71bdc236b13cfc6c994221b4f886bd95468882cc4e0e280`.
All unverified/no-activation flags were preserved. This is an acceptance run,
not a performance comparison or package-installation proof.

Private reports `python-stage-actual-cli.json`, `.stderr` and
`python-stage-actual-cli-observation.json` are under the local
`core-lifecycle-29d511c364fb499286ae57b389e34c6a` evidence directory. Two CLI tests
passed for argument/hash failures and command discovery without starting tools
or creating state. The recorded CLI run predates the final cancellation and
startup-cleanup corrections. A later explicitly enabled native UV/Python test
passed on those corrections in 1.65 seconds (`python-stage-current-actual.stdout`
/ `.stderr`). The default native UV suite uses Rust fixtures; the real package
test is ignored unless supplied explicit executable paths and hashes.

Parent review found a remaining setup-failure deadlock: the retained CommandSpec
kept the parent's stdout write endpoint open while the stderr-startup failure
path joined the stdout reader. The new bounded subprocess test reproduced a
three-second timeout on the old placement and passes after dropping CommandSpec
immediately after child creation. `python-pipe-baseline.*` preserves the failure;
`python-stage-before-pipe-close.rs` preserves the earlier source. The combined
native run passed 29 cases with two explicit package cases ignored; subsequent
focused checks re-exercised the setup-failure branch after its lint adjustment.
Final source/check identity is in the local `cbm-python-checkpoint.json`.

The final rebuilt CLI repeated the outside-checkout preparation successfully
after all corrections. It retained 16 files under
`python-venv-staging/22284-1789021424311474600-0/candidate-venv`, with manifest
SHA-256 `a0b5b13197e70fdef831e3e1aa43a3be2238ef53ac28d32a974a4a54adc3d7e3`.
All unverified/no-activation flags remained false. `python-stage-final-cli.json`
and `cbm-python-final-cli-observation.json` bind this case to final manager
SHA-256 `1eb19258b794c8e964dfdccb05219079cea0fc142e55e836035bd202359ea239`.

Full runtime provenance, wheel locks, registration lifecycle and global
activation remain separate unfinished requirements.
