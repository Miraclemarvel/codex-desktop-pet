$ErrorActionPreference='Stop'
$petRoot=Split-Path $PSScriptRoot
$exe=Join-Path $petRoot 'rust/target/release/CodexPetRust.exe'
$evidence=Join-Path $petRoot ('outputs/rust-lifecycle/'+[guid]::NewGuid().ToString('N'))
$results=@()
foreach($manualExit in @($false,$true)){
 $profile=Join-Path $evidence $(if($manualExit){'manual'}else{'owner-exit'})
 New-Item -ItemType Directory -Path $profile -Force | Out-Null
 $stateFile=Join-Path $profile 'host-state.json'
 $owner=$null;$watcher=$null;$petPid=$null
 try{
  $owner=Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60') -WindowStyle Hidden -PassThru
  $watcher=Start-Process -FilePath $exe -ArgumentList @('--watch','--demo','--profile',('"'+$profile+'"'),'--codex-pid',$owner.Id) -WindowStyle Hidden -PassThru
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{
   $state=$null
   if(Test-Path -LiteralPath $stateFile){try{$state=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json}catch{}}
   if($state.status -eq 'running' -and $state.petPid){break}
   Start-Sleep -Milliseconds 200
  }while([DateTime]::UtcNow -lt $deadline)
  if($state.status -ne 'running' -or !$state.petPid){throw 'Native pet did not start with its isolated owner.'}
  $petPid=[int]$state.petPid
  Start-Sleep -Seconds 2
  if($manualExit){
   $quit=Start-Process -FilePath $exe -ArgumentList @('--quit','--profile',('"'+$profile+'"')) -WindowStyle Hidden -PassThru
   if(!$quit.WaitForExit(5000)){throw 'Native quit command timed out.'}
   $deadline=[DateTime]::UtcNow.AddSeconds(8)
   do{
    $state=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
    if($state.status -eq 'dismissed'){break}
    Start-Sleep -Milliseconds 200
   }while([DateTime]::UtcNow -lt $deadline)
   if($state.status -ne 'dismissed'){throw 'Manual exit was not respected for this Codex session.'}
   Start-Sleep -Seconds 2
   if(Get-Process -Id $petPid -ErrorAction SilentlyContinue){throw 'Native pet reopened after a manual exit.'}
  }
  Stop-Process -Id $owner.Id
  $deadline=[DateTime]::UtcNow.AddSeconds(8)
  do{
   if(!(Get-Process -Id $petPid -ErrorAction SilentlyContinue)){break}
   Start-Sleep -Milliseconds 200
  }while([DateTime]::UtcNow -lt $deadline)
  if(Get-Process -Id $petPid -ErrorAction SilentlyContinue){throw 'Native pet remained after its owner exited.'}
  if(!$watcher.WaitForExit(5000)){throw 'Native watcher did not exit after its fixed test owner ended.'}
  $results+=[pscustomobject]@{startedWithOwner=$true;exitedWithOwner=$true;manualExitRespected=$(if($manualExit){$true}else{$null});realCodexWasNotClosed=$true}
 }finally{
  if($owner){Stop-Process -Id $owner.Id -ErrorAction SilentlyContinue}
  if($watcher -and !$watcher.HasExited){Stop-Process -Id $watcher.Id -ErrorAction SilentlyContinue}
  if($petPid){Stop-Process -Id $petPid -ErrorAction SilentlyContinue}
 }
}
$results | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'result.json') -Encoding utf8
$results | ConvertTo-Json
