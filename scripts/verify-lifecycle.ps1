param([switch]$ManualExit)
$ErrorActionPreference='Stop'
$petRoot=Split-Path $PSScriptRoot
$release=Join-Path $petRoot 'outputs\release\CodexPet'
$exe=Join-Path $release 'CodexPet.exe'
$hostExe=Join-Path $release 'CodexPetHost.exe'
$session='qa-'+[guid]::NewGuid().ToString('N')
$evidence=Join-Path $petRoot ("outputs\lifecycle-qa\"+$session)
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$statePath=Join-Path $evidence 'host-state.json'
$testData=Join-Path $evidence 'profile'
$mockOwner=$null
$watcher=$null
$childPid=$null
try {
 $mockOwner=Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60') -WindowStyle Hidden -PassThru
 $watcher=Start-Process -FilePath $hostExe -ArgumentList @('--session',$session,'--codex-pid',$mockOwner.Id,'--pet-exe',('"'+$exe+'"'),'--pet-user-data',('"'+$testData+'"'),'--state-path',('"'+$statePath+'"')) -WindowStyle Hidden -PassThru
 $deadline=[DateTime]::UtcNow.AddSeconds(15)
 do {
  if(Test-Path -LiteralPath $statePath){try{$state=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json}catch{}}
  if($state.status -eq 'running' -and $state.petPid){break}
  Start-Sleep -Milliseconds 200
 }while([DateTime]::UtcNow -lt $deadline)
 if($state.status -ne 'running' -or !$state.petPid){throw 'The pet was not started for the test owner.'}
 $childPid=[int]$state.petPid
 # Give the isolated Electron main process time to enter its parent monitor.
 Start-Sleep -Seconds 3
 $started=Get-Process -Id $childPid -ErrorAction Stop
 if($ManualExit){
  Start-Process -FilePath $exe -ArgumentList @('--test-user-data',('"'+$testData+'"'),'--quit') -WindowStyle Hidden -Wait
  Start-Sleep -Seconds 3
  $state=Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
  if($state.status -ne 'dismissed' -or $state.petPid){throw 'A manually exited pet was reopened during the same owner session.'}
 }
 Stop-Process -Id $mockOwner.Id -ErrorAction Stop
 $deadline=[DateTime]::UtcNow.AddSeconds(8)
 do {
  $alive=Get-Process -Id $childPid -ErrorAction SilentlyContinue
  if(!$alive){break}
  Start-Sleep -Milliseconds 200
 }while([DateTime]::UtcNow -lt $deadline)
 if($alive){throw 'The pet stayed alive after its owner exited.'}
 $watcher.WaitForExit(5000) | Out-Null
 if(!$watcher.HasExited){throw 'The isolated test watcher did not finish.'}
 $result=[pscustomobject]@{ownerPid=$mockOwner.Id;petPid=$childPid;startedWithOwner=$true;exitedWithOwner=$true;manualExitRespected=[bool]$ManualExit;isolatedProfile=$testData;codexWasNotClosed=$true}
 $result | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'result.json') -Encoding utf8
 $result | ConvertTo-Json
} finally {
 if($mockOwner){Stop-Process -Id $mockOwner.Id -ErrorAction SilentlyContinue}
 if($watcher -and !$watcher.HasExited){Start-Process -FilePath $hostExe -ArgumentList @('--stop','--session',$session) -WindowStyle Hidden -Wait; $watcher.WaitForExit(5000) | Out-Null}
 if($childPid){Stop-Process -Id $childPid -ErrorAction SilentlyContinue}
}
