# QuantumVault — tek-node devnet + cüzdan UI (Windows PowerShell)
#
# Kullanım:
#   .\run-single.ps1 start     # 1 node + cüzdan UI + tarayıcı
#   .\run-single.ps1 stop      # her ikisini de durdur
#   .\run-single.ps1 status    # RPC ve cüzdan sağlığını sor
#   .\run-single.ps1 clean     # state'i ve cüzdanı sıfırla
#
# Çevre değişkenleri:
#   QV_SINGLE_WORK   varsayılan: devnet\work-single
#   QV_WALLET_PW     varsayılan: devnetpw

param([string]$cmd = "start")
$ErrorActionPreference = "Stop"

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
$Work        = if ($env:QV_SINGLE_WORK) { $env:QV_SINGLE_WORK } else { Join-Path $ScriptDir "work-single" }

$RpcPort     = 8545
$MetricsPort = 9601
$P2pPort     = 17001
$WalletPort  = 7777
$WalletPw    = if ($env:QV_WALLET_PW) { $env:QV_WALLET_PW } else { "devnetpw" }

function Stop-Single {
  $pidFile = Join-Path $Work "pids"
  if (Test-Path $pidFile) {
    foreach ($p in Get-Content $pidFile) {
      try { Stop-Process -Id ([int]$p) -Force -ErrorAction SilentlyContinue } catch {}
    }
    Remove-Item $pidFile -Force
    Write-Host "[stop] tek-node devnet durduruldu."
  } else { Write-Host "[stop] çalışan bir tek-node bulunamadı." }
}

function Show-Status {
  $body = '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'
  try {
    $r = Invoke-RestMethod -Uri "http://127.0.0.1:$RpcPort" -Method POST `
          -ContentType "application/json" -Body $body -TimeoutSec 3
    Write-Host "  node     rpc=$RpcPort   height=$($r.result.height)  tip=$($r.result.block_hash.Substring(0,16))..."
  } catch { Write-Host "  node     rpc=$RpcPort   <unreachable>" }
  try {
    $r = Invoke-RestMethod -Uri "http://127.0.0.1:$WalletPort/api/status" `
          -Method GET -TimeoutSec 3
    Write-Host "  wallet   http=$WalletPort  unlocked=$($r.unlocked)  keystore_exists=$($r.keystore_exists)"
  } catch { Write-Host "  wallet   http=$WalletPort  <unreachable>" }
}

function Clean-Single {
  Stop-Single
  if (Test-Path $Work) {
    Remove-Item -Recurse -Force $Work
    Write-Host "[clean] $Work silindi."
  } else { Write-Host "[clean] zaten temiz." }
}

switch ($cmd) {
  "stop"   { Stop-Single; exit 0 }
  "status" { Show-Status; exit 0 }
  "clean"  { Clean-Single; exit 0 }
  "start"  { }
  default  { Write-Host "usage: .\run-single.ps1 {start|stop|status|clean}"; exit 1 }
}

# Eski process'leri öldür (yeniden start için)
Stop-Single | Out-Null

# 1) Binaries
Write-Host "[1/5] cargo build -p qv-node -p qv-wallet ..."
Push-Location $ProjectRoot
cargo build -p qv-node -p qv-wallet
try {
  $TargetDir = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
} catch {
  $TargetDir = Join-Path $ProjectRoot "target"
}
Pop-Location
$NodeBin   = Join-Path $TargetDir "debug\qv-node.exe"
$WalletBin = Join-Path $TargetDir "debug\qv-wallet.exe"
if (-not (Test-Path $NodeBin))   { throw "qv-node binary not found at $NodeBin" }
if (-not (Test-Path $WalletBin)) { throw "qv-wallet binary not found at $WalletBin" }
Write-Host "  qv-node   : $NodeBin"
Write-Host "  qv-wallet : $WalletBin"

# 2) Work dir + temiz config
New-Item -ItemType Directory -Force -Path $Work | Out-Null
$NodeData   = Join-Path $Work "node-data"
$NodeConfig = Join-Path $Work "node.toml"
$WalletKeystore = Join-Path $Work "wallet.json"

# qv-node --init önceki çağrıdan kalan config'i overwrite eder, ama data dir
# zaten varsa "data already initialized" gibi davranabilir; temiz başlangıç için
# silelim.
if (Test-Path $NodeData) { Remove-Item -Recurse -Force $NodeData }
if (Test-Path $NodeConfig) { Remove-Item -Force $NodeConfig }
if (Test-Path $WalletKeystore) { Remove-Item -Force $WalletKeystore }

