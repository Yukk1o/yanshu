$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$racketExe = Join-Path $projectRoot ".toolchains\racket\Racket.exe"

if (-not (Test-Path -LiteralPath $racketExe)) {
    & (Join-Path $PSScriptRoot "bootstrap.ps1")
}

& $racketExe (Join-Path $projectRoot "tests\all.rkt")
exit $LASTEXITCODE

