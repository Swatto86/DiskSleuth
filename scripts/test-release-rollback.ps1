<#
.SYNOPSIS
    Regression test for update-application.ps1's rollback behaviour.

.DESCRIPTION
    The release script snapshots Cargo.toml/Cargo.lock so it can undo an
    *uncommitted* version bump. Once the bump has been committed, restoring
    those snapshots does not undo anything -- it creates a spurious
    uncommitted downgrade against a commit that may already be on origin.

    This builds a throwaway git repository with a stub `cargo` on PATH, runs
    the real release script in it, and forces a failure on either side of the
    commit:

      * failure BEFORE the commit  -> manifest must be rolled back
      * failure AFTER  the commit  -> manifest must be left alone

.EXAMPLE
    pwsh -File .\scripts\test-release-rollback.ps1
#>

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$script   = Join-Path $repoRoot "update-application.ps1"
if (-not (Test-Path $script)) { throw "update-application.ps1 not found at $script" }

$failures = @()
function Assert-That {
    param([bool]$Condition, [string]$Message)
    if ($Condition) {
        Write-Host "[OK]   $Message" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $Message" -ForegroundColor Red
        $script:failures += $Message
    }
}

# ── Build a throwaway repository ─────────────────────────────────────────────

function New-Fixture {
    param([switch]$WithRemote)

    $dir = Join-Path ([System.IO.Path]::GetTempPath()) ("dsrel_" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $dir | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $dir "stubbin") | Out-Null

    # Stub cargo: the release script's build/lint/test gates are not what is
    # under test here, and running them would take minutes.
    Set-Content -Path (Join-Path $dir "stubbin\cargo.bat") -Value "@echo off`r`nexit /b 0" -Encoding ASCII

    @'
[workspace]
members = []

[workspace.package]
version = "1.1.1"
edition = "2021"
'@ | Set-Content -Path (Join-Path $dir "Cargo.toml") -Encoding utf8

    "# lock" | Set-Content -Path (Join-Path $dir "Cargo.lock") -Encoding utf8
    Copy-Item $script (Join-Path $dir "update-application.ps1")

    Push-Location $dir
    & git init -q .
    & git config user.email "test@example.invalid"
    & git config user.name  "Release Test"
    & git add -A
    & git commit -qm "init"
    Pop-Location

    return $dir
}

# Run the release script in $dir, answering "y" to the confirmation prompt.
function Invoke-ReleaseScript {
    param([string]$Dir, [string]$Version)
    Push-Location $Dir
    try {
        $out = "y" | pwsh -NoProfile -Command @"
`$env:PATH = '$Dir\stubbin;' + `$env:PATH
./update-application.ps1 -Version $Version -Notes 'rollback test'
"@ 2>&1
        return ($out | Out-String)
    } finally {
        Pop-Location
    }
}

function Get-ManifestVersion {
    param([string]$Dir)
    $m = Select-String -Path (Join-Path $Dir "Cargo.toml") -Pattern '^version\s*=\s*"(.+?)"' |
         Select-Object -First 1
    return $m.Matches.Groups[1].Value
}

# ── Case 1: failure after the commit (no origin -> `git push` fails) ─────────

Write-Host ""
Write-Host "Case 1: push fails after the version bump is committed" -ForegroundColor Cyan

$dir = New-Fixture
try {
    $output = Invoke-ReleaseScript -Dir $dir -Version "1.2.0"

    Push-Location $dir
    $committedMsg = (& git log -1 --pretty=%s).Trim()
    $dirty = & git status --porcelain -- Cargo.toml Cargo.lock
    Pop-Location

    Assert-That ($committedMsg -eq "chore: bump version to 1.2.0") `
        "the version bump reached a commit (precondition)"
    Assert-That ([string]::IsNullOrWhiteSpace(($dirty | Out-String))) `
        "manifest files are unmodified -- the committed bump was NOT rolled back"
    Assert-That ((Get-ManifestVersion $dir) -eq "1.2.0") `
        "Cargo.toml still holds the committed version 1.2.0"
    Assert-That ($output -match "NOT rolled back") `
        "the script says the manifest was left alone"
    Assert-That ($output -notmatch "Working tree restored") `
        "the script does not claim the tree was restored"
} finally {
    Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
}

# ── Case 2: failure before the commit (rollback must still happen) ───────────

Write-Host ""
Write-Host "Case 2: failure before the commit still rolls the manifest back" -ForegroundColor Cyan

$dir = New-Fixture
try {
    # Make `cargo update --workspace` (step 1) fail, so the script throws
    # before it reaches the commit.
    Set-Content -Path (Join-Path $dir "stubbin\cargo.bat") -Value "@echo off`r`nexit /b 1" -Encoding ASCII

    $output = Invoke-ReleaseScript -Dir $dir -Version "1.2.0"

    Push-Location $dir
    $commitCount = (& git rev-list --count HEAD).Trim()
    $dirty = & git status --porcelain -- Cargo.toml Cargo.lock
    Pop-Location

    Assert-That ($commitCount -eq "1") "no bump commit was made (precondition)"
    Assert-That ((Get-ManifestVersion $dir) -eq "1.1.1") `
        "Cargo.toml was rolled back to 1.1.1"
    Assert-That ([string]::IsNullOrWhiteSpace(($dirty | Out-String))) `
        "manifest files match HEAD after rollback"
    Assert-That ($output -match "Working tree restored") `
        "the script reports the rollback"
} finally {
    Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
}

# ── Result ───────────────────────────────────────────────────────────────────

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host "$($failures.Count) assertion(s) failed." -ForegroundColor Red
    exit 1
}
Write-Host "All rollback assertions passed." -ForegroundColor Green
exit 0
