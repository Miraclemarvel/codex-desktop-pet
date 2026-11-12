param([switch]$SkipTests, [switch]$Offline, [string]$OutputDirectory)
$ErrorActionPreference='Stop'
$petRoot=Split-Path $PSScriptRoot
$cargoCommand=Get-Command cargo -ErrorAction SilentlyContinue
$cargoPath=if($cargoCommand){$cargoCommand.Source}else{Join-Path $petRoot 'outputs/toolchain/cargo/bin/cargo.exe'}
if(!(Test-Path -LiteralPath $cargoPath)){throw 'Install Rust stable for Windows MSVC, then run this script again.'}
if($cargoPath -like '*outputs*toolchain*cargo*'){
 $env:RUSTUP_HOME=Join-Path $petRoot 'outputs/toolchain/rustup'
 $env:CARGO_HOME=Join-Path $petRoot 'outputs/toolchain/cargo'
}
$previousFlags=$env:RUSTFLAGS
$previousEncodedFlags=$env:CARGO_ENCODED_RUSTFLAGS
$compilerFlags=@()
if($previousEncodedFlags){$compilerFlags+=@($previousEncodedFlags.Split([char]31))}
elseif($previousFlags){$compilerFlags+=@($previousFlags -split '\s+' | Where-Object {$_})}
$compilerFlags+=@('-C','target-feature=+crt-static')
$cargoHome=if($env:CARGO_HOME){$env:CARGO_HOME}else{Join-Path $env:USERPROFILE '.cargo'}
$rustupHome=if($env:RUSTUP_HOME){$env:RUSTUP_HOME}else{Join-Path $env:USERPROFILE '.rustup'}
foreach($mapping in @(@($petRoot,'/src/codex-desktop-pet'),@($cargoHome,'/cargo'),@($rustupHome,'/rustup'))){
 $compilerFlags+='--remap-path-prefix='+$mapping[0]+'='+$mapping[1]
 $compilerFlags+='--remap-path-prefix='+$mapping[0].Replace('\','/')+'='+$mapping[1]
}
# Unit separators preserve flags containing spaces in build directory paths.
$env:CARGO_ENCODED_RUSTFLAGS=$compilerFlags -join [char]31
[string[]]$offlineArgs=if($Offline){@('--offline')}else{@()}
Push-Location $petRoot
try{
 & $cargoPath fetch --manifest-path rust/Cargo.toml --locked @offlineArgs
 if($LASTEXITCODE -ne 0){throw 'Rust dependencies could not be fetched.'}
 python scripts/collect-rust-licenses.py --cargo $cargoPath
 if($LASTEXITCODE -ne 0){throw 'Rust license collection failed.'}
 python scripts/prepare-rust-assets.py
 if($LASTEXITCODE -ne 0){throw 'Rust artwork archive failed validation.'}
 if(!$SkipTests){
  & $cargoPath test --manifest-path rust/Cargo.toml --locked --release @offlineArgs
  if($LASTEXITCODE -ne 0){throw 'Rust tests failed.'}
 }
 & $cargoPath build --manifest-path rust/Cargo.toml --locked --release @offlineArgs
 if($LASTEXITCODE -ne 0){throw 'Rust compilation failed.'}
 $version=(Select-String -LiteralPath rust/Cargo.toml -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
 $name='CodexPet-Rust-Windows-x64-v'+$version+'.exe'
 $packageOutput=if($OutputDirectory){[IO.Path]::GetFullPath($OutputDirectory)}else{Join-Path $petRoot 'outputs'}
 New-Item -ItemType Directory -Path $packageOutput -Force | Out-Null
 $destination=Join-Path $packageOutput $name
 Copy-Item -LiteralPath rust/target/release/CodexPetRust.exe -Destination $destination -Force
 python scripts/set-exe-icon.py $destination assets/app.ico
 if($LASTEXITCODE -ne 0){throw 'Rust application icon embedding failed.'}
 $digest=(Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
 [IO.File]::WriteAllText(($destination+'.sha256'),($digest+'  '+$name+"`n"),[Text.Encoding]::ASCII)
 [pscustomobject]@{path=$destination;bytes=(Get-Item -LiteralPath $destination).Length;sha256=$digest;rustSourceFiles=1;animations=92} | ConvertTo-Json
}finally{
 Pop-Location
 $env:CARGO_ENCODED_RUSTFLAGS=$previousEncodedFlags
}
