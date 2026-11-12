$ErrorActionPreference='Stop'
$petRoot=Split-Path $PSScriptRoot
$petExe=Join-Path $petRoot 'CodexPet.exe'
if(!(Test-Path -LiteralPath $petExe)){throw 'CodexPet.exe was not found.'}
Start-Process -FilePath $petExe -ArgumentList '--disable-autostart' -WorkingDirectory $petRoot -WindowStyle Hidden -Wait
Write-Output 'Codex desktop pet auto-follow is disabled.'
