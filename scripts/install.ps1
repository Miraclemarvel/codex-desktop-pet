$ErrorActionPreference='Stop'
$petRoot=Split-Path $PSScriptRoot
$petExe=Join-Path $petRoot 'CodexPet.exe'
$hostExe=Join-Path $petRoot 'CodexPetHost.exe'
if(!(Test-Path -LiteralPath $petExe)){throw 'Please run this script from the extracted CodexPet release folder.'}
if(!(Test-Path -LiteralPath $hostExe)){throw 'CodexPetHost.exe was not found.'}
Start-Process -FilePath $hostExe -ArgumentList '--stop' -WindowStyle Hidden -Wait
Start-Process -FilePath $petExe -ArgumentList '--quit' -WindowStyle Hidden -Wait
Start-Process -FilePath $petExe -ArgumentList '--install-config' -WorkingDirectory $petRoot -WindowStyle Hidden -Wait
$shell=New-Object -ComObject WScript.Shell
$shortcut=$shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Desktop')) 'Codex 白龙桌宠.lnk'))
$shortcut.TargetPath=$petExe
$shortcut.WorkingDirectory=$petRoot
$shortcut.IconLocation=$petExe
$shortcut.Description='Codex 白龙桌宠 · 点击查看剩余额度'
$shortcut.Save()
Start-Process -FilePath $hostExe -WorkingDirectory $petRoot -WindowStyle Hidden
Write-Output 'Codex desktop pet installed. Keep this folder in place.'
