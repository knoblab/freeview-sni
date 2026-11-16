@echo off
setlocal EnableDelayedExpansion
chcp 65001 >nul

REM ============================================================================
REM FreeView vs Direct Interleaved Benchmark (cloudflare-speed-cli)
REM Usage:
REM   benchmark_interleaved.bat                    (uses freeview.exe)
REM   benchmark_interleaved.bat freeview-3.2.1-test.exe
REM ============================================================================

set "ROUNDS=10"
set "PROXY_PORT=8080"
set "COOLDOWN_SEC=3"

set "SCRIPT_DIR=%~dp0"
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

if "%~1"=="" (
    set "FREEVIEW_EXE=%SCRIPT_DIR%\freeview.exe"
) else (
    if exist "%~1" (
        set "FREEVIEW_EXE=%~f1"
    ) else (
        set "FREEVIEW_EXE=%SCRIPT_DIR%\%~1"
    )
)

if not exist "%FREEVIEW_EXE%" (
    echo [ERROR] FreeView executable not found: "%FREEVIEW_EXE%"
    pause
    exit /b 1
)

for %%F in ("%FREEVIEW_EXE%") do set "FREEVIEW_NAME=%%~nxF"

REM 1. Locate or install cloudflare-speed-cli
set "CF_CLI="
for /f "delims=" %%C in ('where cloudflare-speed-cli 2^>nul') do (
    if not defined CF_CLI set "CF_CLI=%%C"
)
if not defined CF_CLI (
    if exist "%USERPROFILE%\.cargo\bin\cloudflare-speed-cli.exe" (
        set "CF_CLI=%USERPROFILE%\.cargo\bin\cloudflare-speed-cli.exe"
    )
)
if not defined CF_CLI (
    echo [INFO] cloudflare-speed-cli not found. Installing via cargo...
    where cargo >nul 2>&1
    if errorlevel 1 (
        echo [ERROR] Neither cloudflare-speed-cli nor cargo is installed.
        echo         Install from: https://github.com/kavehtehrani/cloudflare-speed-cli/releases
        pause
        exit /b 1
    )
    cargo install cloudflare-speed-cli
    if errorlevel 1 (
        echo [ERROR] Failed to install cloudflare-speed-cli.
        pause
        exit /b 1
    )
    set "CF_CLI=%USERPROFILE%\.cargo\bin\cloudflare-speed-cli.exe"
)

if not exist "%CF_CLI%" (
    echo [ERROR] cloudflare-speed-cli executable missing: "%CF_CLI%"
    pause
    exit /b 1
)

REM 2. Create timestamped output directory
for /f "delims=" %%T in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd_HHmmss"') do set "STAMP=%%T"
set "OUT_DIR=%SCRIPT_DIR%\bench_results\%STAMP%"
mkdir "%OUT_DIR%" >nul 2>&1

echo ============================================================================
echo  Interleaved Benchmark: %FREEVIEW_NAME% vs Direct ^(None^)
echo  Rounds     : %ROUNDS% pairs ^(%ROUNDS% FreeView + %ROUNDS% Direct^)
echo  CLI        : "%CF_CLI%"
echo  FreeView   : "%FREEVIEW_EXE%"
echo  Output Dir : "%OUT_DIR%"
echo ============================================================================
echo.

REM Ensure clean initial state before round 1
call :stop_freeview

for /L %%i in (1,1,%ROUNDS%) do (
    echo ----------------------------------------------------------------------------
    echo [Round %%i/%ROUNDS%] Phase A: FreeView ON ^(%FREEVIEW_NAME%^)
    echo ----------------------------------------------------------------------------

    call :start_freeview
    if errorlevel 1 (
        echo [ERROR] Failed to start FreeView on port %PROXY_PORT%.
        call :stop_freeview
        exit /b 1
    )

    "%CF_CLI%" --text --auto-save false --proxy "http://127.0.0.1:%PROXY_PORT%" --comments "freeview_%%i" --export-csv "%OUT_DIR%\freeview_%%i.csv" --export-json "%OUT_DIR%\freeview_%%i.json"
    if errorlevel 1 echo [WARN] Round %%i FreeView run returned non-zero exit code.

    call :stop_freeview
    timeout /t %COOLDOWN_SEC% /nobreak >nul

    echo.
    echo ----------------------------------------------------------------------------
    echo [Round %%i/%ROUNDS%] Phase B: Direct ^(None / No Proxy^)
    echo ----------------------------------------------------------------------------

    set "HTTP_PROXY="
    set "HTTPS_PROXY="
    set "ALL_PROXY="
    set "http_proxy="
    set "https_proxy="
    set "all_proxy="

    "%CF_CLI%" --text --auto-save false --comments "none_%%i" --export-csv "%OUT_DIR%\none_%%i.csv" --export-json "%OUT_DIR%\none_%%i.json"
    if errorlevel 1 echo [WARN] Round %%i Direct run returned non-zero exit code.

    if %%i LSS %ROUNDS% (
        timeout /t %COOLDOWN_SEC% /nobreak >nul
    )
    echo.
)

