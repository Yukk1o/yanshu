$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$toolchainRoot = Join-Path $projectRoot ".toolchains"
$archive = Join-Path $toolchainRoot "racket-minimal-9.3-x86_64-win32.tgz"
$racketExe = Join-Path $toolchainRoot "racket\Racket.exe"
$downloadUrl = "https://download.racket-lang.org/releases/9.3/installers/racket-minimal-9.3-x86_64-win32.tgz"
$expectedSha256 = "eebeb6b02056bcc4196fe5fe843f8412402f247bae5fe297a1adcbc09edec471"

if (Test-Path -LiteralPath $racketExe) {
    & $racketExe --version
    exit 0
}

New-Item -ItemType Directory -Path $toolchainRoot -Force | Out-Null

if (-not (Test-Path -LiteralPath $archive)) {
    Invoke-WebRequest -UseBasicParsing -Uri $downloadUrl -OutFile $archive
}

$actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
if ($actualSha256 -ne $expectedSha256) {
    throw "Racket archive checksum mismatch: $actualSha256"
}

tar -xzf $archive -C $toolchainRoot
& $racketExe --version

