@echo off
title hostrun debug
cd /d "%~dp0"



call corepack.cmd enable
echo corepack enable exit code: %errorlevel%

call corepack.cmd pnpm install
echo pnpm install exit code: %errorlevel%

call corepack.cmd pnpm tauri dev
echo tauri dev exit code: %errorlevel%
pause

