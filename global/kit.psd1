@{
    SchemaVersion = 1
    ProfileName = 'harness'
    Profile = 'global/harness.config.toml'
    Instructions = 'global/principles-of-work.md'
    Skills = '.agents/skills'
    Agents = 'global/agents'
    Launcher = 'tools/codex.ps1'
    Hooks = 'global/hooks.json'
    HookLauncher = 'tools/hook.ps1'
    RequiredFiles = @('install.ps1', 'tools/kit.psm1', 'tools/codex.ps1', 'tools/launcher.psm1',
        'tools/code-tools.psm1', 'tools/activation.psm1', 'tools/mcp.ps1', 'tools/hook.ps1', 'global/hooks.json', 'global/code-tools.json',
        'tools/code-tools/registration.py', 'tools/code-tools/discovery.py', 'tools/code-tools/launch.py', 'tools/code-tools/check.py',
        'tools/code-tools/graphify_proxy.py', 'tools/code-tools/graphify_update.py', 'tools/code-tools/mcp_provision.py', 'tools/code-tools/lsp_provision.py', 'tools/code-tools/serena_entry.py', 'tools/code-tools/nuphus_proxy.py', 'tools/code-tools/dependencies.py',
        'tools/lsp/server.py', 'tools/lsp/backend.py', 'tools/lsp/journal.py', 'tools/lsp/registry.py', 'tools/lsp/delphi.py',
        'tools/lsp/markdown_client.py', 'tools/lsp/markdown-parse.cjs', 'tools/lsp/bash-windows.cjs', 'tools/lsp/xml-diagnostics.ps1', 'tools/lsp/powershell-diagnostics.ps1')
    ManagedSettings = @(
        'approval_policy', 'sandbox_mode', 'model', 'model_reasoning_effort',
        'approvals_reviewer', 'notice.hide_rate_limit_model_nudge', 'tui.pet'
    )
    # These are observed compatibility baselines, checked again by the installer.
    PowerShellMinimum = '7.4'
    CodexMinimum = '0.153.4'
    OpenSpecMinimum = '1.12.0'
}
