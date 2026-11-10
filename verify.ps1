param([ValidateSet('check','test','build','dev')][string]$Action='check')
$ErrorActionPreference='Stop'
$env:CARGO_HOME='D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/cargo'
$env:RUSTUP_HOME='D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/rustup'
$env:PATH="$env:CARGO_HOME/bin;$env:PATH"
$env:RUSTUP_TOOLCHAIN='stable-x86_64-pc-windows-msvc'
$env:RUSTUP_AUTO_INSTALL='0'
Set-Location $PSScriptRoot
$rustc="$env:CARGO_HOME/bin/rustc.exe"
$cargo="$env:CARGO_HOME/bin/cargo.exe"
if(!(Test-Path $rustc) -or !(Test-Path $cargo)){throw 'Rust安装未完成。'}
$version=& $rustc --version --verbose 2>&1
if($LASTEXITCODE -ne 0){throw "Rust工具链尚不可用：$version"}
if(($version -notcontains 'release: 1.99.1') -or ($version -notcontains 'host: x86_64-pc-windows-msvc')){
    throw "工具链与固定版本不一致，停止构建：$version"
}
& $cargo --version
if($LASTEXITCODE -ne 0){throw 'Cargo不可用'}
if($Action -eq 'check'){
    npm run build
    if($LASTEXITCODE -ne 0){throw 'Frontend build failed'}
    & $cargo check --manifest-path src-tauri/Cargo.toml
}elseif($Action -eq 'test'){
    & $cargo test --manifest-path src-tauri/Cargo.toml
}elseif($Action -eq 'build'){
    # 人类已授权：只结束核对为本工程EXE的进程，其他应用不在范围内。
    $workbenchExe=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'src-tauri/target/debug/sfx-tools.exe'))
    Get-Process sfx-tools -ErrorAction SilentlyContinue | Where-Object {[string]::Equals($_.Path,$workbenchExe,[StringComparison]::OrdinalIgnoreCase)} | Stop-Process -Force -ErrorAction Stop
    $runtimeFfmpeg=Get-Command ffmpeg -ErrorAction SilentlyContinue
    if($runtimeFfmpeg){$env:SW_FFMPEG_PATH=$runtimeFfmpeg.Source}
    npm run tauri -- build --debug --no-bundle
}else{
    npm run tauri -- dev
}
if($LASTEXITCODE -ne 0){throw "Validation failed: $LASTEXITCODE"}
