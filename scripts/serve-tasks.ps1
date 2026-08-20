$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$racketExe = Join-Path $projectRoot ".toolchains\racket\Racket.exe"
$program = Join-Path $projectRoot "examples\tasks\service.yan"
$scenarios = Join-Path $projectRoot "examples\tasks\scenarios.json"
$runtimeDirectory = Join-Path $projectRoot ".runtime\tasks"
$codeStore = Join-Path $runtimeDirectory "code"
$store = Join-Path $runtimeDirectory "store.json"
$port = if ([string]::IsNullOrWhiteSpace($env:YANSHU_HTTP_PORT)) {
    "8080"
}
else {
    $env:YANSHU_HTTP_PORT
}

if (-not (Test-Path -LiteralPath $racketExe)) {
    & (Join-Path $PSScriptRoot "bootstrap.ps1")
}

New-Item -ItemType Directory -Path $runtimeDirectory -Force | Out-Null

Push-Location $projectRoot
try {
    & $racketExe "src\cli.rkt" deploy-service $program $scenarios $codeStore
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & $racketExe "src\cli.rkt" serve-active $codeStore $port $store
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
