# QuantumVault — 4-node local devnet launcher (Windows PowerShell)
#   .\run-devnet.ps1 start | stop | status
# Env: QV_NODE_BIN, QV_DEVNET_WORK, QV_WARMUP, QV_STAGGER
param([string]$cmd = "start")
$ErrorActionPreference = "Stop"

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
$Work    = if ($env:QV_DEVNET_WORK) { $env:QV_DEVNET_WORK } else { Join-Path $ScriptDir "work4" }
$Warmup  = if ($env:QV_WARMUP)  { $env:QV_WARMUP }  else { 12 }
$Stagger = if ($env:QV_STAGGER) { [int]$env:QV_STAGGER } else { 1 }

$P2P = @(17001, 17002, 17003, 17004)
$RPC = @(8545, 8546, 8547, 8548)
$MET = @(9601, 9602, 9603, 9604)
$VRF = @(
  "1111111111111111111111111111111111111111111111111111111111111111",
  "2222222222222222222222222222222222222222222222222222222222222222",
  "3333333333333333333333333333333333333333333333333333333333333333",
  "4444444444444444444444444444444444444444444444444444444444444444")
$NKEY = @(
  "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
  "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2",
  "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3",
  "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4")
$Stake = 2500000000000

function Stop-Devnet {
  $pidFile = Join-Path $Work "pids"
  if (Test-Path $pidFile) {
    foreach ($p in Get-Content $pidFile) {
      try { Stop-Process -Id ([int]$p) -Force -ErrorAction SilentlyContinue } catch {}
    }
    Remove-Item $pidFile -Force
    Write-Host "[stop] devnet stopped."
  } else { Write-Host "[stop] no running devnet." }
}

