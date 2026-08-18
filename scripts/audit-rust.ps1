$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

if (-not (Get-Command cargo-deny -ErrorAction SilentlyContinue)) {
    throw "cargo-deny is required; install it with: cargo install cargo-deny --locked"
}

Push-Location $projectRoot
try {
    cargo deny check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $metadata = cargo metadata --format-version 1 --locked | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $workspaceIds = [System.Collections.Generic.HashSet[string]]::new(
        [string[]]$metadata.workspace_members
    )
    $reachableIds = [System.Collections.Generic.HashSet[string]]::new(
        [string[]]$metadata.workspace_members
    )
    do {
        $changed = $false
        foreach ($node in $metadata.resolve.nodes) {
            if (-not $reachableIds.Contains([string]$node.id)) { continue }
            foreach ($dependency in $node.deps) {
                $activeKinds = @(
                    $dependency.dep_kinds |
                        Where-Object { $_.target -ne "cfg(any())" }
                )
                if ($activeKinds.Count -eq 0) { continue }
                if ($reachableIds.Add([string]$dependency.pkg)) { $changed = $true }
            }
        }
    } while ($changed)
    $unsafePattern = '^\s*(pub\s+)?unsafe\s+(fn|impl|trait)|unsafe\s*\{'

    Write-Output "Dependency unsafe implementation inventory:"
    foreach ($package in ($metadata.packages | Sort-Object name)) {
        if ($workspaceIds.Contains([string]$package.id) -or
            -not $reachableIds.Contains([string]$package.id)) { continue }
        $sourceRoot = Split-Path -Parent $package.manifest_path
        $matches = @(
            Get-ChildItem -LiteralPath $sourceRoot -Recurse -Filter *.rs -File |
                Select-String -Pattern $unsafePattern
        )
        Write-Output ("  {0}@{1}: {2} matching lines" -f `
            $package.name, $package.version, $matches.Count)
    }
}
finally {
    Pop-Location
}
