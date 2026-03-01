<#
.SYNOPSIS
    Automates the full DiskSleuth release lifecycle.

.DESCRIPTION
    update-application.ps1 is the single authoritative release script for
    DiskSleuth.  It performs every step required to cut a new release:

      1. Validates the target version against the current version in Cargo.toml.
      2. Collects release notes (interactively or from the -Notes parameter).
      3. Verifies git repository state and checks for tag conflicts.
      4. Snapshots manifest and lockfile for automatic rollback on failure.
      5. Updates the workspace version in Cargo.toml and refreshes Cargo.lock.
      6. Shows a diff summary and prompts for confirmation before irreversible steps.
      7. Pre-release build  : cargo build --release
      8. Quality gates      : cargo fmt -- --check
                              cargo clippy -- -D warnings
                              cargo test --workspace
      9. Commits the version bump.
     10. Creates an annotated git tag and pushes HEAD + tag to origin.
     11. Prunes all previous vX.Y.Z tags (local + remote + GitHub Release).
     12. Prints a success banner with the CI monitor URL.

    On any failure the manifest and lockfile are restored from the in-memory
    snapshots so the working tree is left in a clean state.

.PARAMETER Version
    Target semantic version in X.Y.Z format.  If omitted the script prompts
    interactively.  Must be greater than the current version unless -Force is
    specified.

.PARAMETER Notes
    Release notes text.  If omitted the script prompts interactively; enter
    an empty line to finish.  Must be non-empty.

.PARAMETER Force
    Skip the version-regression check (allows releasing the same or an older
    version number) and overwrite an existing tag if one already exists.

.PARAMETER DryRun
    Print every planned action without modifying files, git state, or GitHub.
    Exits 0 on success, 1 on validation failure.

.EXAMPLE
    # Interactive release
    .\update-application.ps1

.EXAMPLE
    # Fully automated (CI-friendly)
    .\update-application.ps1 -Version 2.1.0 -Notes "Bug fixes"

.EXAMPLE
    # Preview without making changes
    .\update-application.ps1 -Version 2.1.0 -Notes "Preview" -DryRun

.EXAMPLE
    # Force-overwrite an existing tag
    .\update-application.ps1 -Version 2.1.0 -Notes "Hotfix" -Force
#>

[CmdletBinding()]
param(
    [string]$Version,
    [string]$Notes,
    [switch]$Force,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# ── Coloured output helpers ───────────────────────────────────────────────────

function Write-Info     { param([string]$msg) Write-Host "[INFO] $msg" -ForegroundColor Cyan }
function Write-Success  { param([string]$msg) Write-Host "[OK]   $msg" -ForegroundColor Green }
function Write-WarnLine { param([string]$msg) Write-Host "[WARN] $msg" -ForegroundColor Yellow }
function Write-ErrorLine { param([string]$msg) Write-Host "[ERR]  $msg" -ForegroundColor Red }

# ── Git helpers ───────────────────────────────────────────────────────────────

# Run a git command, throw a descriptive error if exit code is non-zero.
function Invoke-Git {
    param([string[]]$GitArgs)
    $result = & git @GitArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "git $($GitArgs -join ' ') failed (exit $LASTEXITCODE): $result"
    }
    return $result
}

function Test-IsGitRepository {
    $out = & git rev-parse --is-inside-work-tree 2>&1
    return ($LASTEXITCODE -eq 0 -and "$out".Trim() -eq "true")
}

# Convert the origin remote URL to a browsable HTTPS URL for display.
function Get-RemoteHttpsUrl {
    try {
        $url = (& git remote get-url origin 2>$null) -replace '\.git$', ''
        # SSH format: git@github.com:owner/repo  -> https://github.com/owner/repo
        $url = $url -replace '^git@([^:]+):', 'https://$1/'
        return $url
    } catch {
        return "https://github.com"
    }
}

# ── Path resolution ───────────────────────────────────────────────────────────

# Returns the directory that contains this script (always the workspace root).
function Get-WorkspaceRoot {
    return $PSScriptRoot
}

# ── Version helpers ───────────────────────────────────────────────────────────

