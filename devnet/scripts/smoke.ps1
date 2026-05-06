# QuantumVault L1 — Smoke Demo (Windows PowerShell)
#
# Mevcut iskeletin canlı olduğunu kanıtlayan minimum akış:
#   1) qv-wallet init  -> 24 kelimelik BIP-39 mnemonic uretir
#   2) qv-wallet import-mnemonic -> mnemonic'in valid oldugunu round-trip dogrular
#   3) qv-miner init   -> PQC operator anahtarlari + operator.toml uretir
#   4) qv-node --init  -> devnet config skeleton'u yazar
#   5) qv-node start   -> N saniye arka planda baslat, loglari topla, durdur
#   6) RPC kontrolu    -> bind edilmis mi? (Beklenti: HAYIR -- Faz 1 gorevi)
#
# Calistirma:
#   pwsh -ExecutionPolicy Bypass -File devnet\scripts\smoke.ps1
#   veya
#   powershell.exe -ExecutionPolicy Bypass -File .\devnet\scripts\smoke.ps1

[CmdletBinding()]
param(
    [int]$NodeRunSeconds = 8
)

# PowerShell 5.1 native command'larin stderr'ini RemoteException olarak yorumlar.
# cargo --quiet warning'leri stderr'e basabiliyor; "Stop" olursa script ilk
# warning'de patlar. Continue ile devam ediyoruz; her adimda zaten exit code
# ve dosya varligi kontrol ediliyor.
$ErrorActionPreference = "Continue"
$OutputEncoding = [System.Text.UTF8Encoding]::new()
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$env:CARGO_TERM_COLOR = "never"
if (-not $env:RUST_LOG) { $env:RUST_LOG = "info" }

# Repo koku scripts/ -> devnet/ -> repo
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$Manifest = Join-Path $RepoRoot "Cargo.toml"
$WorkDir  = Join-Path $RepoRoot "devnet\work"

