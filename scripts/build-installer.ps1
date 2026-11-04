# Build the Windows installer with OAuth credentials stamped in at compile time.
# Credentials are read from .env.local so they never live in tracked files.
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$envFile = Join-Path $root '.env.local'
if (-not (Test-Path $envFile)) {
    throw "Missing $envFile - OAuth login cannot be stamped into the build."
}

$loaded = 0
foreach ($line in Get-Content $envFile) {
    $trimmed = $line.Trim()
    if ($trimmed -eq '' -or $trimmed.StartsWith('#')) { continue }
    $idx = $trimmed.IndexOf('=')
    if ($idx -lt 1) { continue }
    $key = $trimmed.Substring(0, $idx).Trim()
    $value = $trimmed.Substring($idx + 1).Trim().Trim('"')
    if ($key -like 'TERMIGO_ANTIGRAVITY_*' -or $key -like 'TERMIGO_CLAUDE_*') {
        [Environment]::SetEnvironmentVariable($key, $value, 'Process')
        $loaded++
    }
}
Write-Host "stamped-env-vars=$loaded"

Push-Location (Join-Path $root 'desktop')
try {
    & npm run tauri:build -- --bundles nsis
    $code = $LASTEXITCODE
} finally {
    Pop-Location
}
Write-Host "INSTALLER_BUILD_EXIT=$code"
exit $code