# 3) qv-node --init  (config + genesis + mnemonic'i ekrana basar)
Write-Host "[2/5] qv-node --init --network devnet ..."
$initArgs = @(
  "--init",
  "--network", "devnet",
  "--data-dir", "`"$NodeData`"",
  "--config",   "`"$NodeConfig`"",
  "--rpc-addr", "127.0.0.1:$RpcPort",
  "--metrics-addr", "127.0.0.1:$MetricsPort"
) -join ' '
$initLog = Join-Path $Work "init.log"
$initProc = Start-Process -FilePath $NodeBin -ArgumentList $initArgs `
  -WorkingDirectory $ProjectRoot `
  -RedirectStandardOutput $initLog -PassThru -Wait
if ($initProc.ExitCode -ne 0) {
  throw "qv-node --init failed (exit $($initProc.ExitCode)). See $initLog"
}
Write-Host "  init OK — config: $NodeConfig"

# 4) Node'u arka planda başlat
Write-Host "[3/5] qv-node baslatiliyor ..."
$nodeArgs = "--config `"$NodeConfig`" --data-dir `"$NodeData`" --network devnet --rpc-addr 127.0.0.1:$RpcPort --metrics-addr 127.0.0.1:$MetricsPort --log-level info"
$nodeProc = Start-Process -FilePath $NodeBin -ArgumentList $nodeArgs `
  -WorkingDirectory $ProjectRoot `
  -RedirectStandardOutput (Join-Path $Work "node.log") `
  -RedirectStandardError  (Join-Path $Work "node.err") `
  -PassThru -WindowStyle Hidden
$Pids = @($nodeProc.Id)
Write-Host "  node    pid=$($nodeProc.Id) rpc=127.0.0.1:$RpcPort log=$Work\node.log"

# 5) RPC ayağa kalksin diye bekle (15 sn timeout)
Write-Host "  node RPC ayaga kalkmasi bekleniyor ..."
$ready = $false
for ($i = 0; $i -lt 15; $i++) {
  Start-Sleep -Seconds 1
  try {
    Invoke-RestMethod -Uri "http://127.0.0.1:$RpcPort" -Method POST `
      -ContentType "application/json" `
      -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}' `
      -TimeoutSec 2 | Out-Null
    $ready = $true
    Write-Host "  node RPC hazir ($i sn sonra)"
    break
  } catch {}
}
if (-not $ready) {
  Write-Host "  uyari: node RPC 15 sn icinde yanit vermedi. node.err'a bakin." -ForegroundColor Yellow
}

# 6) Cüzdanı devnet test mnemonic'i ile import et
Write-Host "[4/5] cuzdani devnet test mnemonic ile import et ..."
$walletInitArgs = "--keystore `"$WalletKeystore`" --rpc http://127.0.0.1:$RpcPort devnet-import --password $WalletPw"
$wInit = Start-Process -FilePath $WalletBin -ArgumentList $walletInitArgs `
  -WorkingDirectory $ProjectRoot `
  -RedirectStandardOutput (Join-Path $Work "wallet-init.log") `
  -RedirectStandardError  (Join-Path $Work "wallet-init.err") `
  -PassThru -Wait
if ($wInit.ExitCode -ne 0) {
  Write-Host "  uyari: cuzdan import basarisiz (exit $($wInit.ExitCode)). wallet-init.err'a bakin." -ForegroundColor Yellow
} else {
  Write-Host "  cuzdan keystore: $WalletKeystore  (parola: $WalletPw)"
}

# 7) Cüzdan UI'sini başlat
Write-Host "[5/5] cuzdan UI 127.0.0.1:$WalletPort baslatiliyor ..."
$walletArgs = "--keystore `"$WalletKeystore`" --rpc http://127.0.0.1:$RpcPort serve --bind 127.0.0.1:$WalletPort"
$walletProc = Start-Process -FilePath $WalletBin -ArgumentList $walletArgs `
  -WorkingDirectory $ProjectRoot `
  -RedirectStandardOutput (Join-Path $Work "wallet.log") `
  -RedirectStandardError  (Join-Path $Work "wallet.err") `
  -PassThru -WindowStyle Hidden
$Pids += $walletProc.Id
Write-Host "  wallet  pid=$($walletProc.Id) ui=http://127.0.0.1:$WalletPort log=$Work\wallet.log"

# 8) PID'leri kaydet
$Pids | Set-Content -Path (Join-Path $Work "pids")

# 9) Tarayıcı ac
Start-Sleep -Seconds 2
Write-Host ""
Write-Host "[ok] tek-node devnet calisiyor."
Write-Host "     wallet UI : http://127.0.0.1:$WalletPort   (parola: $WalletPw)"
Write-Host "     node RPC  : http://127.0.0.1:$RpcPort"
Write-Host "     loglar    : $Work"
Write-Host "     durdur    : .\run-single.ps1 stop"
Start-Process "http://127.0.0.1:$WalletPort"
