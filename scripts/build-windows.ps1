# Build stillshot.exe and the per-user NSIS installer. Run on Windows.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

cargo build --release
New-Item -ItemType Directory -Force -Path dist | Out-Null
Copy-Item target\release\stillshot.exe dist\stillshot.exe -Force

function Find-Makensis {
  $cmd = Get-Command makensis -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  foreach ($dir in @(
      ${env:ProgramFiles(x86)},
      $env:ProgramFiles
    )) {
    if (-not $dir) { continue }
    $candidate = Join-Path $dir "NSIS\makensis.exe"
    if (Test-Path $candidate) { return $candidate }
  }
  return $null
}

$makensis = Find-Makensis
if (-not $makensis) {
  Write-Host "makensis was not found. Install NSIS (choco install nsis) to build Stillshot-Setup.exe."
  Write-Host "The portable exe is at dist\stillshot.exe"
  exit 0
}

# /NOCD makes File and OutFile paths relative to the repo root, which is how
# installer.nsi is written (target\release\... and dist\...).
& $makensis /NOCD "packaging\windows\installer.nsi"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if (-not (Test-Path "dist\Stillshot-Setup.exe")) {
  Write-Error "makensis exited 0 but dist\Stillshot-Setup.exe is missing"
  exit 1
}
Write-Host "Built dist\stillshot.exe and dist\Stillshot-Setup.exe"