REM 3. Merge CSVs and print summary comparison
echo ============================================================================
echo  Generating Summary Report...
echo ============================================================================

powershell -NoProfile -Command ^
  "$outDir = '%OUT_DIR%';" ^
  "$fvFiles = @(Get-ChildItem -Path $outDir -Filter 'freeview_*.csv' | Sort-Object { [int]($_.BaseName -replace 'freeview_','') });" ^
  "$noFiles = @(Get-ChildItem -Path $outDir -Filter 'none_*.csv' | Sort-Object { [int]($_.BaseName -replace 'none_','') });" ^
  "$rows = @();" ^
  "foreach ($f in ($fvFiles + $noFiles)) { $rows += Import-Csv $f.FullName };" ^
  "if ($rows.Count -eq 0) { Write-Host 'No CSV rows found.'; exit 1 };" ^
  "$mergedPath = Join-Path $outDir 'all_runs.csv';" ^
  "$rows | Export-Csv -Path $mergedPath -NoTypeInformation -Encoding UTF8;" ^
  "function Get-Stats($items, $prop) {" ^
  "  $vals = @($items | ForEach-Object { $v = 0.0; if ([double]::TryParse($_.$prop, [ref]$v)) { $v } } | Sort-Object);" ^
  "  if ($vals.Count -eq 0) { return 'N/A' };" ^
  "  $mean = ($vals | Measure-Object -Average).Average;" ^
  "  $mid = [int][Math]::Floor($vals.Count / 2);" ^
  "  $med = if ($vals.Count %% 2 -eq 1) { $vals[$mid] } else { ($vals[$mid-1] + $vals[$mid]) / 2.0 };" ^
  "  return ('{0:N2} / {1:N2}' -f $mean, $med);" ^
  "}" ^
  "$fv = @($rows | Where-Object { $_.comments -like 'freeview_*' });" ^
  "$no = @($rows | Where-Object { $_.comments -like 'none_*' });" ^
  "$metrics = @('download_mbps','upload_mbps','idle_median_ms','dl_loaded_median_ms','ul_loaded_median_ms','dns_resolution_ms','tls_handshake_ms','bufferbloat_ms');" ^
  "Write-Host ('Samples: FreeView=' + $fv.Count + ', Direct=' + $no.Count);" ^
  "Write-Host ('Merged CSV: ' + $mergedPath);" ^
  "Write-Host '';" ^
  "Write-Host ('{0,-22} | {1,-20} | {2,-20}' -f 'Metric (mean / med)', 'FreeView', 'Direct (None)');" ^
  "Write-Host ('-' * 68);" ^
  "foreach ($m in $metrics) {" ^
  "  Write-Host ('{0,-22} | {1,-20} | {2,-20}' -f $m, (Get-Stats $fv $m), (Get-Stats $no $m));" ^
  "}"

echo.
echo [DONE] All results saved in: "%OUT_DIR%"
pause
exit /b 0

:start_freeview
powershell -NoProfile -Command "Start-Process -FilePath '%FREEVIEW_EXE%' -ArgumentList '--headless' -WindowStyle Hidden"
powershell -NoProfile -Command ^
  "$ok = $false;" ^
  "for ($k = 0; $k -lt 40; $k++) {" ^
  "  try {" ^
  "    $c = New-Object System.Net.Sockets.TcpClient;" ^
  "    $ar = $c.BeginConnect('127.0.0.1', %PROXY_PORT%, $null, $null);" ^
  "    if ($ar.AsyncWaitHandle.WaitOne(200)) { $c.EndConnect($ar); $c.Close(); $ok = $true; break }" ^
  "    $c.Close();" ^
  "  } catch {}" ^
  "  Start-Sleep -Milliseconds 250;" ^
  "};" ^
  "if (-not $ok) { exit 1 }"
exit /b %errorlevel%

:stop_freeview
powershell -NoProfile -Command "Get-Process | Where-Object { $_.ProcessName -like 'freeview*' } | Stop-Process -Force -ErrorAction SilentlyContinue"
"%FREEVIEW_EXE%" --restore >nul 2>&1
exit /b 0
