## 1. Process containment

- [x] 1.1 Make `inherit_console` inherit parent stdio/console without NUL redirection, reject mixing it with redirected handles, and verify with a process test that a console-inherited child is in the job and not bound to NUL
- [x] 1.2 Add unbounded root wait plus leftover reap for a foreground Job and verify a `tree-exit` grandchild dies after the root exits without an execution deadline

## 2. Launcher session job

- [x] 2.1 Wrap `native_launcher::run` in a kill-on-close session Job using `inherit_console`, without helper memory/CPU caps or assigning the wrapper itself, and verify `run` returns the upstream exit code while a detached helper is reaped
- [x] 2.2 Keep independently started kit services outside the session job and verify existing `command()` pipe tests and xAI shim sibling startup still pass

## 3. Checks

- [x] 3.1 Run `cargo test --locked -p harness-core --jobs 1 native_launcher -- --test-threads=1` and `cargo test --locked -p harness-core --test process --jobs 1 -- --test-threads=1` after building `harness-process-fixture`, and confirm they pass
