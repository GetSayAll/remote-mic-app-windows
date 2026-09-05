@echo off
rem 带按键映射诊断日志启动无线麦（排查"返回→退格未生效"类问题用）。
rem 日志写入 %USERPROFILE%\sayall-diag.log，测试完把该文件发给开发即可。
rem 注意：需先退出正在运行的无麦（单实例守卫会拦截第二个实例）。
set SAYALL_GATT_LOG=%USERPROFILE%\sayall-diag.log
start "" "%LOCALAPPDATA%\无线麦 SayAll\sayall-windows-app.exe"
echo 已启动（诊断日志：%%USERPROFILE%%\sayall-diag.log）
