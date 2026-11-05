[CmdletBinding()]
param(
    [ValidateSet('msi', 'nsis')]
    [string[]]$Bundles = @('msi', 'nsis')
)

# Usage: pwsh -File scripts/build-installer.ps1
# Select one bundle with: pwsh -File scripts/build-installer.ps1 -Bundles msi

$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'Run this script with PowerShell 7: pwsh -File scripts/build-installer.ps1'
}
if (-not $IsWindows) {
    throw 'This script builds Windows MSI and NSIS installers and must run on Windows.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $repoRoot 'apps\desktop'
$packageJson = Join-Path $desktopRoot 'package.json'
if (-not (Test-Path -LiteralPath $packageJson -PathType Leaf)) {
    throw "Desktop package.json not found: $packageJson"
}

$nodeCommand = Get-Command 'node.exe' -ErrorAction Stop
$nodeVersionText = (& $nodeCommand.Source --version).Trim()
if ($LASTEXITCODE -ne 0) {
    throw 'Could not read the Node.js version.'
}
$nodeVersion = [version]($nodeVersionText.TrimStart('v'))
if ($nodeVersion -lt [version]'22.0') {
    throw "Node.js 22 or newer is required; found $nodeVersionText."
}

foreach ($tool in @('npm.cmd', 'cargo.exe', 'rustc.exe')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Required build tool '$tool' was not found on PATH. Install Node.js 22+ and Rust, then reopen PowerShell 7."
    }
}

if (-not (Get-Command 'cl.exe' -ErrorAction SilentlyContinue)) {
    $vsShellCandidates = [System.Collections.Generic.List[string]]::new()
    $vsShellCandidates.Add('D:\BuildTools\VS2022\Common7\Tools\Launch-VsDevShell.ps1')

    $programFilesX86 = [Environment]::GetFolderPath('ProgramFilesX86')
    $vswhere = Join-Path $programFilesX86 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
        $vsInstall = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath |
            Select-Object -First 1
        if ($LASTEXITCODE -eq 0 -and $vsInstall) {
            $vsShellCandidates.Add((Join-Path $vsInstall 'Common7\Tools\Launch-VsDevShell.ps1'))
        }
    }

    $vsShell = $vsShellCandidates |
        Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
        Select-Object -First 1
    if (-not $vsShell) {
        throw 'Visual Studio 2022 C++ build tools were not found. Install the Desktop development with C++ workload, then rerun this script.'
    }

    . $vsShell -Arch amd64 -HostArch amd64
    if (-not (Get-Command 'cl.exe' -ErrorAction SilentlyContinue)) {
        throw 'Visual Studio shell loaded, but cl.exe is still unavailable.'
    }
}

$nodeModules = Join-Path $desktopRoot 'node_modules'
if (-not (Test-Path -LiteralPath $nodeModules -PathType Container)) {
    Push-Location $desktopRoot
    try {
        npm.cmd ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }
}

$bundleList = ($Bundles | Select-Object -Unique) -join ','
Write-Host "Building TokenScope Windows installer bundle(s): $bundleList"
Push-Location $desktopRoot
try {
    npm.cmd run pretauri:build
    if ($LASTEXITCODE -ne 0) {
        throw "Agent preparation failed with exit code $LASTEXITCODE."
    }

    npx.cmd tauri build --bundles $bundleList
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri installer build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

$bundleRoot = Join-Path $desktopRoot 'src-tauri\target\release\bundle'
foreach ($bundle in $Bundles | Select-Object -Unique) {
    $bundleDirectory = Join-Path $bundleRoot $bundle
    if (-not (Test-Path -LiteralPath $bundleDirectory -PathType Container)) {
        throw "Expected bundle directory was not created: $bundleDirectory"
    }
    $extension = if ($bundle -eq 'msi') { '.msi' } else { '.exe' }
    $artifact = Get-ChildItem -LiteralPath $bundleDirectory -File -Filter "*$extension" |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $artifact) {
        throw "No .$bundle installer was produced in: $bundleDirectory"
    }
    $hash = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash
    Write-Host "Created: $($artifact.FullName)"
    Write-Host "  Size: $($artifact.Length) bytes"
    Write-Host "  SHA256: $hash"
}