$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

Push-Location $projectRoot
try {
    & (Join-Path $PSScriptRoot "check-repository-boundaries.ps1")
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo test --locked --workspace
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo clippy --locked --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cargo check --locked --manifest-path fuzz/Cargo.toml --bins
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    node --test scripts/release.test.mjs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    node scripts/release-metadata.mjs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if ($env:YANSHU_CHECK_V1_REFERENCE -eq "1") {
        & (Join-Path $PSScriptRoot "diff-frontends.ps1")
        exit $LASTEXITCODE
    }
    Write-Output "ok - frozen v1 reference differential skipped (set YANSHU_CHECK_V1_REFERENCE=1 to enable)"
    exit 0
}
finally {
    Pop-Location
}
