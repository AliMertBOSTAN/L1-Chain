# QuantumVault — TAM PAKET: 4 node + cüzdan UI + node-monitor (PowerShell)
#
# Bir komutla bütün local geliştirme yığınını ayağa kaldırır. Mevcut
# `run-devnet.ps1` 4 node'u açar, bu wrapper üzerine cüzdan UI ve
# `node-monitor/` Node.js panelini ekler.
#
# Kullanım:
#   .\run-all.ps1 start     # tüm yığını başlat (build dahil)
#   .\run-all.ps1 stop      # hepsini durdur
#   .\run-all.ps1 status    # 4 node + cüzdan + monitör durumu
#   .\run-all.ps1 clean     # tüm state'i sıfırla
#
# Çevre değişkenleri (opsiyonel):
#   QV_DEVNET_WORK   varsayılan: devnet\work4   (run-devnet.ps1 ile aynı)
#   QV_WALLET_PW     varsayılan: devnetpw
#   QV_MONITOR_PORT  varsayılan: 7070

param([string]$cmd = "start")
$ErrorActionPreference = "Stop"

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
$Work        = if ($env:QV_DEVNET_WORK) { $env:QV_DEVNET_WORK } else { Join-Path $ScriptDir "work4" }
$WalletPort  = 7777
$MonitorPort = if ($env:QV_MONITOR_PORT) { [int]$env:QV_MONITOR_PORT } else { 7070 }
$WalletPw    = if ($env:QV_WALLET_PW) { $env:QV_WALLET_PW } else { "devnetpw" }

function Stop-Extras {
  $pidFile = Join-Path $Work "extras-pids"
  if (Test-Path $pidFile) {
    foreach ($p in Get-Content $pidFile) {
      try { Stop-Process -Id ([int]$p) -Force -ErrorAction SilentlyContinue } catch {}
    }
    Remove-Item $pidFile -Force
    Write-Host "[stop] cuzdan + monitor durduruldu."
  } else { Write-Host "[stop] ekstra process bulunamadi." }
}

function Stop-All {
  Stop-Extras
  & "$ScriptDir\run-devnet.ps1" stop
}

function Show-Status {
  & "$ScriptDir\run-devnet.ps1" status
  try {
    $r = Invoke-RestMethod -Uri "http://127.0.0.1:$WalletPort/api/status" -Method GET -TimeoutSec 3
    Write-Host "  wallet   http=$WalletPort  unlocked=$($r.unlocked)  keystore_exists=$($r.keystore_exists)"
  } catch { Write-Host "  wallet   http=$WalletPort  <unreachable>" }
  try {
    Invoke-WebRequest -Uri "http://127.0.0.1:$MonitorPort/" -Method GET -TimeoutSec 3 | Out-Null
    Write-Host "  monitor  http=$MonitorPort  up"
  } catch { Write-Host "  monitor  http=$MonitorPort  <unreachable>" }
}

function Clean-All {
  Stop-All
  if (Test-Path $Work) {
    Remove-Item -Recurse -Force $Work
    Write-Host "[clean] $Work silindi."
  } else { Write-Host "[clean] zaten temiz." }
}

switch ($cmd) {
  "stop"   { Stop-All; exit 0 }
  "status" { Show-Status; exit 0 }
  "clean"  { Clean-All; exit 0 }
  "start"  { }
  default  { Write-Host "usage: .\run-all.ps1 {start|stop|status|clean}"; exit 1 }
}

# Eski extras'lari oldur (yeniden start)
Stop-Extras | Out-Null

# 1) 4 node'u baslat (run-devnet.ps1 build + start yapar)
Write-Host "[1/3] 4-node devnet baslatiliyor (run-devnet.ps1 start) ..."
& "$ScriptDir\run-devnet.ps1" start

# 2) node0 RPC'si gelene kadar bekle (24 sn)
Write-Host "  node0 RPC bekleniyor ..."
$ready = $false
for ($i = 0; $i -lt 24; $i++) {
  Start-Sleep -Seconds 1
  try {
    Invoke-RestMethod -Uri "http://127.0.0.1:8545" -Method POST `
      -ContentType "application/json" `
      -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}' `
      -TimeoutSec 2 | Out-Null
    $ready = $true
    Write-Host "  node0 RPC hazir ($i sn)"
    break
  } catch {}
}
if (-not $ready) {
  Write-Host "  uyari: node0 RPC 24 sn icinde yanit vermedi. work4\node0.err'a bakin." -ForegroundColor Yellow
}

