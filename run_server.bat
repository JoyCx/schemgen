@echo off
setlocal enabledelayedexpansion
title SchemGen2 Server

echo ============================================================
echo   SchemGen2 — GLB to Litematica Converter
echo   Web app: Rust backend ^(Actix-web^) + Vite React frontend
echo   For the CLI run: backend\target\release\schemgen2.exe help
echo ============================================================
echo.

cd /d "%~dp0"

:: ── Check Python (required for trimesh voxelization) ──────────
echo [1/4] Checking Python...
where python >nul 2>nul
if !errorlevel! neq 0 (
    echo [ERROR] python not found in PATH. Please install Python 3.
    goto :fail
)
for /f "tokens=2 delims= " %%v in ('python --version 2^>^&1') do echo   Python %%v

python -c "import trimesh; import numpy; import scipy" >nul 2>nul
if !errorlevel! neq 0 (
    echo [WARN ] trimesh/numpy/scipy not installed. Installing...
    pip install trimesh numpy scipy pillow --quiet
    if !errorlevel! neq 0 (
        echo [ERROR] Failed to install Python dependencies.
        goto :fail
    )
)
echo   Python deps: OK

:: ── Check Rust backend binary ─────────────────────────────────
echo.
echo [2/4] Checking Rust backend...
set "EXE=backend\target\release\schemgen2.exe"
if not exist "!EXE!" (
    echo [WARN ] Release binary not found. Building...
    cd backend
    cargo build --release
    cd ..
    if not exist "!EXE!" (
        echo [ERROR] Failed to build backend.
        goto :fail
    )
)
echo   Backend binary: OK

:: ── Check Node.js / Frontend ──────────────────────────────────
echo.
echo [3/4] Checking frontend...
where node >nul 2>nul
if !errorlevel! neq 0 (
    echo [ERROR] Node.js not found. Please install Node.js.
    goto :fail
)
for /f "tokens=*" %%v in ('node -v 2^>^&1') do echo   Node %%v

if not exist "frontend\node_modules" (
    echo [INFO ] Installing frontend dependencies...
    cd frontend
    call npm install
    cd ..
    if not exist "frontend\node_modules" (
        echo [ERROR] npm install failed.
        goto :fail
    )
)
echo   Frontend deps: OK

:: ── Start servers ─────────────────────────────────────────────
echo.
echo [4/4] Starting servers...

:: Kill any existing instances
taskkill /f /im schemgen2.exe >nul 2>nul
taskkill /f /im node.exe /fi "WINDOWTITLE eq SchemGen2*" >nul 2>nul

:: Start backend
echo   Starting backend on http://localhost:3001 ...
start "SchemGen2 Backend" /MIN cmd /c "cd /d %cd%\backend && target\release\schemgen2.exe serve"

:: Wait a moment for backend to start
timeout /t 2 /nobreak >nul

:: Start frontend dev server
echo   Starting frontend on http://localhost:5173 ...
start "SchemGen2 Frontend" /MIN cmd /c "cd /d %cd%\frontend && npx vite --host"

timeout /t 3 /nobreak >nul

echo.
echo ============================================================
echo   SchemGen2 is running!
echo.
echo   Frontend:  http://localhost:5173
echo   Backend:   http://localhost:3001
echo.
echo   Drop a .glb file in the browser to convert it.
echo ============================================================
echo.

:: Open browser
start http://localhost:5173

echo Press any key to stop both servers...
pause >nul

:: Cleanup
echo Stopping servers...
taskkill /f /im schemgen2.exe >nul 2>nul
echo Done.
goto :eof

:fail
echo.
echo ============================================================
echo   Setup failed. See errors above.
echo ============================================================
pause >nul
exit /b 1
