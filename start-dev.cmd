@echo off
setlocal
set "CARGO_HOME=D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\cargo"
set "RUSTUP_HOME=D:\Ai-Files\Tools\BuildEnv\sfx-tools-rust\rustup"
set "PATH=%CARGO_HOME%\bin;%PATH%"
cd /d "%~dp0"
title SFX Tools - dev (source)
call npm run tauri dev