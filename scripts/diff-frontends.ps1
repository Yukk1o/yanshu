$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$racketExe = Join-Path $projectRoot ".toolchains\racket\Racket.exe"
$racketCli = Join-Path $projectRoot "src\cli.rkt"
$rustExe = Join-Path $projectRoot "target\debug\ail-cli.exe"

if (-not (Test-Path -LiteralPath $racketExe)) {
    & (Join-Path $PSScriptRoot "bootstrap.ps1")
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Push-Location $projectRoot
try {
    cargo build --locked --quiet -p ail-cli
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $cases = @(
        "conformance\v1\programs\core.ail",
        "conformance\v1\programs\schema.ail",
        "conformance\v1\programs\library.ail",
        "conformance\v1\invalid\multiple-forms.ail",
        "conformance\v1\invalid\unknown-library.ail"
    )

    foreach ($source in $cases) {
        $racketOutput = @(& $racketExe $racketCli inspect $source)
        $racketExit = $LASTEXITCODE
        $rustOutput = @(& $rustExe inspect $source)
        $rustExit = $LASTEXITCODE

        if ($racketExit -ne $rustExit) {
            throw "frontend exit code differs for ${source}: Racket=$racketExit Rust=$rustExit"
        }

        $racketJson = ($racketOutput -join "`n") |
            ConvertFrom-Json |
            ConvertTo-Json -Depth 100 -Compress
        $rustJson = ($rustOutput -join "`n") |
            ConvertFrom-Json |
            ConvertTo-Json -Depth 100 -Compress
        if ($racketJson -ne $rustJson) {
            throw "frontend JSON differs for $source"
        }
        Write-Output "ok - frontend parity: $source"
    }

    $manifest = "conformance\v1\manifest.json"
    $racketOutput = @(& $racketExe $racketCli conformance $manifest)
    $racketExit = $LASTEXITCODE
    $rustOutput = @(& $rustExe conformance $manifest)
    $rustExit = $LASTEXITCODE
    if ($racketExit -ne $rustExit) {
        throw "conformance exit code differs: Racket=$racketExit Rust=$rustExit"
    }
    $racketJson = ($racketOutput -join "`n") |
        ConvertFrom-Json |
        ConvertTo-Json -Depth 100 -Compress
    $rustJson = ($rustOutput -join "`n") |
        ConvertFrom-Json |
        ConvertTo-Json -Depth 100 -Compress
    if ($racketJson -ne $rustJson) {
        throw "complete conformance report differs"
    }
    Write-Output "ok - complete conformance report parity: 17 cases"

    $serviceProgram = "examples\tasks\service.ail"
    $serviceSuite = "examples\tasks\scenarios.json"
    $racketOutput = @(& $racketExe $racketCli test-service $serviceProgram $serviceSuite)
    $racketExit = $LASTEXITCODE
    $rustOutput = @(& $rustExe test-service $serviceProgram $serviceSuite)
    $rustExit = $LASTEXITCODE
    if ($racketExit -ne $rustExit) {
        throw "service suite exit code differs: Racket=$racketExit Rust=$rustExit"
    }
    $racketJson = ($racketOutput -join "`n") |
        ConvertFrom-Json |
        ConvertTo-Json -Depth 100 -Compress
    $rustJson = ($rustOutput -join "`n") |
        ConvertFrom-Json |
        ConvertTo-Json -Depth 100 -Compress
    if ($racketJson -ne $rustJson) {
        throw "service scenario report differs"
    }
    Write-Output "ok - task service report parity: 11 scenarios"

    $initialProgram = "examples\discount\v1.ail"
    $candidateProgram = "examples\discount\v2.ail"
    $racketOutput = @(& $racketExe $racketCli version-conformance $initialProgram $candidateProgram)
    $racketExit = $LASTEXITCODE
    $rustOutput = @(& $rustExe version-conformance $initialProgram $candidateProgram)
    $rustExit = $LASTEXITCODE
    if ($racketExit -ne $rustExit) {
        throw "version-store scenario exit code differs: Racket=$racketExit Rust=$rustExit"
    }
    $racketJson = ($racketOutput -join "`n") |
        ConvertFrom-Json |
        ConvertTo-Json -Depth 100 -Compress
    $rustJson = ($rustOutput -join "`n") |
        ConvertFrom-Json |
        ConvertTo-Json -Depth 100 -Compress
    if ($racketJson -ne $rustJson) {
        throw "version-store scenario report differs"
    }
    Write-Output "ok - version-store lifecycle report parity"
    exit 0
}
finally {
    Pop-Location
}