function Show-Status {
  for ($i = 0; $i -lt 4; $i++) {
    $body = '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'
    try {
      $r = Invoke-RestMethod -Uri "http://127.0.0.1:$($RPC[$i])" -Method POST `
            -ContentType "application/json" -Body $body -TimeoutSec 3
      Write-Host "  node$i rpc=$($RPC[$i])  height=$($r.result.height)"
    } catch { Write-Host "  node$i rpc=$($RPC[$i])  <unreachable>" }
  }
}

switch ($cmd) {
  "stop"   { Stop-Devnet; exit 0 }
  "status" { Show-Status; exit 0 }
  "start"  { }
  default  { Write-Host "usage: .\run-devnet.ps1 {start|stop|status}"; exit 1 }
}

# Resolve the qv-node binary.
if ($env:QV_NODE_BIN) {
  $Bin = $env:QV_NODE_BIN
} else {
  Write-Host "[build] cargo build -p qv-node ..."
  Push-Location $ProjectRoot
  cargo build -p qv-node
  # Ask cargo for the real target directory (honours any custom target-dir).
  try {
    $TargetDir = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
  } catch {
    $TargetDir = Join-Path $ProjectRoot "target"
  }
  Pop-Location
  $Bin = Join-Path $TargetDir "debug\qv-node.exe"
}
if (-not (Test-Path $Bin)) {
  Write-Host "qv-node binary not found: $Bin"
  Write-Host "Hint: locate it and pass it explicitly, e.g.:"
  Write-Host "  `$env:QV_NODE_BIN = 'C:\path\to\qv-node.exe'; .\devnet\run-devnet.ps1 start"
  exit 1
}
Write-Host "[bin] $Bin"

New-Item -ItemType Directory -Force -Path $Work | Out-Null
Get-ChildItem $Work -File -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -like "node*" } | Remove-Item -Force -ErrorAction SilentlyContinue

# Generate the 4 node configs.
for ($i = 0; $i -lt 4; $i++) {
  $seeds = @()
  for ($j = 0; $j -lt 4; $j++) {
    if ($j -ne $i) { $seeds += "`"/ip4/127.0.0.1/tcp/$($P2P[$j])`"" }
  }
  $lines = @(
    'network = "devnet"',
    "data_dir = `"$($Work -replace '\\','/')/node$i-data`"",
    "listen_addr = `"/ip4/127.0.0.1/tcp/$($P2P[$i])`"",
    "rpc_addr = `"127.0.0.1:$($RPC[$i])`"",
    "metrics_addr = `"127.0.0.1:$($MET[$i])`"",
    "bootstrap_peers = []",
    "seed_nodes = [$($seeds -join ', ')]",
    'storage_backend = "memory"',
    "node_key_seed_hex = `"$($NKEY[$i])`"",
    "round_robin_leader = true",
    "startup_warmup_secs = $Warmup",
    "",
    "[gossip]",
    "max_peers = 64", "max_inbound_peers = 32", "target_outbound_peers = 16",
    "message_ttl = 16", "heartbeat_interval_ms = 1000",
    "",
    "[mempool]",
    "max_clear_pool_size = 10000", "max_encrypted_pool_size = 1000",
    "min_fee_rate = 0", "tx_ttl_slots = 200",
    "",
    "[stake_pool]",
    "vrf_seed_hex = `"$($VRF[$i])`"",
    "initial_stake = $Stake", "active_slot_coeff = 0.05"
  )
  for ($j = 0; $j -lt 4; $j++) {
    $lines += @("", "[[genesis_pools]]", "vrf_seed_hex = `"$($VRF[$j])`"", "stake = $Stake")
  }
  $lines | Set-Content -Path (Join-Path $Work "node$i.toml") -Encoding ascii
}
Write-Host "[config] 4 node configs written to $Work"

# Launch the nodes (staggered so each can dial the ones already up).
# Her node icin nodeN.cmd JSON metadata yazariz - watchdog yeniden
# baslatmak gerektiginde ayni argumanlarla launch icin bunu okur.
$Pids = @()
for ($i = 0; $i -lt 4; $i++) {
  $cfg = Join-Path $Work "node$i.toml"
  $logOut = Join-Path $Work "node$i.log"
  $logErr = Join-Path $Work "node$i.err"
  # Pass args as one quoted string: the config path can contain spaces
  # (e.g. "...\L1 Blockchain\..."), which a bare -ArgumentList array splits.
  $argLine = "--config `"$cfg`" --network devnet --log-level info"

  # Watchdog'un respawn icin okuyacagi metadata.
  $meta = @{
    bin    = $Bin
    args   = $argLine
    rpc    = $RPC[$i]
    p2p    = $P2P[$i]
    met    = $MET[$i]
    logOut = $logOut
    logErr = $logErr
  }
  $meta | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $Work "node$i.cmd") -Encoding ascii

  $p = Start-Process -FilePath $Bin `
        -ArgumentList $argLine `
        -WorkingDirectory $ProjectRoot `
        -RedirectStandardOutput $logOut `
        -RedirectStandardError  $logErr `
        -PassThru -WindowStyle Hidden
  $Pids += $p.Id
  Write-Host "  node$i pid=$($p.Id) rpc=127.0.0.1:$($RPC[$i]) p2p=$($P2P[$i]) metrics=$($MET[$i])"
  Start-Sleep -Seconds $Stagger
}
$Pids | Set-Content -Path (Join-Path $Work "pids")
Write-Host "[up] 4-node devnet running (warmup ${Warmup}s). logs: $Work\nodeN.log"

# Watchdog (opsiyonel) - QV_LAG_RESTART=1000 gibi degisken set'liyse arka planda
# bir PowerShell process'i 1000+ blok geride kalan node'lari kapatip yeniden acar.
if ($env:QV_LAG_RESTART) {
  $threshold = [int]$env:QV_LAG_RESTART
  $interval  = if ($env:QV_LAG_INTERVAL) { [int]$env:QV_LAG_INTERVAL } else { 30 }
  $consec    = if ($env:QV_LAG_CONSEC) { [int]$env:QV_LAG_CONSEC } else { 2 }
  $watchdog  = Join-Path $ScriptDir "watchdog.ps1"
  if (Test-Path $watchdog) {
    $wargs = "-NoProfile -ExecutionPolicy Bypass -File `"$watchdog`" -Work `"$Work`" -Threshold $threshold -Interval $interval -ConsecutiveChecks $consec"
    $wp = Start-Process -FilePath "powershell.exe" -ArgumentList $wargs `
            -RedirectStandardOutput (Join-Path $Work "watchdog.log") `
            -RedirectStandardError  (Join-Path $Work "watchdog.err") `
            -PassThru -WindowStyle Hidden
    Write-Host "  watchdog pid=$($wp.Id) threshold=$threshold interval=${interval}s consec=$consec"
    # Watchdog PID'ini pids dosyasinin sonuna ekle ki stop ile birlikte dussun.
    Add-Content -Path (Join-Path $Work "pids") -Value $wp.Id
  } else {
    Write-Host "  uyari: QV_LAG_RESTART set ama watchdog.ps1 bulunamadi: $watchdog" -ForegroundColor Yellow
  }
}

Write-Host "     monitor: python $ScriptDir\monitor.py --work $Work"
Write-Host "     stop:    .\run-devnet.ps1 stop"
