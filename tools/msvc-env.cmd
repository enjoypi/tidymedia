@echo off
rem msvc-env.cmd - source vcvars64 + backfill UCRT INCLUDE/LIB + RUSTFLAGS /LIBPATH
rem
rem Fixes two local pitfalls (VS 18 / SDK 10.0.28000.0 verified):
rem   1. vcvars64's INCLUDE/LIB miss Windows Kits UCRT paths
rem      -> cl.exe cannot find stddef.h (C1083) when cc-rs builds zstd-sys
rem   2. cc-rs windows_registry cannot find the new VS -> rustc links with default LIB
rem      -> link.exe cannot find kernel32.lib (LNK1181) / ucrt.lib (LNK1104)
rem All paths discovered dynamically; no edit needed when MSVC/SDK version changes.
rem
rem NOTE: comments stay ASCII - cmd parses .cmd with the ANSI codepage (GBK here),
rem UTF-8 CJK bytes would corrupt parsing. Keep this file ASCII-only.
rem
rem Usage (call leaks set into caller; the && command after inherits the env):
rem   cmd //c "call tools\msvc-env.cmd && cargo build --release"

rem 1. discover vcvars64 (glob newest install; vswhere does not recognize VS 18 preview)
rem    powershell by full path: cmd launched from Git Bash may lack System32 in PATH
for /f "delims=" %%i in ('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -Command "(Get-ChildItem 'C:\Program Files*\Microsoft Visual Studio\*\*\VC\Auxiliary\Build\vcvars64.bat' -EA SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName"') do set "VCVARS=%%i"
if not defined VCVARS (echo error: vcvars64.bat not found - install VS C++ workload >&2 & exit /b 1)

rem 2. source vcvars64 (provides PATH with cl.exe, VCINSTALLDIR, VCToolsVersion)
call "%VCVARS%" >nul 2>&1

rem 3. discover SDK version (newest dir under Windows Kits\10\Lib)
for /f "delims=" %%i in ('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -Command "(Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory -EA SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1).Name"') do set "SDKVER=%%i"
if not defined SDKVER (echo error: Windows SDK not found >&2 & exit /b 1)

rem 4. force complete INCLUDE/LIB (override vcvars64's incomplete values, not append)
set "MSVCDIR=%VCINSTALLDIR%Tools\MSVC\%VCToolsVersion%"
set "SDKROOT=C:\Program Files (x86)\Windows Kits\10"
set "INCLUDE=%MSVCDIR%\include;%SDKROOT%\Include\%SDKVER%\ucrt;%SDKROOT%\Include\%SDKVER%\um;%SDKROOT%\Include\%SDKVER%\shared;%SDKROOT%\Include\%SDKVER%\winrt"
set "LIB=%MSVCDIR%\lib\x64;%SDKROOT%\Lib\%SDKVER%\um\x64;%SDKROOT%\Lib\%SDKVER%\ucrt\x64"

rem 5. RUSTFLAGS append /LIBPATH (covers pitfall 2: rustc links bypassing env LIB).
rem    short 8.3 paths avoid quote-parsing issues of spaces in space-separated RUSTFLAGS.
for %%i in ("%MSVCDIR%\lib\x64") do set "MSVCLIB=%%~si"
for %%i in ("%SDKROOT%\Lib\%SDKVER%\um\x64") do set "UMLIB=%%~si"
for %%i in ("%SDKROOT%\Lib\%SDKVER%\ucrt\x64") do set "UCRTLIB=%%~si"
set "RUSTFLAGS=-C link-arg=/LIBPATH:%MSVCLIB% -C link-arg=/LIBPATH:%UMLIB% -C link-arg=/LIBPATH:%UCRTLIB% %RUSTFLAGS%"