# 3) qv-wallet derle
Write-Host "[2/3] qv-wallet derleniyor ..."
Push-Location $ProjectRoot
cargo build -p qv-wallet
try {
  $TargetDir = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
} catch {
  $TargetDir = Join-Path $ProjectRoot "target"
}
Pop-Location
$WalletBin = Join-Path $TargetDir "debug\qv-wallet.exe"
if (-not (Test-Path $WalletBin)) { throw "qv-wallet binary not found at $WalletBin" }

# Cuzdan keystore yoksa devnet test mnemonic ile kur
$WalletKeystore = Join-Path $Work "wallet.json"
if (Test-Path $WalletKeystore) {
  Write-Host "  cuzdan keystore zaten var: $WalletKeystore"
} else {
  Write-Host "  cuzdan devnet-import (parola: $WalletPw) ..."
  $argLine = "--keystore `"$WalletKeystore`" --rpc http://127.0.0.1:8545 devnet-import --password $WalletPw"
  $wInit = Start-Process -FilePath $WalletBin -ArgumentList $argLine `
    -WorkingDirectory $ProjectRoot `
    -RedirectStandardOutput (Join-Path $Work "wallet-init.log") `
    -RedirectStandardError  (Join-Path $Work "wallet-init.err") `
    -PassThru -Wait
  if ($wInit.ExitCode -ne 0) {
    Write-Host "  uyari: cuzdan import basarisiz (exit $($wInit.ExitCode))." -ForegroundColor Yellow
  }
}

# Cuzdan UI baslat
$Extras = @()
$walletArgs = "--keystore `"$WalletKeystore`" --rpc http://127.0.0.1:8545 serve --bind 127.0.0.1:$WalletPort"
$walletProc = Start-Process -FilePath $WalletBin -ArgumentList $walletArgs `
  -WorkingDirectory $ProjectRoot `
  -RedirectStandardOutput (Join-Path $Work "wallet.log") `
  -RedirectStandardError  (Join-Path $Work "wallet.err") `
  -PassThru -WindowStyle Hidden
$Extras += $walletProc.Id
Write-Host "  wallet  pid=$($walletProc.Id) ui=http://127.0.0.1:$WalletPort"

# 4) node-monitor baslat (varsa)
Write-Host "[3/3] node-monitor baslatiliyor ..."
$MonitorDir = Join-Path $ProjectRoot "node-monitor"
$MonitorJs  = Join-Path $MonitorDir "index.js"
if (Test-Path $MonitorJs) {
  try {
    & node --version | Out-Null
    $monitorArgs = "`"$MonitorJs`" --work `"$Work`" --port $MonitorPort"
    $monitorProc = Start-Process -FilePath "node" -ArgumentList $monitorArgs `
      -WorkingDirectory $MonitorDir `
      -RedirectStandardOutput (Join-Path $Work "monitor.log") `
      -RedirectStandardError  (Join-Path $Work "monitor.err") `
      -PassThru -WindowStyle Hidden
    $Extras += $monitorProc.Id
    Write-Host "  monitor pid=$($monitorProc.Id) ui=http://127.0.0.1:$MonitorPort"
  } catch {
    Write-Host "  monitor atlandi (node.exe bulunamadi). Node.js yukleyin veya monitor'u atlamak icin: ." -ForegroundColor Yellow
  }
} else {
  Write-Host "  monitor atlandi ($MonitorJs bulunamadi)."
}

# 5) PID kaydı
$Extras | Set-Content -Path (Join-Path $Work "extras-pids")

# 6) Tarayıcı sekmeleri
Start-Sleep -Seconds 2
Write-Host ""
Write-Host "[ok] TAM PAKET calisiyor."
Write-Host "     wallet UI : http://127.0.0.1:$WalletPort   (parola: $WalletPw)"
Write-Host "     monitor   : http://127.0.0.1:$MonitorPort"
Write-Host "     4 node RPC: 127.0.0.1:8545..8548"
Write-Host "     loglar    : $Work"
Write-Host "     durdur    : .\run-all.ps1 stop"
Start-Process "http://127.0.0.1:$WalletPort"
Start-Process "http://127.0.0.1:$MonitorPort"
