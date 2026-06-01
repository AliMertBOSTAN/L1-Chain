//! Single-file HTML / CSS / JS UI for the wallet HTTP server.
//!
//! Embedded as a `&'static str` so the binary is fully self-contained — no
//! external asset paths, no template engine. The UI talks to the same
//! axum server it is served from via `/api/*` JSON endpoints (see
//! [`crate::server`]).

pub const INDEX_HTML: &str = r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>QuantumVault Wallet</title>
<style>
  :root {
    --bg: #0b1020;
    --panel: #131a32;
    --panel-2: #1a2347;
    --text: #e7ecf4;
    --muted: #8a93ad;
    --accent: #6aa9ff;
    --accent-2: #46e0a0;
    --danger: #ff6b6b;
    --border: #283157;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: linear-gradient(180deg, #070b1e 0%, #0b1020 100%);
    color: var(--text);
    min-height: 100vh;
  }
  header {
    padding: 20px 24px;
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  header h1 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    letter-spacing: 0.4px;
  }
  header h1 .badge {
    color: var(--accent);
    font-weight: 500;
    margin-left: 8px;
    font-size: 12px;
    padding: 2px 8px;
    border: 1px solid var(--accent);
    border-radius: 999px;
  }
  #status-bar {
    font-size: 12px;
    color: var(--muted);
    display: flex;
    gap: 16px;
    align-items: center;
  }
  #status-bar .dot {
    width: 8px; height: 8px; border-radius: 50%;
    background: var(--danger);
    display: inline-block; margin-right: 6px;
  }
  #status-bar.unlocked .dot { background: var(--accent-2); }
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: 24px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px;
  }
  .panel {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 18px;
  }
  .panel h2 {
    margin: 0 0 12px;
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 1px;
    color: var(--muted);
    font-weight: 600;
  }
  .panel.span2 { grid-column: 1 / span 2; }
  label { display: block; font-size: 12px; color: var(--muted); margin: 8px 0 4px; }
  input, textarea {
    width: 100%;
    background: var(--panel-2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 9px 10px;
    font: inherit;
    outline: none;
  }
  input:focus, textarea:focus { border-color: var(--accent); }
  textarea { min-height: 72px; resize: vertical; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; }
  button {
    background: var(--accent);
    color: #001428;
    border: none;
    border-radius: 6px;
    padding: 8px 14px;
    font-weight: 600;
    cursor: pointer;
    margin-top: 12px;
  }
  button.ghost {
    background: transparent;
    color: var(--text);
    border: 1px solid var(--border);
  }
  button.danger { background: var(--danger); color: #1a0606; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  .row { display: flex; gap: 8px; align-items: center; }
  .row > * { flex: 1; }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; word-break: break-all; }
  .kv { display: grid; grid-template-columns: 110px 1fr; gap: 4px 12px; align-items: baseline; }
  .kv dt { color: var(--muted); font-size: 12px; }
  .kv dd { margin: 0; }
  .balance-big { font-size: 36px; font-weight: 600; letter-spacing: -0.5px; color: var(--accent-2); }
  .balance-big small { color: var(--muted); font-size: 14px; font-weight: 500; margin-left: 6px; }
  .toast {
    position: fixed; right: 20px; bottom: 20px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-left: 4px solid var(--accent);
    padding: 12px 16px;
    border-radius: 6px;
    max-width: 380px;
    box-shadow: 0 10px 30px rgba(0,0,0,0.4);
    opacity: 0; transform: translateY(10px);
    transition: opacity .2s, transform .2s;
  }
  .toast.show { opacity: 1; transform: translateY(0); }
  .toast.err { border-left-color: var(--danger); }
  .toast.ok { border-left-color: var(--accent-2); }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  table th, table td {
    padding: 6px 8px; text-align: left; border-bottom: 1px solid var(--border);
  }
  table th { color: var(--muted); font-weight: 500; }
  .copy-row { display: flex; gap: 8px; align-items: center; }
  .copy-row .mono { flex: 1; padding: 6px 8px; background: var(--panel-2); border: 1px solid var(--border); border-radius: 4px; }
  .hidden { display: none !important; }
  hr { border: none; border-top: 1px solid var(--border); margin: 16px 0; }
  .note { font-size: 12px; color: var(--muted); }
  .note strong { color: var(--text); }
</style>
</head>
<body>
<header>
  <h1>QuantumVault Wallet <span class="badge">post-quantum · stealth · devnet</span></h1>
  <div id="status-bar"><span class="dot"></span><span id="status-text">locked</span><span id="rpc-text"></span></div>
</header>

<main>
  <!-- Unlock / Create / Import (only one shown at a time depending on state) -->
  <section class="panel" id="unlock-panel">
    <h2 id="unlock-title">Open wallet</h2>
    <div id="unlock-existing">
      <label>Password</label>
      <input id="unlock-pw" type="password" autocomplete="current-password">
      <button id="btn-unlock">Unlock</button>
      <button class="ghost" id="show-import">I have a mnemonic</button>
    </div>
    <div id="unlock-create" class="hidden">
      <p class="note">No keystore found at the configured path — create a new wallet or import one.</p>
      <label>Choose a password (min 8 characters)</label>
      <input id="create-pw" type="password" autocomplete="new-password">
      <button id="btn-create">Create new wallet</button>
      <button class="ghost" id="show-import-from-create">Import from mnemonic</button>
    </div>
    <div id="unlock-import" class="hidden">
      <label>Recovery phrase (24 words)</label>
      <textarea id="import-phrase" placeholder="word1 word2 word3 ..."></textarea>
      <label>New password</label>
      <input id="import-pw" type="password" autocomplete="new-password">
      <div class="row">
        <button id="btn-import">Import</button>
        <button class="ghost" id="back-to-unlock">Back</button>
      </div>
    </div>
  </section>

  <section class="panel" id="address-panel">
    <h2>Address</h2>
    <div id="addr-empty" class="note">Unlock the wallet to see your stealth address.</div>
    <div id="addr-filled" class="hidden">
      <dl class="kv">
        <dt>Fingerprint</dt>
        <dd class="mono" id="addr-fp"></dd>
        <dt>Account</dt>
        <dd class="mono" id="addr-account">0</dd>
      </dl>
      <div id="fp-qr-wrap" style="margin:8px 0; padding:8px; background:#fff; border-radius:6px; display:inline-block">
        <img id="fp-qr" alt="fingerprint QR" style="width:180px; height:180px; display:block">
      </div>
      <div class="note" style="margin-top:-4px">Fingerprint QR — for quick identity check only. <strong>Not payable.</strong></div>
      <hr>
      <div class="note">Full stealth address (share to receive):</div>
      <div class="copy-row">
        <div class="mono" id="addr-full"></div>
      </div>
      <div class="row" style="margin-top:10px">
        <button class="ghost" id="btn-copy-addr">Copy</button>
        <button class="ghost" id="btn-download-qvaddr">Download .qvaddr</button>
        <button class="ghost" id="btn-show-full-qr">Show full QR (2 codes)</button>
      </div>
      <hr>
      <button class="ghost danger" id="btn-lock">Lock wallet</button>
    </div>
  </section>

  <section class="panel">
    <h2>Balance</h2>
    <div id="balance-empty" class="note">Unlock the wallet to view your balance.</div>
    <div id="balance-filled" class="hidden">
      <div class="balance-big"><span id="balance-value">0</span><small>units</small></div>
      <button class="ghost" id="btn-refresh">Refresh</button>
    </div>
  </section>

  <section class="panel">
    <h2>Send</h2>
    <div id="send-empty" class="note">Unlock the wallet to send a transfer.</div>
    <div id="send-filled" class="hidden">
      <label>Recipient stealth address (qvst1…)</label>
      <textarea id="send-to" placeholder="qvst1..."></textarea>
      <div class="row" style="margin-bottom:4px">
        <label style="margin:0; flex:0 0 auto" for="send-qvaddr-file">…or load a <code>.qvaddr</code> file:</label>
        <input id="send-qvaddr-file" type="file" accept=".qvaddr,application/json" style="background:transparent; border:none; padding:0">
      </div>
      <div class="row">
        <div>
          <label>Amount (units)</label>
          <input id="send-amount" type="number" min="1">
        </div>
        <div>
          <label>Fee (units)</label>
          <input id="send-fee" type="number" min="0" value="1000">
        </div>
      </div>
      <button id="btn-send">Send transfer</button>
    </div>
  </section>

  <section class="panel span2">
    <h2>Owned UTXOs</h2>
    <div id="utxos-empty" class="note">Unlock the wallet to scan for received stealth payments.</div>
    <div id="utxos-filled" class="hidden">
      <table>
        <thead><tr><th>Kind</th><th>TX ID</th><th>Out</th><th style="text-align:right">Value</th></tr></thead>
        <tbody id="utxos-tbody"></tbody>
      </table>
      <button class="ghost" id="btn-refresh-utxos">Re-scan</button>
    </div>
  </section>

  <section class="panel span2" id="full-qr-panel" style="display:none">
    <h2>Full-address QR (multi-part)</h2>
    <div class="note">
      A full QuantumVault stealth address is ~3 KB and does not fit in a
      single QR code. The wallet splits it into the parts shown below; the
      receiving QuantumVault wallet must scan <strong>all of them</strong>
      to reassemble the address. For most use cases the <code>.qvaddr</code>
      file is the easier path.
    </div>
    <div id="full-qr-grid" style="display:grid; grid-template-columns:1fr 1fr; gap:16px; margin-top:14px"></div>
    <button class="ghost" id="btn-hide-full-qr" style="margin-top:12px">Hide</button>
  </section>

  <section class="panel span2" id="mnemonic-panel" style="display:none">
    <h2>Backup mnemonic</h2>
    <div class="note"><strong>Write this down.</strong> Anyone who knows these 24 words can move your funds. We won't show it again after you close this panel.</div>
    <div class="mono" id="mnemonic-words" style="margin-top:12px;padding:12px;background:var(--panel-2);border:1px dashed var(--accent-2);border-radius:6px"></div>
    <div class="row" style="margin-top:12px">
      <button id="btn-copy-mnemonic">Copy phrase</button>
      <button class="ghost" id="btn-mnemonic-done">I have written it down</button>
    </div>
  </section>
</main>

<div id="toast" class="toast"></div>

<script>
(() => {
  const $ = (id) => document.getElementById(id);
  const show = (id, on=true) => $(id).classList.toggle('hidden', !on);
  const toast = (msg, kind) => {
    const t = $('toast');
    t.textContent = msg;
    t.className = 'toast show ' + (kind || '');
    setTimeout(() => t.classList.remove('show'), 4000);
  };

  async function api(method, path, body) {
    const opt = { method, headers: { 'content-type': 'application/json' } };
    if (body !== undefined) opt.body = JSON.stringify(body);
    const r = await fetch(path, opt);
    const ct = r.headers.get('content-type') || '';
    const data = ct.includes('application/json') ? await r.json() : await r.text();
    if (!r.ok) {
      const msg = (data && data.error) || r.statusText || ('http ' + r.status);
      throw new Error(msg);
    }
    return data;
  }

  async function refreshStatus() {
    const s = await api('GET', '/api/status');
    $('status-text').textContent = s.unlocked ? 'unlocked' : 'locked';
    $('rpc-text').textContent = 'node: ' + s.rpc_url;
    document.getElementById('status-bar').classList.toggle('unlocked', s.unlocked);

    // Unlock panel visibility logic.
    if (s.unlocked) {
      show('unlock-panel', false);
    } else {
      show('unlock-panel', true);
      const showCreate = !s.keystore_exists;
      $('unlock-title').textContent = showCreate ? 'Create or import wallet' : 'Open wallet';
      show('unlock-existing', !showCreate);
      show('unlock-create', showCreate);
      show('unlock-import', false);
    }

    // Panels.
    show('addr-empty', !s.unlocked);
    show('addr-filled', s.unlocked);
    show('balance-empty', !s.unlocked);
    show('balance-filled', s.unlocked);
    show('send-empty', !s.unlocked);
    show('send-filled', s.unlocked);
    show('utxos-empty', !s.unlocked);
    show('utxos-filled', s.unlocked);

    if (s.unlocked) {
      $('addr-fp').textContent = s.fingerprint || '';
      $('addr-full').textContent = s.address || '';
      $('addr-account').textContent = String(s.account ?? 0);
      // Cache-bust the QR endpoint per session so it reflects the
      // currently-unlocked address.
      $('fp-qr').src = '/api/wallet/fingerprint.svg?cb=' + Date.now();
      refreshBalance();
      refreshUtxos();
    } else {
      $('fp-qr').removeAttribute('src');
      $('full-qr-panel').style.display = 'none';
      $('full-qr-grid').innerHTML = '';
    }
  }

  async function refreshBalance() {
    try {
      const b = await api('GET', '/api/balance');
      $('balance-value').textContent = b.balance.toLocaleString();
    } catch (e) {
      $('balance-value').textContent = '—';
      toast('balance: ' + e.message, 'err');
    }
  }

  async function refreshUtxos() {
    try {
      const r = await api('GET', '/api/utxos');
      const tb = $('utxos-tbody');
      tb.innerHTML = '';
      const stealth = r.stealth || [];
      const plain = r.plain || [];
      if (!stealth.length && !plain.length) {
        tb.innerHTML = '<tr><td colspan="4" class="note" style="padding:12px">No UTXOs detected yet (no stealth payments and no plain p2pkh allocations).</td></tr>';
        return;
      }
      const rows = [];
      for (const u of stealth) rows.push({ kind: 'stealth', ...u });
      for (const u of plain) rows.push({ kind: 'plain', ...u });
      rows.sort((a, b) => b.value - a.value);
      for (const u of rows) {
        const tr = document.createElement('tr');
        const short = u.tx_id.slice(0,12) + '…' + u.tx_id.slice(-8);
        const kindBadge = u.kind === 'stealth'
          ? '<span style="color:var(--accent-2)">stealth</span>'
          : '<span style="color:var(--accent)">plain</span>';
        tr.innerHTML = '<td>' + kindBadge + '</td><td class="mono">' + short + '</td><td class="mono">' + u.output_index + '</td><td style="text-align:right" class="mono">' + u.value.toLocaleString() + '</td>';
        tb.appendChild(tr);
      }
    } catch (e) {
      toast('utxos: ' + e.message, 'err');
    }
  }

  // ---- Wire up controls ----
  $('btn-unlock').onclick = async () => {
    try {
      await api('POST', '/api/wallet/unlock', { password: $('unlock-pw').value });
      $('unlock-pw').value = '';
      toast('Wallet unlocked', 'ok');
      refreshStatus();
    } catch (e) { toast(e.message, 'err'); }
  };

  $('btn-create').onclick = async () => {
    try {
      const r = await api('POST', '/api/wallet/create', { password: $('create-pw').value });
      $('create-pw').value = '';
      $('mnemonic-words').textContent = r.mnemonic;
      $('mnemonic-panel').style.display = '';
      toast('Wallet created — back up your mnemonic before sending.', 'ok');
      // Wallet is created but locked; user must unlock.
      refreshStatus();
    } catch (e) { toast(e.message, 'err'); }
  };

  $('btn-import').onclick = async () => {
    try {
      await api('POST', '/api/wallet/import', {
        phrase: $('import-phrase').value.trim(),
        password: $('import-pw').value,
      });
      $('import-phrase').value = '';
      $('import-pw').value = '';
      toast('Wallet imported — unlock to use it.', 'ok');
      refreshStatus();
    } catch (e) { toast(e.message, 'err'); }
  };

  $('btn-lock').onclick = async () => {
    await api('POST', '/api/wallet/lock');
    toast('Locked.', 'ok');
    refreshStatus();
  };

  $('btn-refresh').onclick = refreshBalance;
  $('btn-refresh-utxos').onclick = refreshUtxos;

  $('btn-send').onclick = async () => {
    const to_address = $('send-to').value.trim();
    const amount = parseInt($('send-amount').value, 10);
    const fee = parseInt($('send-fee').value || '0', 10);
    if (!to_address || !amount) { toast('Recipient and amount are required', 'err'); return; }
    $('btn-send').disabled = true;
    try {
      const r = await api('POST', '/api/send', { to_address, amount, fee });
      toast('Sent: tx ' + r.tx_id.slice(0,16) + '…', 'ok');
      $('send-to').value = '';
      $('send-amount').value = '';
      refreshBalance();
      refreshUtxos();
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      $('btn-send').disabled = false;
    }
  };

  $('btn-copy-addr').onclick = () => {
    navigator.clipboard?.writeText($('addr-full').textContent);
    toast('Address copied', 'ok');
  };

  $('btn-download-qvaddr').onclick = async () => {
    try {
      const r = await fetch('/api/wallet/address.qvaddr');
      if (!r.ok) throw new Error('http ' + r.status);
      const blob = await r.blob();
      const cd = r.headers.get('content-disposition') || '';
      const m = /filename=\"([^\"]+)\"/.exec(cd);
      const filename = (m && m[1]) || 'address.qvaddr';
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url; a.download = filename;
      document.body.appendChild(a); a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast('Saved ' + filename, 'ok');
    } catch (e) { toast(e.message, 'err'); }
  };

  $('btn-show-full-qr').onclick = async () => {
    try {
      const r = await api('GET', '/api/wallet/address-qr?parts=2');
      const grid = $('full-qr-grid');
      grid.innerHTML = '';
      r.parts.forEach((svg, i) => {
        const wrap = document.createElement('div');
        wrap.style.cssText = 'background:#fff;border-radius:6px;padding:10px;text-align:center';
        wrap.innerHTML = svg + '<div class="note" style="margin-top:6px;color:#1a2347">Part ' + (i+1) + ' / ' + r.total + '</div>';
        grid.appendChild(wrap);
      });
      $('full-qr-panel').style.display = '';
      $('full-qr-panel').scrollIntoView({behavior: 'smooth', block: 'start'});
    } catch (e) { toast(e.message, 'err'); }
  };

  $('btn-hide-full-qr').onclick = () => {
    $('full-qr-panel').style.display = 'none';
    $('full-qr-grid').innerHTML = '';
  };

  $('send-qvaddr-file').onchange = async (ev) => {
    const file = ev.target.files && ev.target.files[0];
    if (!file) return;
    try {
      const text = await file.text();
      const r = await api('POST', '/api/wallet/import-qvaddr', { json: text });
      $('send-to').value = r.address;
      const label = r.label ? (' [' + r.label + ']') : '';
      toast('Recipient loaded: ' + r.fingerprint + label, 'ok');
    } catch (e) {
      toast('Could not load .qvaddr: ' + e.message, 'err');
    } finally {
      ev.target.value = '';
    }
  };
  $('btn-copy-mnemonic').onclick = () => {
    navigator.clipboard?.writeText($('mnemonic-words').textContent);
    toast('Mnemonic copied', 'ok');
  };
  $('btn-mnemonic-done').onclick = () => {
    $('mnemonic-panel').style.display = 'none';
    $('mnemonic-words').textContent = '';
  };

  $('show-import').onclick = () => { show('unlock-existing', false); show('unlock-import', true); };
  $('show-import-from-create').onclick = () => { show('unlock-create', false); show('unlock-import', true); };
  $('back-to-unlock').onclick = refreshStatus;

  refreshStatus();
})();
</script>
</body>
</html>
"###;
