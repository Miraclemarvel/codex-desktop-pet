$ErrorActionPreference='Stop'
$assets=Join-Path (Split-Path $PSScriptRoot) 'assets'
$upstreamCommit=(Get-Content (Join-Path $assets 'SOURCE.json') -Raw | ConvertFrom-Json).commit
$all=@((Get-Content (Join-Path $assets 'upstream-manifest.json') -Raw | ConvertFrom-Json).animations)+@((Get-Content (Join-Path $assets 'upstream-actions.json') -Raw | ConvertFrom-Json).animations)
$items=$all | Sort-Object id -Unique
New-Item -ItemType Directory -Path (Join-Path $assets 'spritesheet') -Force | Out-Null
$items | ForEach-Object -Parallel {
 $root=$using:assets
 $commit=$using:upstreamCommit
 $target=Join-Path $root $_.sources.webp
 if(!(Test-Path -LiteralPath $target)) {
  $url='https://raw.githubusercontent.com/hunmer/aboutme-pets/'+$commit+'/pets/gpt/'+(($_.sources.webp -split '/' | ForEach-Object {[uri]::EscapeDataString($_)}) -join '/')
  for($attempt=0;$attempt -lt 3;$attempt++) {
   try {Invoke-WebRequest -Uri $url -OutFile "$target.part"; Move-Item -LiteralPath "$target.part" -Destination $target -Force; break} catch {if($attempt -eq 2){throw};Start-Sleep -Seconds 1}
  }
 }
 [pscustomobject]@{id=$_.id;bytes=(Get-Item -LiteralPath $target).Length}
} -ThrottleLimit 6 | Export-Csv -LiteralPath (Join-Path $assets 'downloads.csv') -NoTypeInformation -Encoding utf8
Write-Output "Downloaded $($items.Count) animations"
