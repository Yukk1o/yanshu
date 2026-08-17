$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$racketExe = Join-Path $projectRoot ".toolchains\racket\Racket.exe"

if (-not (Test-Path -LiteralPath $racketExe)) {
    & (Join-Path $PSScriptRoot "bootstrap.ps1")
}

Push-Location $projectRoot
try {
    & $racketExe "src\cli.rkt" demo
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
