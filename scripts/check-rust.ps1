$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

Push-Location $projectRoot
try {
    $crateEntries = Get-ChildItem -LiteralPath "rust\crates" -Directory |
        ForEach-Object {
            $library = Join-Path $_.FullName "src\lib.rs"
            $binary = Join-Path $_.FullName "src\main.rs"
            if (Test-Path -LiteralPath $library) { $library } else { $binary }
        }
    foreach ($entry in $crateEntries) {
        $firstLine = Get-Content -LiteralPath $entry -Encoding UTF8 -TotalCount 1
        if ($firstLine -ne "#![forbid(unsafe_code)]") {
            throw "first-party crate does not forbid unsafe code: $entry"
        }
    }

    $unsafePattern = 'unsafe\s*\{|unsafe\s+fn|unsafe\s+impl|extern\s+"C"|#\s*\[\s*allow\s*\(\s*unsafe_code'
    $unsafeHits = @(& rg -n --glob "*.rs" $unsafePattern "rust\crates")
    if ($LASTEXITCODE -eq 0) {
        $unsafeHits | ForEach-Object { Write-Error $_ }
        throw "first-party unsafe construct detected"
    }
    if ($LASTEXITCODE -gt 1) { exit $LASTEXITCODE }

    cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo test --locked --workspace
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo clippy --locked --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if ($env:AI_EVOLVE_CHECK_V1_REFERENCE -eq "1") {
        & (Join-Path $PSScriptRoot "diff-frontends.ps1")
        exit $LASTEXITCODE
    }
    Write-Output "ok - frozen v1 reference differential skipped (set AI_EVOLVE_CHECK_V1_REFERENCE=1 to enable)"
    exit 0
}
finally {
    Pop-Location
}
