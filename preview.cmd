@echo off
setlocal
cd /d "%~dp0"
start "SFX Tools 预览服务" /min cmd /c "npm run dev"
powershell -NoProfile -ExecutionPolicy Bypass -Command "for($i=0;$i -lt 60;$i++){ try{ $c=New-Object Net.Sockets.TcpClient('127.0.0.1',5173); $c.Close(); break }catch{ Start-Sleep -Milliseconds 500 } }; Start-Process 'http://127.0.0.1:5173/?catalog'"