[CmdletBinding()]
param(
    [ValidateSet('msi', 'nsis')]
    [string]$Bundle = 'msi',

    [switch]$SkipBuild
)

<#
.SYNOPSIS
Builds and starts a Windows TokenScope installer.

.DESCRIPTION
Builds the selected MSI or NSIS package, then runs its installer to install or
update TokenScope in place. It does not uninstall the current version or remove
the user's local data.

.PARAMETER Bundle
Installer format to build and run. Defaults to MSI.

.PARAMETER SkipBuild
Uses the newest installer already present in the release bundle directory.

.EXAMPLE
pwsh -File scripts/reinstall.ps1

.EXAMPLE
pwsh -File scripts/reinstall.ps1 -Bundle nsis

.EXAMPLE
pwsh -File scripts/reinstall.ps1 -Bundle msi -SkipBuild
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'Run this script with PowerShell 7: pwsh -File scripts/reinstall.ps1'
}
if (-not $IsWindows) {
    throw 'The TokenScope reinstall script supports Windows installers only.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $repoRoot 'apps\desktop'
$bundleDirectory = Join-Path $desktopRoot "src-tauri\target\release\bundle\$Bundle"
$extension = if ($Bundle -eq 'msi') { '.msi' } else { '.exe' }

if (-not $SkipBuild) {
    $buildScript = Join-Path $PSScriptRoot 'build-installer.ps1'
    if (-not (Test-Path -LiteralPath $buildScript -PathType Leaf)) {
        throw "Installer build script not found: $buildScript"
    }

    Write-Host "Building the TokenScope $Bundle installer..."
    & $buildScript -Bundles $Bundle
    if ($LASTEXITCODE -ne 0) {
        throw "Installer build failed with exit code $LASTEXITCODE."
    }
}

if (-not (Test-Path -LiteralPath $bundleDirectory -PathType Container)) {
    throw "Installer bundle directory not found: $bundleDirectory"
}

$installer = Get-ChildItem -LiteralPath $bundleDirectory -File -Filter "*$extension" |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $installer) {
    throw "No $Bundle installer was found in: $bundleDirectory"
}

Write-Host "Starting in-place install/update: $($installer.FullName)"
Write-Host "The script does not stop TokenScope or remove its local data. Close the app if the installer requests it."

if ($Bundle -eq 'msi') {
    $msiexec = Join-Path $env:WINDIR 'System32\msiexec.exe'
    if (-not (Test-Path -LiteralPath $msiexec -PathType Leaf)) {
        throw "Windows Installer was not found: $msiexec"
    }

    $installerArguments = @('/i', "`"$($installer.FullName)`"", '/norestart')
    $process = Start-Process -FilePath $msiexec -ArgumentList $installerArguments -Wait -PassThru
    if ($process.ExitCode -eq 3010) {
        Write-Host 'Installation completed; Windows reports that a restart is required.'
    } elseif ($process.ExitCode -ne 0) {
        throw "MSI installer failed with exit code $($process.ExitCode)."
    } else {
        Write-Host 'TokenScope installation completed.'
    }
} else {
    $process = Start-Process -FilePath $installer.FullName -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "NSIS installer failed with exit code $($process.ExitCode)."
    }
    Write-Host 'TokenScope installation completed.'
}
