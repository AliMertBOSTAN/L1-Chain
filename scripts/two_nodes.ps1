# QuantumVault Devnet — Two-Node Local Setup
#
# Launches two nodes on localhost with different ports.
# Node B bootstraps to Node A via libp2p multiaddr.
#
# Usage:
#   .\scripts\two_nodes.ps1
#
# This script starts both nodes as background jobs.
# Press Ctrl+C or close the terminal to stop them.

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║  QuantumVault Devnet — Two-Node Local Network            ║" -ForegroundColor Cyan
Write-Host "╚══════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

# ── Paths ───────────────────────────────────────────────────────────────────
$nodeA_dir  = "data-node-a"
$nodeB_dir  = "data-node-b"
$configA    = "config/node-a.toml"
$configB    = "config/node-b.toml"

# ── Ports ───────────────────────────────────────────────────────────────────
# Node A: P2P 10333, RPC 8545, Metrics 9090
# Node B: P2P 10334, RPC 8546, Metrics 9091

Write-Host "[1/5] Creating data directories..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path $nodeA_dir | Out-Null
New-Item -ItemType Directory -Force -Path $nodeB_dir | Out-Null
New-Item -ItemType Directory -Force -Path "config"   | Out-Null

# ── Initialize Node A ──────────────────────────────────────────────────────
Write-Host "[2/5] Initializing Node A (P2P: 10333, RPC: 8545)..." -ForegroundColor Yellow

cargo run -p qv-node -- `
    --init `
    --network devnet `
    --data-dir $nodeA_dir `
    --config $configA `
    --listen "/ip4/127.0.0.1/tcp/10333" `
    --rpc-addr "127.0.0.1:8545" `
    --metrics-addr "127.0.0.1:9090"

if ($LASTEXITCODE -ne 0) {
    Write-Host "Node A init failed!" -ForegroundColor Red
    exit 1
}

# ── Initialize Node B ──────────────────────────────────────────────────────
Write-Host "[3/5] Initializing Node B (P2P: 10334, RPC: 8546)..." -ForegroundColor Yellow

cargo run -p qv-node -- `
    --init `
    --network devnet `
    --data-dir $nodeB_dir `
    --config $configB `
    --listen "/ip4/127.0.0.1/tcp/10334" `
    --rpc-addr "127.0.0.1:8546" `
    --metrics-addr "127.0.0.1:9091"

