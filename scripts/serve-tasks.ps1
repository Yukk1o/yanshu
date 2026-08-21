$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$program = Join-Path $projectRoot "examples\tasks\service.yan"
$scenarios = Join-Path $projectRoot "examples\tasks\scenarios.json"
$runtimeDirectory = Join-Path $projectRoot ".runtime\tasks"
$codeStore = Join-Path $runtimeDirectory "code"
$dataStore = Join-Path $runtimeDirectory "store.json"
$bindAddress = if ([string]::IsNullOrWhiteSpace($env:YANSHU_HTTP_BIND)) {
    "127.0.0.1:8081"
}
else {
    $env:YANSHU_HTTP_BIND
}

New-Item -ItemType Directory -Path $runtimeDirectory -Force | Out-Null

Push-Location $projectRoot
try {
    cargo run --quiet --locked -p yanshu-cli -- deploy-service $program $scenarios $codeStore
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    cargo run --quiet --locked -p yanshu-server -- $codeStore $bindAddress $dataStore
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