function Write-Step($n, $msg) {
    Write-Host ""
    Write-Host ("=" * 60) -ForegroundColor DarkCyan
    Write-Host "  [$n] $msg" -ForegroundColor Cyan
    Write-Host ("=" * 60) -ForegroundColor DarkCyan
}
function Write-Note($msg) { Write-Host "    > $msg" -ForegroundColor DarkYellow }
function Write-Ok($msg)   { Write-Host "    + $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "    ! $msg" -ForegroundColor Yellow }
function Write-Bad($msg)  { Write-Host "    x $msg" -ForegroundColor Red }

function Invoke-Cargo([string[]]$BinArgs) {
    # cargo'nun stderr'i (warning'ler, "Compiling..." mesajlari, vb.) PowerShell'de
    # her satira RemoteException prefix'i ekleyerek cikiyor — bu hem cirkin hem
    # gercek bir hata degil. Cozum: stderr'i bir gecici dosyaya yonlendir,
    # sadece stdout'u dondur. Hata varsa exit code'dan anlariz.
    $oldEAP = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $tmpErr = [IO.Path]::GetTempFileName()
    try {
        $cargoArgs = @("run", "--release", "--quiet", "--manifest-path", $Manifest) + $BinArgs
        $stdout = & cargo @cargoArgs 2>$tmpErr
        # Eger non-zero exit ve stdout bos ise, stderr'in son birkac satirini geri ver
        # (kullanici en azindan ne oldugunu gorsun).
        if ($LASTEXITCODE -ne 0) {
            $errTail = Get-Content $tmpErr -Tail 10 -ErrorAction SilentlyContinue
            if ($errTail) {
                Write-Host "    [stderr tail]:" -ForegroundColor DarkRed
                $errTail | ForEach-Object { Write-Host "      $_" -ForegroundColor DarkRed }
            }
        }
        return $stdout
    } finally {
        Remove-Item $tmpErr -ErrorAction SilentlyContinue
        $ErrorActionPreference = $oldEAP
    }
}

# --------------------------------------------------------------------
Write-Step "0/7" "Hazirlik: temiz work dizini"

if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
New-Item -ItemType Directory -Path $WorkDir | Out-Null
New-Item -ItemType Directory -Path (Join-Path $WorkDir "keys") | Out-Null
New-Item -ItemType Directory -Path (Join-Path $WorkDir "data") | Out-Null
Write-Ok "Work dizini: $WorkDir"

# --------------------------------------------------------------------
Write-Step "1/7" "qv-wallet init: 24-kelimelik BIP-39 mnemonic"

Write-Note "Komut: qv-wallet init"
Write-Note "Beklenen: 'Mnemonic: ...' satiri + 'SAVE THIS SECURELY!'"

$walletInit = Invoke-Cargo @("--bin","qv-wallet","--","init")
$walletInit | ForEach-Object { Write-Host "      $_" }

$mnemonicLine = $walletInit | Where-Object { $_ -match "^Mnemonic: " } | Select-Object -First 1
$phrase = ($mnemonicLine -replace "^Mnemonic:\s*", "").Trim()

if ($phrase) {
    $wordCount = ($phrase -split '\s+').Count
    Write-Ok "Mnemonic uretildi ($wordCount kelime)"
    if ($wordCount -ne 24) { Write-Warn "24 bekleniyordu, $wordCount geldi" }
    Set-Content -Path (Join-Path $WorkDir "demo-mnemonic.txt") -Value $phrase -Encoding UTF8
    Write-Ok "Kayit: devnet\work\demo-mnemonic.txt"
} else {
    Write-Bad "Mnemonic cikarilamadi"
}

# --------------------------------------------------------------------
Write-Step "2/7" "qv-wallet import-mnemonic: round-trip dogrulama"

if ($phrase) {
    Write-Note "Komut: qv-wallet import-mnemonic '<phrase>'"
    Write-Note "Beklenen: exit 0"
    $importOut = Invoke-Cargo @("--bin","qv-wallet","--","import-mnemonic", $phrase)
    $importOut | ForEach-Object { Write-Host "      $_" }
    if ($LASTEXITCODE -eq 0) { Write-Ok "Round-trip BASARILI" }
    else { Write-Bad "import-mnemonic basarisiz (exit=$LASTEXITCODE)" }

    Write-Note "Negatif test: bozuk phrase"
    $bad = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis"
    $negOut = Invoke-Cargo @("--bin","qv-wallet","--","import-mnemonic", $bad)
    if ($LASTEXITCODE -ne 0) { Write-Ok "Bozuk phrase REDDEDILDI (exit=$LASTEXITCODE) -- dogru davranis" }
    else { Write-Warn "Bozuk phrase kabul edildi -- beklenmeyen" }
}

# --------------------------------------------------------------------
Write-Step "3/7" "qv-miner init: PQC keys + operator.toml"

$operatorToml = Join-Path $WorkDir "operator.toml"
Write-Note "Komut: qv-miner init --pool-name DemoPool --output operator.toml"
Write-Note "Beklenen: operator.toml yazilir"
Write-Warn "BILINEN SINIRLILIK: keys/*.sk dosyalari HENUZ yazilmiyor (ROADMAP Faz 1)"

$minerInit = Invoke-Cargo @(
    "--bin","qv-miner","--",
    "--config-dir", $WorkDir,
    "init",
    "--pool-name", "DemoPool",
    "--output", $operatorToml
)
$minerInit | ForEach-Object { Write-Host "      $_" }

if (Test-Path $operatorToml) {
    Write-Ok "operator.toml olustu"
    Write-Note "Icerik (ilk 25 satir):"
    Get-Content $operatorToml | Select-Object -First 25 | ForEach-Object { Write-Host "        $_" -ForegroundColor DarkGray }
} else {
    Write-Bad "operator.toml olusturulamadi"
}

# --------------------------------------------------------------------
Write-Step "4/7" "qv-node --init: devnet config skeleton"

$nodeConfigPath = Join-Path $WorkDir "qv-node.toml"
$nodeDataDir    = Join-Path $WorkDir "data"

Write-Note "Komut: qv-node --init --network devnet --config qv-node.toml"
Write-Note "Beklenen: qv-node.toml yazilir"

$nodeInit = Invoke-Cargo @(
    "--bin","qv-node","--",
    "--init",
    "--network","devnet",
    "--config", $nodeConfigPath,
    "--data-dir", $nodeDataDir
)
$nodeInit | ForEach-Object { Write-Host "      $_" }

if (Test-Path $nodeConfigPath) {
    Write-Ok "qv-node.toml olustu"
    Write-Note "Icerik (ilk 30 satir):"
    Get-Content $nodeConfigPath | Select-Object -First 30 | ForEach-Object { Write-Host "        $_" -ForegroundColor DarkGray }
} else {
    Write-Bad "qv-node.toml yok"
}

# --------------------------------------------------------------------
Write-Step "5/7" "qv-node $NodeRunSeconds sn arka planda baslat"

Write-Warn "BILINEN SINIRLILIK: jsonrpsee server HENUZ bind etmiyor (ROADMAP Faz 1)"
Write-Note "Komut: qv-node --network devnet --config qv-node.toml --rpc-addr 127.0.0.1:18545"

$logFile = Join-Path $WorkDir "node.log"
$errFile = Join-Path $WorkDir "node.err"

# Start-Process'in -ArgumentList'i bosluklu path'leri yutuyor (PS 5.1).
# Cozum: --manifest-path argumani yerine -WorkingDirectory ile cargo'yu
# repo kokune bagla; manifest'i CWD'den otomatik bulacak.
$cargoArgs = @(
    "run","--release","--quiet",
    "--bin","qv-node","--",
    "--network","devnet",
    "--config", $nodeConfigPath,
    "--data-dir", $nodeDataDir,
    "--rpc-addr","127.0.0.1:18545",
    "--metrics-addr","127.0.0.1:19090",
    "--log-level","info"
)

$proc = Start-Process -FilePath "cargo" -ArgumentList $cargoArgs `
    -WorkingDirectory $RepoRoot `
    -RedirectStandardOutput $logFile `
    -RedirectStandardError  $errFile `
    -PassThru -NoNewWindow

Write-Ok "qv-node PID=$($proc.Id) baslatildi, $NodeRunSeconds sn bekleniyor..."
Start-Sleep -Seconds $NodeRunSeconds

# --------------------------------------------------------------------
Write-Step "6/7" "RPC erisim testi (Beklenti: BIND var -- Faz 1.1 sonrasi)"

$rpcReachable = $false
$client = New-Object System.Net.Sockets.TcpClient
try {
    $task = $client.ConnectAsync("127.0.0.1", 18545)
    if ($task.Wait(1500) -and $client.Connected) {
        $rpcReachable = $true
    }
} catch { $rpcReachable = $false }
finally { $client.Close() }

if ($rpcReachable) {
    Write-Ok "RPC TCP listen OK (127.0.0.1:18545)"
    Write-Note "JSON-RPC istek atiliyor: qv_getTip"
    try {
        $body = @{ jsonrpc="2.0"; id=1; method="qv_getTip"; params=@() } | ConvertTo-Json -Compress
        $resp = Invoke-RestMethod -Uri "http://127.0.0.1:18545" -Method Post -Body $body -ContentType "application/json" -TimeoutSec 5
        Write-Ok "qv_getTip cevabi:"
        Write-Host "        $($resp | ConvertTo-Json -Compress -Depth 5)" -ForegroundColor DarkGray

        Write-Note "JSON-RPC istek atiliyor: qv_getMempoolStatus"
        $body2 = @{ jsonrpc="2.0"; id=2; method="qv_getMempoolStatus"; params=@() } | ConvertTo-Json -Compress
        $resp2 = Invoke-RestMethod -Uri "http://127.0.0.1:18545" -Method Post -Body $body2 -ContentType "application/json" -TimeoutSec 5
        Write-Ok "qv_getMempoolStatus cevabi:"
        Write-Host "        $($resp2 | ConvertTo-Json -Compress -Depth 5)" -ForegroundColor DarkGray

        Write-Note "(BILINEN: cevaplar henuz placeholder; gercek state Faz 1.2'de wire edilecek)"
    } catch {
        Write-Bad "JSON-RPC istek basarisiz: $($_.Exception.Message)"
    }
} else {
    Write-Bad "RPC bind YOK (beklenmiyor) -- Faz 1.1 fix gerekli"
}

# Node'u durdur (parent + qv-node.exe child)
if (-not $proc.HasExited) {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Milliseconds 500
Get-Process -Name "qv-node" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Write-Ok "qv-node ve cargo wrapper durduruldu"

# Loglari goster
Write-Note "node.log (ilk 25 satir):"
if (Test-Path $logFile) {
    $lines = Get-Content $logFile
    if ($lines.Count -gt 0) {
        $lines | Select-Object -First 25 | ForEach-Object { Write-Host "        $_" -ForegroundColor DarkGray }
    } else {
        Write-Host "        (bos)" -ForegroundColor DarkGray
    }
}
if ((Test-Path $errFile) -and ((Get-Item $errFile).Length -gt 0)) {
    Write-Note "node.err (ilk 20 satir):"
    Get-Content $errFile | Select-Object -First 20 | ForEach-Object { Write-Host "        $_" -ForegroundColor DarkRed }
}

# --------------------------------------------------------------------
Write-Step "7/7" "Ozet"

Write-Host ""
Write-Host "  Bu turda CALISAN akislar:" -ForegroundColor Green
Write-Host "    + qv-wallet init      (BIP-39 v2 + rand feature -> 24 kelime)"
Write-Host "    + qv-wallet import    (round-trip + negatif test)"
Write-Host "    + qv-miner init       (operator.toml; PQC keys generate ediliyor ancak diske YAZILMIYOR)"
Write-Host "    + qv-node --init      (qv-node.toml devnet preset)"
Write-Host "    + qv-node startup     (loglar uretiliyor; sleep dongusunde takiliyor)"
Write-Host ""
Write-Host "  Faz 1'de COZULMUS:" -ForegroundColor Green
Write-Host "    + qv-node JSON-RPC server bind (Faz 1.1)"
Write-Host ""
Write-Host "  Bu turda HENUZ CALISMAYAN (ROADMAP Faz 1 devami):" -ForegroundColor Yellow
Write-Host "    - RPC handler'lar gercek state degil placeholder dondurur (Faz 1.2)"
Write-Host "    - qv-wallet keystore.save (Faz 1.5)"
Write-Host "    - qv-wallet balance/scan/send (Faz 1.8)"
Write-Host "    - qv-miner keys'i diske yazma"
Write-Host "    - Gercek block production / consensus loop (Faz 3)"
Write-Host ""
Write-Host "  Daha fazla bilgi: docs\ROADMAP.md" -ForegroundColor Cyan
Write-Host "  Olusturulan dosyalar: $WorkDir" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  Smoke demo BITTI." -ForegroundColor Green
