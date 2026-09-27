## 1. Launcher

- [x] 1.1 Classify TUI launches that always run embedded (live shared
      defaults or an explicit profile) and pass
      `-c features.daemon_auto_start=false`, leaving non-TUI commands,
      `--remote`, explicit opt-outs and daemon-capable starts unchanged.
      Verify with the launcher oracle suite.
- [x] 1.2 Wire the opt-out into ordinary native launch after shared-default
      injection. Verify with the native launcher integration suite.

## 2. Verification and records

- [x] 2.1 Verify the codex-cli 0.157 contract on the installed CLI: the
      feature override flips `daemon_auto_start` off and the argument
      combination parses for the TUI path; record the pinned-source analysis
      in the design note.
- [ ] 2.2 Deploy the built manager through the kit installation lifecycle
      and confirm the ordinary start path through the installed launcher.
