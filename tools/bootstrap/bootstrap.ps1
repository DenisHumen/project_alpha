# SPDX-License-Identifier: MPL-2.0
#
# Developer machine bootstrap for Windows 10/11 (REPO-008).
#
#   powershell -File tools/bootstrap/bootstrap.ps1            install user-level toolchains, then check
#   powershell -File tools/bootstrap/bootstrap.ps1 -Check     only check, change nothing
#   powershell -File tools/bootstrap/bootstrap.ps1 -Skip android,flutter
#
# Never elevates. Missing system tools are reported with the winget command
# that installs them; run it yourself and re-run the bootstrap.

param(
    [switch]$Check,
    [string[]]$Skip = @()
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Skip = @($Skip | ForEach-Object { $_ -split ',' } | Where-Object { $_ })

$V = @{}
Get-Content (Join-Path $PSScriptRoot 'versions.env') | ForEach-Object {
    if ($_ -match '^\s*([A-Z0-9_]+)=(.*)$') { $V[$Matches[1]] = $Matches[2].Trim() }
}

$script:Failed = $false
$script:Actions = New-Object System.Collections.Generic.List[string]
function Ok($m)   { Write-Host "  ok    $m" -ForegroundColor Green }
function Warn($m) { Write-Host "  warn  $m" -ForegroundColor Yellow }
function Fail($m) { Write-Host "  fail  $m" -ForegroundColor Red; $script:Failed = $true }
function Todo($m) { if (-not $script:Actions.Contains($m)) { $script:Actions.Add($m) } }
function Step($m) { Write-Host "`n$m" -ForegroundColor White }
function Have($c) { [bool](Get-Command $c -ErrorAction SilentlyContinue) }

# --- 1. System tools --------------------------------------------------------
Step 'System tools (winget)'
$required = @(
    @{ cmd = 'git';     id = 'Git.Git' },
    @{ cmd = 'git-lfs'; id = 'GitHub.GitLFS' },
    @{ cmd = 'python';  id = 'Python.Python.3.12' },
    @{ cmd = 'cmake';   id = 'Kitware.CMake' },
    @{ cmd = 'ninja';   id = 'Ninja-build.Ninja' }
)
foreach ($t in $required) {
    if (Have $t.cmd) { Ok $t.cmd } else {
        Fail "$($t.cmd) missing"
        Todo "winget install --id $($t.id) -e"
    }
}
if (Have python) {
    $pyv = (& python -c 'import sys; print("%d.%d" % sys.version_info[:2])')
    if ([version]$pyv -ge [version]$V['MIN_PYTHON']) { Ok "python $pyv" } else { Fail "python $pyv < $($V['MIN_PYTHON'])" }
}
# MSVC toolchain for Rust (x86_64-pc-windows-msvc) and Flutter Windows desktop.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = $null
if (Test-Path $vswhere) {
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if ($vs) { Ok "MSVC build tools ($vs)" } else {
    Fail 'Visual Studio Build Tools with "Desktop development with C++" missing'
    Todo 'winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"'
}

# --- 2. Rust ----------------------------------------------------------------
Step 'Rust'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:PATH = "$cargoBin;$env:PATH" }
$channel = (Select-String -Path (Join-Path $Root 'rust-toolchain.toml') -Pattern '^channel\s*=\s*"(.*)"').Matches[0].Groups[1].Value
if (-not (Have rustup)) {
    Fail 'rustup missing'
    Todo 'winget install --id Rustlang.Rustup -e'
} else {
    if (-not $Check) {
        Push-Location $Root
        & rustup toolchain install $channel --profile minimal --component clippy,rustfmt | Out-Null
        Pop-Location
    }
    Push-Location $Root
    $rv = (& rustc --version 2>$null)
    Pop-Location
    if ($rv -match " $([regex]::Escape($channel)) ") { Ok "rustc $channel (rust-toolchain.toml)" } else {
        Fail "rust $channel not installed"; Todo 'just bootstrap'
    }
}

# --- 3. mise ----------------------------------------------------------------
Step 'mise.toml (Flutter, JDK, Go, Node, just, cargo-ndk, nextest)'
if (-not (Have mise)) {
    Fail 'mise missing'
    Todo "winget install --id jdx.mise -e --version $($V['MISE_VERSION'])"
    Todo 'activate mise: https://mise.jdx.dev/getting-started.html (PowerShell)'
} else {
    Ok "mise $((& mise --version) -split ' ' | Select-Object -First 1)"
    Push-Location $Root
    if (-not $Check) {
        & mise trust --quiet (Join-Path $Root 'mise.toml')
        if ($Skip -contains 'flutter') {
            & mise install java go node just cargo:cargo-ndk cargo:cargo-nextest
        } else { & mise install }
    }
    $missing = @(& mise ls --current --missing --no-header 2>$null | ForEach-Object { ($_ -split '\s+')[0] } |
        Where-Object { $_ -and -not ($_ -eq 'flutter' -and $Skip -contains 'flutter') })
    Pop-Location
    if ($missing.Count -eq 0) { Ok 'all mise.toml tools installed' } else {
        $missing | ForEach-Object { Fail "$_ (mise.toml) not installed" }
        Todo 'mise install'
    }
}

# --- 4. Android SDK ---------------------------------------------------------
if ($Skip -notcontains 'android') {
    Step "Android SDK ($($V['ANDROID_PLATFORM']), build-tools $($V['ANDROID_BUILD_TOOLS']), NDK $($V['ANDROID_NDK']))"
    $sdk = $env:ANDROID_HOME
    if (-not $sdk) { $sdk = $env:ANDROID_SDK_ROOT }
    if (-not $sdk) { $sdk = Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
    $sdkmanager = Join-Path $sdk 'cmdline-tools\latest\bin\sdkmanager.bat'
    $pkgs = @('platform-tools', "platforms;$($V['ANDROID_PLATFORM'])",
              "build-tools;$($V['ANDROID_BUILD_TOOLS'])", "ndk;$($V['ANDROID_NDK'])")
    if (-not (Test-Path $sdkmanager)) {
        Fail "Android cmdline-tools missing in $sdk"
        Todo "install Android Studio (winget install --id Google.AndroidStudio -e) or cmdline-tools; set ANDROID_HOME"
    } elseif (-not (Test-Path (Join-Path $sdk 'licenses\android-sdk-license'))) {
        Fail 'Android SDK licences not accepted'
        Todo "read and accept the licences yourself: & '$sdkmanager' --licenses"
    } else {
        if (-not $Check) { & $sdkmanager "--sdk_root=$sdk" --install @pkgs | Out-Null }
        foreach ($p in $pkgs) {
            if (Test-Path (Join-Path $sdk ($p -replace ';', '\'))) { Ok $p } else {
                Fail "$p missing"; Todo "& '$sdkmanager' --sdk_root=$sdk --install '$p'"
            }
        }
    }
    if (-not $env:ANDROID_HOME) { Warn "ANDROID_HOME is not set (expected $sdk)" }
}

# --- Summary ----------------------------------------------------------------
Write-Host ''
if (-not $script:Failed) {
    Write-Host 'bootstrap: environment is ready' -ForegroundColor Green
    exit 0
}
Write-Host 'bootstrap: some checks failed. Do this, then re-run `just bootstrap`:' -ForegroundColor Red
$script:Actions | ForEach-Object { Write-Host "  - $_" }
exit 1
