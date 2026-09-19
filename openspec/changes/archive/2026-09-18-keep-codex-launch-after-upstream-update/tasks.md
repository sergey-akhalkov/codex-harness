## 1. Launcher

- [x] 1.1 On a changed registered upstream executable hash, still
      prepare/exec that path with no extra warning when the harness session
      is otherwise healthy. Keep refusing launcher recursion and a missing
      upstream file. Verify with the native launcher suite.
- [x] 1.2 On a changed `@openai/codex` package.json digest, keep launching
      and still apply managed-package environment when the identity matches.
      Skip that env without blocking launch when the package is unreadable or
      no longer that identity. Verify with the native launcher suite.

## 2. Documentation

- [x] 2.1 Update installation, native-command and decision records so
      ordinary launch continues after an upstream Codex update without a
      routine warning, without private paths.
