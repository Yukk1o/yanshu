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

Write-Output ("ok - {0} crate roots, {1} fuzz targets, no unsafe or credential patterns" -f `
    $crateEntries.Count, $fuzzEntries.Count)
exit 0