if ($LASTEXITCODE -ne 0) {
    Write-Host "Node B init failed!" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "Both nodes initialized." -ForegroundColor Green
Write-Host ""

# ── Start Node A (background job) ──────────────────────────────────────────
Write-Host "[4/5] Starting Node A..." -ForegroundColor Yellow

$jobA = Start-Job -Name "NodeA" -ScriptBlock {
    param($dir, $cfg)
    Set-Location $using:PWD
    cargo run -p qv-node -- `
        --network devnet `
        --data-dir $dir `
        --config $cfg `
        --listen "/ip4/127.0.0.1/tcp/10333" `
        --rpc-addr "127.0.0.1:8545" `
        --metrics-addr "127.0.0.1:9090" `
        --log-level info
} -ArgumentList $nodeA_dir, $configA

# Give Node A a moment to start listening.
Write-Host "  Waiting 5 seconds for Node A to start..." -ForegroundColor DarkGray
Start-Sleep -Seconds 5

# ── Start Node B (bootstrap to A) ──────────────────────────────────────────
Write-Host "[5/5] Starting Node B (bootstrapping to Node A)..." -ForegroundColor Yellow

$jobB = Start-Job -Name "NodeB" -ScriptBlock {
    param($dir, $cfg)
    Set-Location $using:PWD
    cargo run -p qv-node -- `
        --network devnet `
        --data-dir $dir `
        --config $cfg `
        --listen "/ip4/127.0.0.1/tcp/10334" `
        --rpc-addr "127.0.0.1:8546" `
        --metrics-addr "127.0.0.1:9091" `
        --bootstrap "/ip4/127.0.0.1/tcp/10333" `
        --log-level info
} -ArgumentList $nodeB_dir, $configB

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Green
Write-Host "  Both nodes are running!" -ForegroundColor Green
Write-Host ""
Write-Host "  Node A — RPC: http://127.0.0.1:8545  P2P: 10333" -ForegroundColor White
Write-Host "  Node B — RPC: http://127.0.0.1:8546  P2P: 10334" -ForegroundColor White
Write-Host ""
Write-Host "  Query commands:" -ForegroundColor Cyan
Write-Host "    # Node A tip:" -ForegroundColor DarkGray
Write-Host '    Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body ''{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}''' -ForegroundColor White
Write-Host ""
Write-Host "    # Node B tip:" -ForegroundColor DarkGray
Write-Host '    Invoke-RestMethod http://127.0.0.1:8546 -Method POST -ContentType "application/json" -Body ''{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}''' -ForegroundColor White
Write-Host ""
Write-Host "    # Node A mempool:" -ForegroundColor DarkGray
Write-Host '    Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body ''{"jsonrpc":"2.0","id":1,"method":"qv_getMempoolStatus","params":[]}''' -ForegroundColor White
Write-Host ""
Write-Host "    # Send tx (to Node A):" -ForegroundColor DarkGray
Write-Host "    cargo run -p qv-node --example send_tx" -ForegroundColor White
Write-Host ""
Write-Host "  Manage:" -ForegroundColor Cyan
Write-Host "    Receive-Job NodeA    # show Node A logs" -ForegroundColor White
Write-Host "    Receive-Job NodeB    # show Node B logs" -ForegroundColor White
Write-Host "    Stop-Job NodeA,NodeB # stop both nodes" -ForegroundColor White
Write-Host "    Remove-Job NodeA,NodeB" -ForegroundColor White
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Green
Write-Host ""

# ── Tail logs ───────────────────────────────────────────────────────────────
Write-Host "Tailing logs (Ctrl+C to stop)..." -ForegroundColor Yellow
Write-Host ""

try {
    while ($true) {
        # Pull new output from both jobs
        $outA = Receive-Job -Name "NodeA" -ErrorAction SilentlyContinue 2>&1
        $outB = Receive-Job -Name "NodeB" -ErrorAction SilentlyContinue 2>&1

        if ($outA) {
            foreach ($line in $outA) {
                Write-Host "[A] $line" -ForegroundColor DarkCyan
            }
        }
        if ($outB) {
            foreach ($line in $outB) {
                Write-Host "[B] $line" -ForegroundColor DarkMagenta
            }
        }

        # Check if jobs are still running
        $stateA = (Get-Job -Name "NodeA" -ErrorAction SilentlyContinue).State
        $stateB = (Get-Job -Name "NodeB" -ErrorAction SilentlyContinue).State

        if ($stateA -eq "Failed" -or $stateA -eq "Completed") {
            Write-Host "Node A stopped ($stateA)" -ForegroundColor Red
        }
        if ($stateB -eq "Failed" -or $stateB -eq "Completed") {
            Write-Host "Node B stopped ($stateB)" -ForegroundColor Red
        }
        if (($stateA -ne "Running") -and ($stateB -ne "Running")) {
            Write-Host "Both nodes stopped." -ForegroundColor Red
            break
        }

        Start-Sleep -Milliseconds 500
    }
} finally {
    Write-Host ""
    Write-Host "Cleaning up..." -ForegroundColor Yellow
    Stop-Job -Name "NodeA" -ErrorAction SilentlyContinue
    Stop-Job -Name "NodeB" -ErrorAction SilentlyContinue
    Remove-Job -Name "NodeA" -ErrorAction SilentlyContinue
    Remove-Job -Name "NodeB" -ErrorAction SilentlyContinue
    Write-Host "Done." -ForegroundColor Green
}
