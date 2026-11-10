param([switch]$Once)

# 只读观察当前项目的 Rust 1.99.1 下载，不启动或中断安装。
$downloadRoot = 'D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/rustup/downloads'
$hash = '7da83c2af1f209606b0160f25174d2867c7c7b73667c32c9e5fbcef4f5f45348'
$total = 77075164L
$compiler = 'D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustc.exe'
$lastBytes = $null
$lastTime = Get-Date
Write-Host 'Rust 编译器下载进度（每秒刷新，Ctrl+C 退出）' -ForegroundColor Cyan
Write-Host '关闭此窗口不会停止安装。100% 仅表示下载完成，之后还需解压安装。'
Write-Host ''

while ($true) {
    $now = Get-Date
    $partial = Get-Item -LiteralPath (Join-Path $downloadRoot "$hash.partial") -ErrorAction SilentlyContinue
    $complete = Get-Item -LiteralPath (Join-Path $downloadRoot $hash) -ErrorAction SilentlyContinue
    $hasCompiler = Test-Path -LiteralPath $compiler
    $installer = @(Get-Process -Name rustup-init -ErrorAction SilentlyContinue).Count -gt 0
    $bytes = 0L
    $known = $false
    if ($partial) { $bytes = $partial.Length; $known = $true }
    elseif ($complete) { $bytes = $complete.Length; $known = $true }
    elseif ($hasCompiler) { $bytes = $total; $known = $true }

    $speed = '--'
    if ($null -ne $lastBytes -and $known) {
        $seconds = [Math]::Max(0.001, ($now - $lastTime).TotalSeconds)
        $speed = '{0:N1} KB/s' -f ([Math]::Max(0, $bytes - $lastBytes) / $seconds / 1KB)
    }
    $percent = [Math]::Min(100, 100.0 * $bytes / $total)
    $filled = [int][Math]::Floor($percent / 5)
    $bar = ('#' * $filled) + ('-' * (20 - $filled))
    if ($known) {
        $line = '[{0}] {1,5:N1}%  {2:N1}/{3:N1} MB  速度 {4}' -f $bar, $percent, ($bytes / 1MB), ($total / 1MB), $speed
    } else {
        $line = '暂未找到下载缓存，等待安装器写入…'
    }
    if ($hasCompiler) { $line += ' | 编译器已落盘' }
    elseif ($complete) { $line += ' | 下载完成，等待解压' }
    elseif (!$installer) { $line += ' | 安装器未运行' }
    Write-Host ("`r" + $line.PadRight(120)) -NoNewline

    if ($Once) { Write-Host ''; break }
    if ($hasCompiler -and !$installer) {
        Write-Host ''
        $version = & $compiler --version 2>&1
        if ($LASTEXITCODE -eq 0) { Write-Host "编译器可运行：$version" -ForegroundColor Green }
        else { Write-Host "编译器已落盘，但运行检查失败：$version" -ForegroundColor Yellow }
        break
    }
    if (!$installer) {
        Write-Host ''
        Write-Host '安装器已停止，尚未确认编译器可用；请查看安装结果。' -ForegroundColor Yellow
        break
    }
    if ($known) { $lastBytes = $bytes } else { $lastBytes = $null }
    $lastTime = $now
    Start-Sleep -Seconds 1
}