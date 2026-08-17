$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$racketExe = Join-Path $projectRoot ".toolchains\racket\Racket.exe"
$program = Join-Path $projectRoot "examples\discount\v1.ail"
$tests = Join-Path $projectRoot "examples\discount\tests.json"
$cli = Join-Path $projectRoot "src\cli.rkt"
$promptedKey = $false

if (-not (Test-Path -LiteralPath $racketExe)) {
    throw "Racket is not installed. Run scripts\bootstrap.ps1 first."
}

if ([string]::IsNullOrWhiteSpace($env:AI_EVOLVE_PROVIDER)) {
    $env:AI_EVOLVE_PROVIDER = "deepseek-chat"
}
if ([string]::IsNullOrWhiteSpace($env:AI_EVOLVE_BASE_URL)) {
    $env:AI_EVOLVE_BASE_URL = "https://api.deepseek.com"
}
if ([string]::IsNullOrWhiteSpace($env:AI_EVOLVE_MODEL)) {
    $env:AI_EVOLVE_MODEL = "deepseek-v4-flash"
}
if ([string]::IsNullOrWhiteSpace($env:AI_EVOLVE_REASONING_EFFORT)) {
    $env:AI_EVOLVE_REASONING_EFFORT = "high"
}

if ([string]::IsNullOrWhiteSpace($env:AI_EVOLVE_API_KEY) -and
    [string]::IsNullOrWhiteSpace($env:DEEPSEEK_API_KEY) -and
    [string]::IsNullOrWhiteSpace($env:OPENAI_API_KEY)) {
    $secureKey = Read-Host "DeepSeek API key" -AsSecureString
    $keyPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureKey)
    try {
        $env:AI_EVOLVE_API_KEY = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($keyPointer)
        $promptedKey = $true
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($keyPointer)
    }
}

$exitCode = 1
try {
    & $racketExe $cli evolve $program $tests --promote
    $exitCode = $LASTEXITCODE
}
finally {
    if ($promptedKey) {
        Remove-Item Env:AI_EVOLVE_API_KEY -ErrorAction SilentlyContinue
    }
}
exit $exitCode
