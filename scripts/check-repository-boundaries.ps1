$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$crateRoot = Join-Path $projectRoot "rust/crates"
$fuzzTargetRoot = Join-Path $projectRoot "fuzz/fuzz_targets"

function Assert-ForbidsUnsafeCode([string] $entry) {
    $firstLine = Get-Content -LiteralPath $entry -Encoding UTF8 -TotalCount 1
    if ($firstLine -ne "#![forbid(unsafe_code)]") {
        throw "first-party Rust entry does not forbid unsafe code: $entry"
    }
}

$crateEntries = Get-ChildItem -LiteralPath $crateRoot -Directory |
    ForEach-Object {
        $library = Join-Path $_.FullName "src/lib.rs"
        $binary = Join-Path $_.FullName "src/main.rs"
        if (Test-Path -LiteralPath $library) { $library } else { $binary }
    }
foreach ($entry in $crateEntries) {
    Assert-ForbidsUnsafeCode $entry
}

$sourceRoots = @($crateRoot)
if (Test-Path -LiteralPath $fuzzTargetRoot) {
    $fuzzEntries = @(Get-ChildItem -LiteralPath $fuzzTargetRoot -Filter *.rs -File)
    foreach ($entry in $fuzzEntries) {
        Assert-ForbidsUnsafeCode $entry.FullName
    }
    $sourceRoots += $fuzzTargetRoot
}

$unsafePattern = '\bunsafe\s*(\{|fn\b|impl\b|trait\b|extern\b)|extern\s+"C"|#\s*\[\s*allow\s*\(\s*unsafe_code'
$unsafeHits = @(
    Get-ChildItem -LiteralPath $sourceRoots -Recurse -Filter *.rs -File |
        Select-String -Pattern $unsafePattern
)
if ($unsafeHits.Count -gt 0) {
    $unsafeHits | ForEach-Object { Write-Error $_ }
    throw "first-party unsafe construct detected"
}

$workflowRoot = Join-Path $projectRoot ".github/workflows"
$unpinnedActions = @()
if (Test-Path -LiteralPath $workflowRoot) {
    $actionUses = @(
        Get-ChildItem -LiteralPath $workflowRoot -Filter *.yml -File |
            Select-String -Pattern '^\s*uses:\s*([^\s#]+)'
    )
    foreach ($use in $actionUses) {
        $reference = $use.Matches[0].Groups[1].Value
        if ($reference.StartsWith("./")) { continue }
        if ($reference -notmatch '@[0-9a-f]{40}$') {
            $unpinnedActions += $use
        }
    }
}
if ($unpinnedActions.Count -gt 0) {
    $unpinnedActions | ForEach-Object { Write-Error $_ }
    throw "third-party GitHub Actions must be pinned to a full commit SHA"
}

Push-Location $projectRoot
try {
    $credentialPatterns = @(
        'sk-[A-Za-z0-9_-]{20,}',
        'gh[pousr]_[A-Za-z0-9]{20,}',
        'AKIA[0-9A-Z]{16}'
    )
    foreach ($pattern in $credentialPatterns) {
        $hits = @(& git grep -n -I -E -- $pattern -- .)
        if ($LASTEXITCODE -eq 0) {
            $hits | ForEach-Object { Write-Error $_ }
            throw "tracked credential-like value detected"
        }
        if ($LASTEXITCODE -gt 1) { exit $LASTEXITCODE }
    }

    $privateKeyMarker = "-----BEGIN " + "PRIVATE KEY-----"
    $privateKeyHits = @(& git grep -n -I -F -- $privateKeyMarker -- .)
    if ($LASTEXITCODE -eq 0) {
        $privateKeyHits | ForEach-Object { Write-Error $_ }
        throw "tracked private key detected"
    }
    if ($LASTEXITCODE -gt 1) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}

Write-Output ("ok - {0} crate roots, {1} fuzz targets, {2} pinned Actions, no unsafe or credential patterns" -f `
    $crateEntries.Count, $fuzzEntries.Count, $actionUses.Count)
exit 0
