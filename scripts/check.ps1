<#
.SYNOPSIS
TokenScope 本地统一门禁验证脚本。

.EXAMPLE
pwsh -File scripts/check.ps1 -Scope fast
pwsh -File scripts/check.ps1 -Scope desktop -StageAgent
pwsh -File scripts/check.ps1 -Scope all
#>
[CmdletBinding()]
param(
    [ValidateSet('fast', 'rust', 'desktop', 'hub', 'contracts', 'all')]
    [string]$Scope = 'all',
    [switch]$StageAgent
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Step {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][scriptblock]$Action
    )
    Write-Host "`n==> $Name" -ForegroundColor Cyan
    $global:LASTEXITCODE = 0
    & $Action
    if ($LASTEXITCODE -ne 0) {
        throw "门禁失败: $Name (exit code: $LASTEXITCODE)"
    }
}

function Resolve-PythonCommand {
    if (Get-Command py -ErrorAction SilentlyContinue) {
        return @('py', '-3')
    }
    if (Get-Command python3 -ErrorAction SilentlyContinue) {
        return @('python3')
    }
    if (Get-Command python -ErrorAction SilentlyContinue) {
        return @('python')
    }
    throw '未找到可用的 Python 解释器 (py / python3 / python)。'
}

function Invoke-RustWorkspaceChecks {
    Invoke-Step 'Rust fmt check (workspace)' { cargo fmt --all -- --check }
    Invoke-Step 'Rust clippy (workspace)' { cargo clippy --locked --workspace --all-targets -- -D warnings }
    Invoke-Step 'Rust test (workspace)' { cargo test --locked --workspace }
}

function Invoke-DesktopChecks {
    Invoke-Step 'Desktop type/Svelte check' { npm --prefix apps/desktop run check }
    Invoke-Step 'Desktop Vitest' { npm --prefix apps/desktop test }
    Invoke-Step 'Desktop Vite build' { npm --prefix apps/desktop run build }

    $stagedMeta = Join-Path $RepoRoot 'apps/desktop/src-tauri/resources/agent-build.json'
    if ($StageAgent -or -not (Test-Path $stagedMeta)) {
        Invoke-Step 'Build & stage debug tokenscope-agent' {
            cargo build --locked -p tokenscope-agent
            if ($LASTEXITCODE -ne 0) { return }
            node scripts/stage-agent.mjs --debug
        }
    }

    $desktopTarget = Join-Path ([System.IO.Path]::GetTempPath()) 'tokenscope-desktop-check'
    Invoke-Step 'Tauri shell fmt check' {
        cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
    }
    Invoke-Step 'Tauri shell clippy' {
        cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --target-dir $desktopTarget --all-targets -- -D warnings
    }
    Invoke-Step 'Tauri shell test' {
        cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --target-dir $desktopTarget
    }
}

function Invoke-HubChecks {
    if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
        throw '未找到 go 命令，无法执行 Hub 验证。'
    }
    Push-Location (Join-Path $RepoRoot 'hub')
    try {
        Invoke-Step 'Go fmt check (hub)' {
            $unformatted = gofmt -l .
            if ($unformatted) {
                $unformatted | Write-Host
                $global:LASTEXITCODE = 1
            }
        }
        Invoke-Step 'Go test (hub)' { go test ./... }
        Invoke-Step 'Go vet (hub)' { go vet ./... }
    }
    finally {
        Pop-Location
    }
}

function Invoke-ContractChecks {
    $pyCmd = Resolve-PythonCommand
    $prevUtf8 = $env:PYTHONUTF8
    $env:PYTHONUTF8 = '1'
    try {
        $scripts = @(
            'tools/verify_domain_vectors.py',
            'tools/verify_local_store_sql.py',
            'tools/verify_codex_semantics.py',
            'scripts/verify_m005.py',
            'scripts/verify_m006.py',
            'scripts/verify_m007.py',
            'scripts/verify_m008.py',
            'scripts/verify_m009.py',
            'scripts/verify_m010.py',
            'scripts/verify_m012.py',
            'scripts/verify_m101_sql.py',
            'scripts/verify_m102.py',
            'scripts/verify_m104.py',
            'scripts/verify_m105.py',
            'scripts/verify_m106.py',
            'scripts/verify_m107.py'
        )
        foreach ($script in $scripts) {
            Invoke-Step "Contract check: $script" {
                if ($pyCmd.Count -eq 2) {
                    & $pyCmd[0] $pyCmd[1] $script
                }
                else {
                    & $pyCmd[0] $script
                }
            }
        }
    }
    finally {
        $env:PYTHONUTF8 = $prevUtf8
    }
}

Push-Location $RepoRoot
try {
    switch ($Scope) {
        'fast' {
            Invoke-Step 'Desktop type/Svelte check' { npm --prefix apps/desktop run check }
            Invoke-Step 'Desktop Vitest' { npm --prefix apps/desktop test }
            Invoke-Step 'Rust fmt check (workspace)' { cargo fmt --all -- --check }
        }
        'rust' { Invoke-RustWorkspaceChecks }
        'desktop' { Invoke-DesktopChecks }
        'hub' { Invoke-HubChecks }
        'contracts' { Invoke-ContractChecks }
        'all' {
            Invoke-RustWorkspaceChecks
            Invoke-DesktopChecks
            Invoke-HubChecks
            Invoke-ContractChecks
        }
    }
    Write-Host "`n[PASS] 本地门禁 ($Scope) 全部通过。" -ForegroundColor Green
}
finally {
    Pop-Location
}