# Read the current workspace version from Cargo.toml.
function Get-PackageVersion {
    param([string]$ManifestPath)
    $content = Get-Content $ManifestPath -Raw
    if ($content -match '(?m)\[workspace\.package\][^\[]*version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    throw "Could not parse workspace version from $ManifestPath"
}

# Three-component numeric semver comparison.
# Returns -1 (a < b), 0 (a == b), or 1 (a > b).
function Compare-SemVer {
    param([string]$A, [string]$B)
    $pa = $A.Split('.') | ForEach-Object { [int]$_ }
    $pb = $B.Split('.') | ForEach-Object { [int]$_ }
    for ($i = 0; $i -lt 3; $i++) {
        if ($pa[$i] -lt $pb[$i]) { return -1 }
        if ($pa[$i] -gt $pb[$i]) { return  1 }
    }
    return 0
}

# Regex-replace the workspace version in the manifest.
# Preserves original line endings; writes UTF-8 without BOM.
function Update-PackageVersion {
    param([string]$ManifestPath, [string]$NewVersion)
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    $raw = [System.IO.File]::ReadAllText($ManifestPath)

    # Detect original line ending style (CRLF wins if mixed).
    $hasCrlf = $raw.Contains("`r`n")

    $updated = $raw -replace '(?m)(\[workspace\.package\][^\[]*\r?\n)version = "[^"]+"',
                              "`${1}version = `"$NewVersion`""

    # Normalise to a single trailing newline preserving the original EOL.
    $eol = if ($hasCrlf) { "`r`n" } else { "`n" }
    $updated = $updated.TrimEnd() + $eol

    [System.IO.File]::WriteAllText($ManifestPath, $updated, $utf8NoBom)
}

# =============================================================================
# Main body
# =============================================================================

$root = Get-WorkspaceRoot
Set-Location $root

Write-Host ""
Write-Host "============================================" -ForegroundColor Magenta
Write-Host "   DiskSleuth Release Script" -ForegroundColor Magenta
Write-Host "============================================" -ForegroundColor Magenta
Write-Host ""

$cargoToml = Join-Path $root "Cargo.toml"
$cargoLock = Join-Path $root "Cargo.lock"

try {
    # ── 1. Collect & validate version ────────────────────────────────────────

    $currentVersion = Get-PackageVersion $cargoToml
    Write-Info "Current version : $currentVersion"

    if (-not $Version) {
        $Version = Read-Host "Enter new version (e.g. 1.2.0)"
    }

    if ($Version -notmatch '^\d+\.\d+\.\d+$') {
        throw "Invalid version format '$Version'. Use semantic versioning: X.Y.Z"
    }

    $cmp = Compare-SemVer $Version $currentVersion
    if ($cmp -le 0 -and -not $Force) {
        throw "Version $Version is not greater than current $currentVersion. Use -Force to override."
    }

    Write-Info "Target version  : $Version"

    # ── 2. Collect & validate release notes ──────────────────────────────────

    if (-not $Notes) {
        Write-Info "Enter release notes (empty line to finish):"
        $lines = @()
        while ($true) {
            $line = Read-Host
            if ($line -eq '') { break }
            $lines += $line
        }
        $Notes = $lines -join "`n"
    }

    $Notes = $Notes.Trim()
    if ($Notes -eq '') {
        throw "Release notes must not be empty."
    }

    # ── 3. Git state checks ───────────────────────────────────────────────────

    if (-not (Test-IsGitRepository)) {
        if ($DryRun) {
            Write-WarnLine "Not inside a git repository (dry-run: continuing)."
        } else {
            throw "Not inside a git repository. Aborting."
        }
    }

    $tagName = "v$Version"

    # Check for an existing tag.
    $existingTag = & git tag -l $tagName 2>$null
    if ($existingTag -and -not $Force) {
        throw "Tag $tagName already exists. Use -Force to overwrite."
    }
    if ($existingTag -and $Force) {
        Write-WarnLine "Tag $tagName already exists -- will be deleted (-Force)."
    }

    # Warn on uncommitted changes.
    $dirty = & git status --porcelain 2>$null
    if ($dirty) {
        Write-WarnLine "Working tree has uncommitted changes:"
        $dirty | ForEach-Object { Write-Host "  $_" -ForegroundColor Yellow }
    }

    # ── 4. Snapshot originals for rollback ────────────────────────────────────

    $utf8 = [System.Text.UTF8Encoding]::new($false)
    $originalCargo = [System.IO.File]::ReadAllText($cargoToml, $utf8)
    $originalLock = if (Test-Path $cargoLock) {
        [System.IO.File]::ReadAllText($cargoLock, $utf8)
    } else { $null }

    # ── 5. Dry-run: describe and exit ─────────────────────────────────────────

    if ($DryRun) {
        Write-Host ""
        Write-Info "=== DRY RUN -- no files or git objects will be modified ==="
        Write-Host ""
        Write-Host "  Current version : $currentVersion" -ForegroundColor White
        Write-Host "  New version     : $Version"        -ForegroundColor White
        Write-Host "  Git tag         : $tagName"        -ForegroundColor White
        Write-Host ""
        Write-Host "  Release notes:"
        $Notes -split "`n" | ForEach-Object { Write-Host "    $_" }
        Write-Host ""
        Write-Host "  Planned actions:" -ForegroundColor Cyan
        Write-Host "    [1] Update Cargo.toml workspace version to $Version"
        Write-Host "    [2] Refresh Cargo.lock  (cargo update --workspace)"
        Write-Host "    [3] cargo build --release"
        Write-Host "    [4] cargo fmt -- --check"
        Write-Host "    [5] cargo clippy -- -D warnings"
        Write-Host "    [6] cargo test --workspace"
        if ($existingTag -and $Force) {
            Write-Host "    [7] Delete existing tag $tagName (local + remote)"
        }
        Write-Host "    [8] git add Cargo.toml Cargo.lock"
        Write-Host "    [9] git commit -m `"chore: bump version to $Version`""
        Write-Host "   [10] git tag -a $tagName -m <notes>"
        Write-Host "   [11] git push origin HEAD && git push origin $tagName"
        Write-Host "   [12] Delete all previous vX.Y.Z tags (local + remote + GitHub Release)"
        Write-Host ""
        $remoteUrl = Get-RemoteHttpsUrl
        Write-Info "CI/CD URL : $remoteUrl/actions"
        Write-Success "Dry run complete -- no changes made."
        exit 0
    }

    # ── 6. Step 1: Update version strings ────────────────────────────────────

    Write-Info "Step 1 -- Updating version strings..."
    Update-PackageVersion $cargoToml $Version
    Write-Success "  Cargo.toml workspace version -> $Version"

    Write-Info "  Refreshing Cargo.lock..."
    & cargo update --workspace 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "cargo update --workspace failed" }
    Write-Success "  Cargo.lock refreshed"

    $changedFiles = @($cargoToml)
    if (Test-Path $cargoLock) { $changedFiles += $cargoLock }

    # ── 7. Show summary, diff, confirm ────────────────────────────────────────

    Write-Host ""
    Write-Info "Release summary:"
    Write-Host "  $currentVersion  ->  $Version  ($tagName)" -ForegroundColor White
    Write-Host ""
    Write-Host "  Release notes:"
    $Notes -split "`n" | ForEach-Object { Write-Host "    $_" }
    Write-Host ""
    Write-Info "Diff of changed files:"
    & git --no-pager diff -- $changedFiles
    Write-Host ""

    $answer = Read-Host "Proceed? (y/N)"
    if ($answer -notmatch '^(y|Y|yes|YES)$') {
        Write-Host "Aborted by user." -ForegroundColor Yellow
        [System.IO.File]::WriteAllText($cargoToml, $originalCargo, $utf8)
        if ($null -ne $originalLock -and (Test-Path $cargoLock)) {
            [System.IO.File]::WriteAllText($cargoLock, $originalLock, $utf8)
        }
        exit 1
    }

    # ── 8. Step 2: Pre-release build ─────────────────────────────────────────

    Write-Info "Step 2 -- Pre-release build (cargo build --release)..."
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
    Write-Success "  Build succeeded"

    # ── 9. Step 3: Quality gates ──────────────────────────────────────────────

    Write-Info "Step 3 -- Quality gates..."

    Write-Info "  [fmt]  cargo fmt -- --check"
    & cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw "cargo fmt check failed -- run 'cargo fmt --all' and commit." }
    Write-Success "  Format check passed"

    Write-Info "  [lint] cargo clippy -- -D warnings"
    & cargo clippy --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed -- fix warnings before releasing." }
    Write-Success "  Lint passed"

    Write-Info "  [test] cargo test --workspace"
    & cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed -- all tests must pass before releasing." }
    Write-Success "  All tests passed"

    # ── 10. Step 4: Handle existing tag ──────────────────────────────────────

    if ($existingTag -and $Force) {
        Write-Info "Step 4 -- Deleting existing tag $tagName (-Force)..."
        & git tag -d $tagName 2>&1 | Out-Null
        $ErrorActionPreference = "Continue"
        & git push origin --delete $tagName 2>&1 | Out-Null
        $ErrorActionPreference = "Stop"
        Write-Success "  Deleted existing tag $tagName"
    }

    # ── 11. Step 5: Commit version bump ──────────────────────────────────────

    Write-Info "Step 5 -- Committing version bump..."
    Invoke-Git (@("add", "--") + $changedFiles)
    $stagedChanges = & git diff --cached --quiet ; $hasStagedChanges = $LASTEXITCODE -ne 0
    if ($hasStagedChanges) {
        Invoke-Git @("commit", "-m", "chore: bump version to $Version")
        Write-Success "  Committed: chore: bump version to $Version"
    } else {
        Write-Info "  Nothing to commit (version already at $Version) -- skipping commit"
    }

    # ── 12. Step 6: Tag and push ──────────────────────────────────────────────

    Write-Info "Step 6 -- Creating tag $tagName and pushing..."
    $tmp = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllText($tmp, $Notes, $utf8)
    Invoke-Git @("tag", "-a", $tagName, "-F", $tmp)
    Remove-Item $tmp

    Invoke-Git @("push", "origin", "HEAD")
    Invoke-Git @("push", "origin", $tagName)
    Write-Success "  Pushed HEAD and tag $tagName"

    # ── 13. Step 7: Prune older release tags ─────────────────────────────────

    Write-Info "Step 7 -- Pruning older release tags..."
    $allTags = & git tag -l "v*.*.*" | Where-Object { $_.Trim() -ne $tagName }
    $ghAvailable = $null -ne (Get-Command gh -ErrorAction SilentlyContinue)

    foreach ($oldTag in $allTags) {
        $oldTag = $oldTag.Trim()
        if (-not $oldTag) { continue }

        if ($ghAvailable) {
            $ErrorActionPreference = "Continue"
            & gh release delete $oldTag --yes 2>&1 | Out-Null
            $ErrorActionPreference = "Stop"
        }

        & git tag -d $oldTag 2>&1 | Out-Null

        $ErrorActionPreference = "Continue"
        & git push origin --delete $oldTag 2>&1 | Out-Null
        $ErrorActionPreference = "Stop"

        Write-Info "  Pruned $oldTag"
    }
    if ($allTags) {
        Write-Success "  Pruned $(@($allTags).Count) old tag(s)"
    } else {
        Write-Info "  No previous tags to prune"
    }

    # ── Done ────────────────────────────────────────────────────────────────

    $remoteUrl = Get-RemoteHttpsUrl
    Write-Host ""
    Write-Host "============================================" -ForegroundColor Green
    Write-Success "Release $tagName created successfully!"
    Write-Host "============================================" -ForegroundColor Green
    Write-Host ""
    Write-Info "GitHub Actions will build and publish the release."
    Write-Info "Monitor CI : $remoteUrl/actions"
    Write-Host ""

} catch {
    Write-Host ""
    Write-ErrorLine "Release failed: $_"
    Write-WarnLine  "Rolling back manifest and lockfile..."

    if ($null -ne $originalCargo) {
        $rb_utf8 = [System.Text.UTF8Encoding]::new($false)
        [System.IO.File]::WriteAllText($cargoToml, $originalCargo, $rb_utf8)
        Write-Info "  Cargo.toml restored"
    }
    if ($null -ne $originalLock -and (Test-Path $cargoLock)) {
        $rb_utf8 = [System.Text.UTF8Encoding]::new($false)
        [System.IO.File]::WriteAllText($cargoLock, $originalLock, $rb_utf8)
        Write-Info "  Cargo.lock restored"
    }

    Write-ErrorLine "Release aborted. Working tree restored."
    exit 1
}
