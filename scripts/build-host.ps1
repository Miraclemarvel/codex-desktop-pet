param([string]$OutputPath)
$ErrorActionPreference='Stop'
$petRoot=Split-Path $PSScriptRoot
if(!$OutputPath){$OutputPath=Join-Path $petRoot 'native\CodexPetHost.exe'}
$compiler=Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if(!(Test-Path -LiteralPath $compiler)){throw '.NET Framework C# compiler was not found.'}
& $compiler /nologo /codepage:65001 /optimize+ /target:winexe /platform:x64 /reference:System.Management.dll "/win32icon:$petRoot\assets\tray.ico" "/out:$OutputPath" (Join-Path $petRoot 'native\CodexPetHost.cs')
if($LASTEXITCODE -ne 0){throw 'Native host compilation failed.'}
Write-Output $OutputPath
