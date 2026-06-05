# QuantumVault devnet supervision watchdog
# ----------------------------------------
# Calistirma:
#   .\watchdog.ps1 -Work <work_dir> -Threshold 1000 -Interval 30
#
# Her -Interval saniyede bir, $Work icindeki nodeN.cmd JSON metadata
# dosyalarini okur, listelenen RPC portlarini sorgulayip her node'un
# qv_getTip yuksekligini ogrenir. Maksimum yukseklikten -Threshold
# blok'tan fazla geride kalan node'lar -ConsecutiveChecks olcumde ust
# uste geride kalirsa process'leri Stop-Process ile sonlandirilir ve
# ayni argumanlarla yeniden baslatilir. Yeni PID, devnet pids dosyasina
# eski PID'in yerine yazilir; boylece run-devnet.ps1 stop hala calisir.
#
# Memory storage backend ile node restart, peer'lerden tam yeniden
# senkronizasyon (ADR-010 SyncManager) tetikler.

param(
  [string]$Work,
  [int]$Threshold = 1000,
  [int]$Interval = 30,
  [int]$ConsecutiveChecks = 2
)

$ErrorActionPreference = "Continue"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not $Work) { $Work = Join-Path $ScriptDir "work4" }
if (-not (Test-Path $Work)) {
  Write-Host "[watchdog] work dir not found: $Work"
  exit 1
}

# nodeN.cmd dosyalarindan node'lari kesfet.
$Nodes = @()
Get-ChildItem $Work -Filter "node*.cmd" -ErrorAction SilentlyContinue |
  Sort-Object Name | ForEach-Object {
    $name = $_.BaseName
    try {
      $meta = Get-Content $_.FullName -Raw | ConvertFrom-Json
      $Nodes += @{
        Name     = $name
        Bin      = $meta.bin
        Args     = $meta.args
        Rpc      = [int]$meta.rpc
        LogOut   = $meta.logOut
        LogErr   = $meta.logErr
        LagCount = 0
      }
    } catch {
      Write-Host "[watchdog] failed to parse $($_.Name): $_"
    }
  }

if (-not $Nodes -or $Nodes.Count -eq 0) {
  Write-Host "[watchdog] no node*.cmd metadata in $Work - run-devnet.ps1 calismadi mi?"
  exit 1
}

Write-Host "[watchdog] supervising $($Nodes.Count) nodes, threshold=$Threshold blok, interval=${Interval}s"

# ----- yardimcilar -----
function Get-NodeTip([int]$rpcPort) {
  $body = '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'
  try {
    $r = Invoke-RestMethod -Uri "http://127.0.0.1:$rpcPort" -Method POST `
           -ContentType "application/json" -Body $body -TimeoutSec 5
    if ($r -and $r.result -and ($r.result.height -ne $null)) {
      return [int]$r.result.height
    }
  } catch {}
  return -1
}

function Find-NodePid([int]$rpcPort) {
  # rpcPort'u dinleyen TCP socket'in sahibi PID'i bul.
  $lines = netstat -ano -p tcp 2>$null | Select-String ":$rpcPort\s.*LISTENING"
  foreach ($l in $lines) {
    $parts = ($l.Line -split '\s+') | Where-Object { $_ -ne "" }
    if ($parts.Count -ge 5) {
      $candidate = $parts[-1]
      if ($candidate -match '^\d+$') { return [int]$candidate }
    }
  }
  return 0
}

function Update-PidFile([int]$oldPid, [int]$newPid) {
  $pidFile = Join-Path $Work "pids"
  if (-not (Test-Path $pidFile)) { return }
  $existing = @(Get-Content $pidFile | Where-Object { $_ -match '^\d+$' } | ForEach-Object { [int]$_ })
  $updated = @()
  $replaced = $false
  foreach ($pid in $existing) {
    if ($pid -eq $oldPid) { $updated += $newPid; $replaced = $true }
    else { $updated += $pid }
  }
  if (-not $replaced) { $updated += $newPid }
  $updated | Set-Content -Path $pidFile
}

function Restart-Node($node, [string]$reason) {
  $oldPid = Find-NodePid $node.Rpc
  Write-Host "[watchdog] RESTART $($node.Name) reason=$reason oldPid=$oldPid"
  if ($oldPid -gt 0) {
    try { Stop-Process -Id $oldPid -Force -ErrorAction SilentlyContinue } catch {}
  }
  Start-Sleep -Seconds 3

  $argList = $node.Args
  if ($argList -is [array]) { $argList = $argList -join ' ' }

  $p = Start-Process -FilePath $node.Bin `
        -ArgumentList $argList `
        -RedirectStandardOutput $node.LogOut `
        -RedirectStandardError  $node.LogErr `
        -PassThru -WindowStyle Hidden
  Write-Host "[watchdog] $($node.Name) respawned pid=$($p.Id)"
  Update-PidFile $oldPid $p.Id
  # Yeni node'a sync icin nefes alma payi.
  Start-Sleep -Seconds 5
}

# ----- ana dongu -----
while ($true) {
  Start-Sleep -Seconds $Interval

  $tips = @()
  foreach ($n in $Nodes) {
    $h = Get-NodeTip $n.Rpc
    $tips += @{ Name = $n.Name; Height = $h }
  }

  $reachable = $tips | Where-Object { $_.Height -ge 0 }
  if (-not $reachable -or $reachable.Count -eq 0) {
    Write-Host "[watchdog] no nodes reachable - skip"
    continue
  }
  $maxHeight = ($reachable | Measure-Object -Property Height -Maximum).Maximum

  # Zincir henuz cok kisaysa anlamli bir lag testi yapilamaz.
  if ($maxHeight -lt $Threshold) {
    Write-Host "[watchdog] tip=$maxHeight too low for lag check"
    continue
  }

  $report = ""
  for ($i = 0; $i -lt $Nodes.Count; $i++) {
    $n = $Nodes[$i]
    $h = $tips[$i].Height
    $tag = ""
    if ($h -lt 0) {
      $n.LagCount++
      $tag = "UNREACHABLE (cons=$($n.LagCount))"
    } else {
      $lag = $maxHeight - $h
      if ($lag -gt $Threshold) {
        $n.LagCount++
        $tag = "lag=$lag (cons=$($n.LagCount))"
      } else {
        if ($n.LagCount -gt 0) { $tag = "recovered (lag=$lag)" }
        else { $tag = "OK (lag=$lag)" }
        $n.LagCount = 0
      }
    }
    $report += "  $($n.Name) h=$h $tag`n"

    if ($n.LagCount -ge $ConsecutiveChecks) {
      Write-Host ""
      Write-Host "[watchdog] tip across cluster = $maxHeight"
      Write-Host $report.TrimEnd()
      $reason = if ($h -lt 0) { "unreachable" } else { "behind by $($maxHeight - $h)" }
      Restart-Node $n $reason
      $n.LagCount = 0
      $report = ""
    }
  }

  # Periyodik durum (lag yok da olsa).
  if ($report) {
    $oneline = $report.TrimEnd().Replace("`r","").Replace("`n"," | ")
    Write-Host "[watchdog] tip=$maxHeight ; $oneline"
  }
}
