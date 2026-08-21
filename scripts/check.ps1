$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

Push-Location $projectRoot
try {
    & (Join-Path $PSScriptRoot "check-repository-boundaries.ps1")
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo test --locked --workspace -j 1
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo clippy --locked --workspace --all-targets -j 1 -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo check --locked --manifest-path fuzz/Cargo.toml --bins
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    node --test scripts/release.test.mjs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    node scripts/release-metadata.mjs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    node scripts/check-doc-links.mjs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Output "ok - Rust workspace, fuzz targets, release tooling, and documentation links passed"
    exit 0
}
finally {
    Pop-Location
}
