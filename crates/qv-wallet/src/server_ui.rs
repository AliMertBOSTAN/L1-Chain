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
  <div id="status-bar">
    <span class="dot"></span>
    <span id="status-text">locked</span>
    <span id="user-text"></span>
    <span id="rpc-text"></span>
  </div>
</header>

<main>
  <!-- Multi-tenant auth panel (login + register). Shown when /api/status says multi_tenant=true. -->
  <section class="panel" id="auth-panel" style="display:none">
    <h2 id="auth-title">Sign in</h2>
    <div class="note" style="margin-bottom:8px">
      <strong>Demo (custodial)</strong> — bu sunucu cüzdanını kendi RAM'inde tutar. Gerçek
      parayla kullanma; yalnızca test ortamı için.
    </div>
    <div class="row" style="margin-bottom:8px">
      <button class="ghost" id="auth-tab-login" style="margin:0">Login</button>
      <button class="ghost" id="auth-tab-register" style="margin:0">Register</button>
    </div>
    <div id="auth-login-view">
      <label>Kullanıcı adı</label>
      <input id="login-username" type="text" autocomplete="username">
      <label>Parola</label>
      <input id="login-password" type="password" autocomplete="current-password">
      <button id="btn-login">Giriş yap</button>
    </div>
    <div id="auth-register-view" class="hidden">
      <label>Kullanıcı adı (3-32 char, a-z 0-9 _ -)</label>
      <input id="reg-username" type="text" autocomplete="username">
      <label>Parola (min 8 char)</label>
      <input id="reg-password" type="password" autocomplete="new-password">
      <label>Mevcut mnemonic ile içeri aktar (opsiyonel)</label>
      <textarea id="reg-phrase" placeholder="(boş bırakırsan yeni cüzdan üretilir)"></textarea>
      <button id="btn-register">Kayıt ol</button>
    </div>
  </section>

  <!-- Single-user unlock / create / import (only one shown at a time depending on state) -->
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
        <dd>
          <div class="row" style="gap:6px; align-items:center">
            <select id="account-picker" style="flex:1; background:var(--panel-2); color:var(--text); border:1px solid var(--border); border-radius:6px; padding:6px 8px; font:inherit">
              <option value="0">#0</option>
            </select>
            <button class="ghost" id="btn-new-account" title="Derive a new account from the same mnemonic" style="margin:0; flex:0 0 auto; padding:6px 10px">+ New</button>
          </div>
          <div class="note" id="account-hint" style="margin-top:4px">Switch accounts to use a different stealth address from the same mnemonic.</div>
        </dd>
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
      <div class="row" style="margin-top:6px">
        <button class="ghost" id="btn-download-view-key" title="Hand to an auditor to grant scan-only visibility. Cannot spend.">Download view key (audit)</button>
      </div>
      <hr>
      <button class="ghost danger" id="btn-lock"><span id="btn-lock-label">Lock wallet</span></button>
    </div>
  </section>

  <section class="panel">
    <h2>Balance</h2>
    <div id="balance-empty" class="note">Unlock the wallet to view your balance.</div>
    <div id="balance-filled" class="hidden">
      <div class="balance-big"><span id="balance-value">0</span><small>units</small></div>
      <div class="row">
        <button class="ghost" id="btn-refresh">Refresh</button>
        <button class="ghost" id="btn-faucet" title="Devnet only — drip funds from DEVNET_TEST_MNEMONIC account 0 into this wallet.">Get devnet test funds</button>
      </div>
      <div id="faucet-form" class="hidden" style="margin-top:10px; padding:12px; background:var(--panel-2); border:1px solid var(--border); border-radius:6px">
        <div class="note" style="margin-bottom:6px">
          <strong>Devnet only.</strong> Drips from the well-known
          <code>DEVNET_TEST_MNEMONIC</code> (the standard "abandon … art" phrase).
          Funds appear as a stealth UTXO under this wallet's view key.
        </div>
        <div class="row">
          <div>
            <label>Amount (units)</label>
            <input id="faucet-amount" type="number" min="1" value="1000000">
          </div>
          <div>
            <label>Fee (units)</label>
            <input id="faucet-fee" type="number" min="0" value="1000">
          </div>
        </div>
        <div class="row" style="margin-top:6px">
          <button id="btn-faucet-go">Drip</button>
          <button class="ghost" id="btn-faucet-cancel">Cancel</button>
        </div>
      </div>
    </div>
  </section>

  <section class="panel">
    <h2>Send</h2>
    <div id="send-empty" class="note">Unlock the wallet to send a transfer.</div>
    <div id="send-filled" class="hidden">
      <label>Recipient stealth address (qvst1…)</label>
      <textarea id="send-to" placeholder="qvst1..."></textarea>
      <div class="row" style="margin-bottom:4px">
        <label style="margin:0; flex:0 0 auto" for="send-contact-picker">…or pick a contact:</label>
        <select id="send-contact-picker" style="background:var(--panel-2); color:var(--text); border:1px solid var(--border); border-radius:6px; padding:8px 10px">
          <option value="">(none)</option>
        </select>
      </div>
      <div class="row" style="margin-bottom:4px">
        <label style="margin:0; flex:0 0 auto" for="send-qvaddr-file">…or load a <code>.qvaddr</code> file:</label>
        <input id="send-qvaddr-file" type="file" accept=".qvaddr,application/json" style="background:transparent; border:none; padding:0">
      </div>
      <div class="row" style="margin-bottom:4px">
        <button class="ghost" id="btn-scan-qr" style="margin:0; flex:0 0 auto" title="Scan a multi-part stealth address QR with the device camera.">Scan QR with camera</button>
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
        <thead><tr><th>Kind</th><th>TX ID</th><th>Out</th><th style="text-align:right">Value</th><th>Disclose</th></tr></thead>
        <tbody id="utxos-tbody"></tbody>
      </table>
      <button class="ghost" id="btn-refresh-utxos">Re-scan</button>
    </div>
  </section>

  <section class="panel span2" id="history-panel">
    <h2>Transaction history</h2>
    <div id="history-empty" class="note">Unlock the wallet to see past sends and incoming stealth receipts.</div>
    <div id="history-filled" class="hidden">
      <div class="note" style="margin-bottom:8px">
        <strong>Sent</strong> entries are persisted locally (encrypted with your wallet
        password). <strong>Received</strong> entries are derived from the current UTXO
        scan — they disappear from the table once you spend that output.
      </div>
      <div class="row" style="margin-bottom:6px; align-items:center">
        <label style="margin:0; flex:0 0 auto" for="history-filter">Filter:</label>
        <select id="history-filter" style="background:var(--panel-2); color:var(--text); border:1px solid var(--border); border-radius:6px; padding:6px 8px">
          <option value="all">All entries</option>
          <option value="sent">Sent only</option>
          <option value="received">Received only</option>
          <option value="account">This account only</option>
        </select>
        <button class="ghost" id="btn-refresh-history" style="margin:0">Refresh</button>
      </div>
      <table>
        <thead>
          <tr>
            <th>When</th>
            <th>Kind</th>
            <th>Acct</th>
            <th>Counterparty</th>
            <th>TX ID</th>
            <th style="text-align:right">Amount</th>
            <th style="text-align:right">Fee</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody id="history-tbody"></tbody>
      </table>
    </div>
  </section>

  <section class="panel span2" id="contacts-panel">
    <h2>Address book</h2>
    <div id="contacts-empty" class="note">Unlock the wallet to manage labelled stealth-address contacts.</div>
    <div id="contacts-filled" class="hidden">
      <div class="note" style="margin-bottom:8px">
        Encrypted alongside the keystore (Argon2id + AES-256-GCM). Anyone with the
        wallet password can read the labels and addresses; nobody else can.
      </div>
      <table>
        <thead><tr><th>Label</th><th>Fingerprint</th><th>Notes</th><th></th></tr></thead>
        <tbody id="contacts-tbody"></tbody>
      </table>
      <hr>
      <div class="row" style="margin-bottom:6px">
        <input id="contacts-add-label" type="text" placeholder="label (e.g. alice)">
        <input id="contacts-add-notes" type="text" placeholder="optional notes">
      </div>
      <label>Stealth address (qvst1…)</label>
      <textarea id="contacts-add-address" placeholder="qvst1..."></textarea>
      <div class="row" style="margin-top:8px">
        <button id="btn-contacts-add">Add contact</button>
        <button class="ghost" id="btn-contacts-refresh">Refresh</button>
      </div>
    </div>
  </section>

  <section class="panel span2" id="disclose-panel" style="display:none">
    <h2>Selective disclosure</h2>
    <div class="note">
      Producing a <code>.qvdisclose</code> file lets a verifier confirm that
      <strong>you</strong> received a specific stealth UTXO, optionally
      revealing the amount — without sharing your view key, mnemonic, or
      spend authority. Multiple disclosures from the same account share the
      same spend public key, so they can be linked back to one wallet.
    </div>
    <dl class="kv" style="margin-top:10px">
      <dt>Outpoint</dt>
      <dd class="mono" id="disclose-outpoint"></dd>
      <dt>On-chain value</dt>
      <dd class="mono" id="disclose-value"></dd>
    </dl>
    <label>Disclose amount? (optional — leave blank to keep amount private)</label>
    <input id="disclose-amount" type="number" min="0" placeholder="(none)">
    <label>Label (optional)</label>
    <input id="disclose-label" type="text" placeholder="e.g. invoice #42">
    <div class="row" style="margin-top:12px">
      <button id="btn-disclose-create">Create &amp; download .qvdisclose</button>
      <button class="ghost" id="btn-disclose-cancel">Cancel</button>
    </div>
  </section>

  <section class="panel span2" id="qr-scanner-panel" style="display:none">
    <h2>Scan recipient QR</h2>
    <div class="note">
      Point the camera at each QR code in turn. Multi-part addresses
      (<code>QVADDR1:k/N</code>) accumulate automatically — the panel
      closes once every part has been captured.
    </div>
    <div id="qr-scanner-unsupported" class="hidden note" style="color:var(--danger); margin-top:8px">
      This browser does not expose <code>BarcodeDetector</code>. Chrome /
      Edge on a device with a camera work best; in other browsers, use
      the <code>.qvaddr</code> file upload instead.
    </div>
    <div id="qr-scanner-stage" class="hidden" style="margin-top:12px">
      <video id="qr-video" autoplay playsinline muted style="width:100%; max-width:480px; border-radius:8px; background:#000; display:block"></video>
      <div id="qr-scanner-status" class="note" style="margin-top:8px">Starting camera…</div>
      <div id="qr-scanner-parts" class="mono" style="margin-top:4px; font-size:11px"></div>
    </div>
    <div class="row" style="margin-top:12px">
      <button class="ghost" id="btn-qr-scanner-cancel">Close scanner</button>
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

  // ---- Auth token (multi-tenant only) ----
  const TOKEN_KEY = 'qv_session_token';
  function getToken() { try { return localStorage.getItem(TOKEN_KEY); } catch (_) { return null; } }
  function setToken(t) {
    try {
      if (t) localStorage.setItem(TOKEN_KEY, t);
      else localStorage.removeItem(TOKEN_KEY);
    } catch (_) {}
  }

  // ---- Clipboard (HTTPS-only API + execCommand fallback for plain HTTP/LAN) ----
  async function copyText(text) {
    // Modern API works only in secure contexts (HTTPS or localhost). On
    // a LAN URL like http://192.168.x.x:7777 it silently fails — fall
    // back to a hidden <textarea> + document.execCommand('copy').
    if (navigator.clipboard && window.isSecureContext) {
      try { await navigator.clipboard.writeText(text); return true; } catch (_) {}
    }
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.left = '-9999px';
    ta.style.top = '0';
    document.body.appendChild(ta);
    ta.focus();
    ta.select();
    ta.setSelectionRange(0, ta.value.length);
    let ok = false;
    try { ok = document.execCommand('copy'); } catch (_) {}
    document.body.removeChild(ta);
    return ok;
  }

  // ---- Authenticated fetch for non-JSON endpoints (SVG, file downloads) ----
  async function authFetch(path) {
    const headers = {};
    const tok = getToken();
    if (tok) headers['authorization'] = 'Bearer ' + tok;
    return fetch(path, { headers });
  }

  async function api(method, path, body) {
    const headers = { 'content-type': 'application/json' };
    const tok = getToken();
    if (tok) headers['authorization'] = 'Bearer ' + tok;
    const opt = { method, headers };
    if (body !== undefined) opt.body = JSON.stringify(body);
    const r = await fetch(path, opt);
    const ct = r.headers.get('content-type') || '';
    const data = ct.includes('application/json') ? await r.json() : await r.text();
    if (!r.ok) {
      // 401 ⇒ session expired or never logged in; drop the stale token so the
      // next refreshStatus() shows the login screen.
      if (r.status === 401) {
        setToken(null);
      }
      const msg = (data && data.error) || r.statusText || ('http ' + r.status);
      throw new Error(msg);
    }
    return data;
  }

  async function refreshStatus() {
    const s = await api('GET', '/api/status');
    $('status-text').textContent = s.unlocked ? 'unlocked' : 'locked';
    $('rpc-text').textContent = 'node: ' + s.rpc_url;
    $('user-text').textContent = s.username ? ('user: ' + s.username) : '';
    document.getElementById('status-bar').classList.toggle('unlocked', s.unlocked);

    // Lock button label depends on mode.
    if ($('btn-lock-label')) {
      $('btn-lock-label').textContent = s.multi_tenant ? 'Logout' : 'Lock wallet';
    }

    // Pick auth UX based on server mode.
    if (s.multi_tenant) {
      // Multi-tenant: hide single-user panel via inline style (overrides .hidden).
      $('unlock-panel').style.display = 'none';
      if (s.unlocked) {
        $('auth-panel').style.display = 'none';
      } else {
        $('auth-panel').style.display = '';
        // Default to login tab.
        show('auth-login-view', true);
        show('auth-register-view', false);
        $('auth-title').textContent = 'Sign in';
      }
    } else {
      // Single-user: hide auth panel, clear any inline display on unlock-panel
      // so `show()` / `.hidden` class takes effect again.
      $('auth-panel').style.display = 'none';
      $('unlock-panel').style.display = '';
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
    show('contacts-empty', !s.unlocked);
    show('contacts-filled', s.unlocked);
    show('history-empty', !s.unlocked);
    show('history-filled', s.unlocked);

    if (s.unlocked) {
      $('addr-fp').textContent = s.fingerprint || '';
      $('addr-full').textContent = s.address || '';
      // Load the fingerprint QR via authenticated fetch + blob URL.
      // Plain <img src=...> doesn't include the Authorization header,
      // so in multi-tenant mode it would 401 silently.
      loadFingerprintQr();
      refreshAccounts(s.account ?? 0);
      refreshBalance();
      refreshUtxos();
      refreshContacts();
      refreshHistory();
    } else {
      $('fp-qr').removeAttribute('src');
      $('full-qr-panel').style.display = 'none';
      $('full-qr-grid').innerHTML = '';
      $('disclose-panel').style.display = 'none';
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
        tb.innerHTML = '<tr><td colspan="5" class="note" style="padding:12px">No UTXOs detected yet (no stealth payments and no plain p2pkh allocations).</td></tr>';
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
        // Selective disclosure only makes sense for stealth UTXOs — plain
        // p2pkh outputs are already visible on-chain by design.
        const discloseCell = u.kind === 'stealth'
          ? '<button class="ghost discloseBtn" data-tx="' + u.tx_id + '" data-out="' + u.output_index + '" data-value="' + u.value + '">Disclose</button>'
          : '<span class="note">—</span>';
        tr.innerHTML = '<td>' + kindBadge + '</td><td class="mono">' + short + '</td><td class="mono">' + u.output_index + '</td><td style="text-align:right" class="mono">' + u.value.toLocaleString() + '</td><td>' + discloseCell + '</td>';
        tb.appendChild(tr);
      }
      // Bind disclose buttons after the rows are in the DOM.
      tb.querySelectorAll('button.discloseBtn').forEach(btn => {
        btn.addEventListener('click', () => {
          const tx = btn.dataset.tx;
          const out = parseInt(btn.dataset.out, 10);
          const value = parseInt(btn.dataset.value, 10);
          openDisclosePanel(tx, out, value);
        });
      });
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
    // Mode-aware: in multi-tenant we logout (remove session); in single-user
    // we lock the global wallet.
    try {
      const s = await api('GET', '/api/status');
      if (s.multi_tenant) {
        try { await api('POST', '/api/auth/logout'); } catch (_) {}
        setToken(null);
        toast('Logged out.', 'ok');
      } else {
        await api('POST', '/api/wallet/lock');
        toast('Locked.', 'ok');
      }
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      refreshStatus();
    }
  };

  // ---- Multi-tenant auth handlers ----
  $('auth-tab-login').onclick = () => {
    show('auth-login-view', true);
    show('auth-register-view', false);
    $('auth-title').textContent = 'Sign in';
  };
  $('auth-tab-register').onclick = () => {
    show('auth-login-view', false);
    show('auth-register-view', true);
    $('auth-title').textContent = 'Create account';
  };

  $('btn-login').onclick = async () => {
    const username = $('login-username').value.trim();
    const password = $('login-password').value;
    if (!username || !password) {
      toast('Kullanıcı adı + parola gerekli', 'err');
      return;
    }
    try {
      const r = await api('POST', '/api/auth/login', { username, password });
      setToken(r.session_token);
      $('login-password').value = '';
      toast('Giriş başarılı: ' + r.username, 'ok');
      refreshStatus();
    } catch (e) {
      toast(e.message, 'err');
    }
  };

  $('btn-register').onclick = async () => {
    const username = $('reg-username').value.trim();
    const password = $('reg-password').value;
    const phrase = $('reg-phrase').value.trim() || null;
    if (!username || !password) {
      toast('Kullanıcı adı + parola gerekli', 'err');
      return;
    }
    try {
      const body = { username, password };
      if (phrase) body.phrase = phrase;
      const r = await api('POST', '/api/auth/register', body);
      setToken(r.session_token);
      $('reg-password').value = '';
      $('reg-phrase').value = '';
      if (r.mnemonic) {
        $('mnemonic-words').textContent = r.mnemonic;
        $('mnemonic-panel').style.display = '';
        toast('Cüzdan oluşturuldu — mnemonic\'i yedekle!', 'ok');
      } else {
        toast('Hesap oluşturuldu: ' + r.username, 'ok');
      }
      refreshStatus();
    } catch (e) {
      toast(e.message, 'err');
    }
  };

  $('btn-refresh').onclick = refreshBalance;
  $('btn-refresh-utxos').onclick = refreshUtxos;
  $('btn-refresh-history').onclick = refreshHistory;
  $('history-filter').onchange = renderHistory;

  // ---- Devnet faucet ----
  $('btn-faucet').onclick = () => {
    show('faucet-form', true);
  };
  $('btn-faucet-cancel').onclick = () => {
    show('faucet-form', false);
  };
  $('btn-faucet-go').onclick = async () => {
    const amount = parseInt($('faucet-amount').value, 10);
    const fee = parseInt($('faucet-fee').value || '0', 10);
    if (!Number.isFinite(amount) || amount <= 0) {
      toast('Faucet amount must be a positive integer', 'err');
      return;
    }
    $('btn-faucet-go').disabled = true;
    try {
      const r = await api('POST', '/api/devnet/faucet', { amount, fee });
      toast('Faucet sent ' + r.amount.toLocaleString() + ' units (tx ' + r.tx_id.slice(0, 14) + '…)', 'ok');
      show('faucet-form', false);
      // Give the node a beat to apply the tx, then refresh.
      setTimeout(() => {
        refreshBalance();
        refreshUtxos();
        refreshHistory();
      }, 800);
    } catch (e) {
      toast('Faucet failed: ' + e.message, 'err');
    } finally {
      $('btn-faucet-go').disabled = false;
    }
  };

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
      refreshHistory();
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      $('btn-send').disabled = false;
    }
  };

  $('btn-copy-addr').onclick = async () => {
    const ok = await copyText($('addr-full').textContent);
    toast(ok ? 'Address copied' : 'Copy failed - tarayicin secure context degil', ok ? 'ok' : 'err');
  };

  // ---- Fingerprint QR loader (authenticated) ----
  let lastFpQrUrl = null;
  async function loadFingerprintQr() {
    const img = $('fp-qr');
    try {
      const r = await authFetch('/api/wallet/fingerprint.svg?cb=' + Date.now());
      if (!r.ok) {
        img.removeAttribute('src');
        return;
      }
      const blob = await r.blob();
      const url = URL.createObjectURL(blob);
      img.src = url;
      if (lastFpQrUrl) {
        try { URL.revokeObjectURL(lastFpQrUrl); } catch (_) {}
      }
      lastFpQrUrl = url;
    } catch (_) {
      img.removeAttribute('src');
    }
  }

  // ---- Multi-account picker ----
  async function refreshAccounts(currentAccount) {
    try {
      const r = await api('GET', '/api/wallet/accounts');
      const picker = $('account-picker');
      picker.innerHTML = '';
      const known = r.accounts || [];
      // Defensive: keystore should always have at least the current
      // account, but if for some reason the list comes back empty we still
      // need *some* selectable option so the dropdown isn't blank.
      const rows = known.length
        ? known
        : [{ account: currentAccount, fingerprint: '', address: '' }];
      for (const a of rows) {
        const opt = document.createElement('option');
        opt.value = String(a.account);
        const fpShort = a.fingerprint ? (' — ' + a.fingerprint.slice(0, 14) + '…') : '';
        opt.textContent = '#' + a.account + fpShort;
        if (a.account === currentAccount) opt.selected = true;
        picker.appendChild(opt);
      }
      picker.dataset.nextAccount = String(r.next_account ?? (currentAccount + 1));
      $('account-hint').textContent =
        'Known accounts: ' + (known.length || 1) +
        ' · next new index: #' + picker.dataset.nextAccount;
    } catch (e) {
      toast('accounts: ' + e.message, 'err');
    }
  }

  $('account-picker').onchange = async (ev) => {
    const acct = parseInt(ev.target.value, 10);
    if (!Number.isFinite(acct)) return;
    try {
      await api('POST', '/api/wallet/switch-account', { account: acct });
      toast('Switched to account #' + acct, 'ok');
      refreshStatus();
    } catch (e) {
      toast('switch: ' + e.message, 'err');
    }
  };

  $('btn-new-account').onclick = async () => {
    const picker = $('account-picker');
    const next = parseInt(picker.dataset.nextAccount || '0', 10);
    if (!confirm('Derive a new account #' + next + ' from the same mnemonic?\n\nThis stores a fresh view keypair in the keystore and switches to it.')) {
      return;
    }
    try {
      await api('POST', '/api/wallet/switch-account', { account: next });
      toast('Derived & switched to account #' + next, 'ok');
      refreshStatus();
    } catch (e) {
      toast('new account: ' + e.message, 'err');
    }
  };

  // ---- Transaction history ----
  let historyCache = { entries: [], current_account: 0 };
  function fmtTimestamp(ts) {
    if (!ts) return '—';
    try { return new Date(ts * 1000).toLocaleString(); } catch (_) { return String(ts); }
  }
  function renderHistory() {
    const tb = $('history-tbody');
    tb.innerHTML = '';
    const mode = $('history-filter').value;
    let rows = historyCache.entries.slice();
    if (mode === 'sent') rows = rows.filter(e => e.kind === 'sent');
    else if (mode === 'received') rows = rows.filter(e => e.kind === 'received');
    else if (mode === 'account') rows = rows.filter(e => e.account === historyCache.current_account);
    if (!rows.length) {
      tb.innerHTML = '<tr><td colspan="8" class="note" style="padding:12px">No history entries match the current filter.</td></tr>';
      return;
    }
    for (const e of rows) {
      const tr = document.createElement('tr');
      const kindBadge = e.kind === 'sent'
        ? '<span style="color:var(--danger)">sent</span>'
        : '<span style="color:var(--accent-2)">received</span>';
      const short = e.tx_id.slice(0, 12) + '…' + e.tx_id.slice(-6);
      const counterparty = e.counterparty_label
        ? escapeHtml(e.counterparty_label)
        : (e.counterparty_fingerprint
            ? escapeHtml(e.counterparty_fingerprint.slice(0, 14) + '…')
            : '—');
      const fee = (e.fee !== null && e.fee !== undefined)
        ? Number(e.fee).toLocaleString()
        : '—';
      tr.innerHTML =
          '<td class="mono" style="font-size:11px">' + escapeHtml(fmtTimestamp(e.timestamp)) + '</td>'
        + '<td>' + kindBadge + '</td>'
        + '<td class="mono">#' + e.account + '</td>'
        + '<td class="mono" title="' + escapeHtml(e.counterparty_fingerprint || '') + '">' + counterparty + '</td>'
        + '<td class="mono" title="' + escapeHtml(e.tx_id) + '">' + short + '</td>'
        + '<td style="text-align:right" class="mono">' + Number(e.amount).toLocaleString() + '</td>'
        + '<td style="text-align:right" class="mono">' + fee + '</td>'
        + '<td class="note">' + escapeHtml(e.status || '') + '</td>';
      tb.appendChild(tr);
    }
  }
  async function refreshHistory() {
    try {
      const r = await api('GET', '/api/history');
      historyCache = {
        entries: r.entries || [],
        current_account: r.current_account ?? 0,
      };
      renderHistory();
    } catch (e) {
      toast('history: ' + e.message, 'err');
    }
  }

  // ---- Address book ----
  async function refreshContacts() {
    try {
      const r = await api('GET', '/api/contacts');
      const tb = $('contacts-tbody');
      const picker = $('send-contact-picker');
      tb.innerHTML = '';
      picker.innerHTML = '<option value="">(none)</option>';
      if (!r.contacts.length) {
        tb.innerHTML = '<tr><td colspan="4" class="note" style="padding:12px">No contacts yet — add one below.</td></tr>';
        return;
      }
      for (const c of r.contacts) {
        const tr = document.createElement('tr');
        const fpShort = c.fingerprint.slice(0, 12) + '…';
        const notes = c.notes || '';
        tr.innerHTML = '<td class="mono">' + escapeHtml(c.label) + '</td>'
                     + '<td class="mono" title="' + escapeHtml(c.fingerprint) + '">' + fpShort + '</td>'
                     + '<td>' + escapeHtml(notes) + '</td>'
                     + '<td><button class="ghost contactRemoveBtn" data-label="' + escapeHtml(c.label) + '">Remove</button></td>';
        tb.appendChild(tr);

        const opt = document.createElement('option');
        opt.value = c.address;
        opt.textContent = c.label + '  (' + fpShort + ')';
        picker.appendChild(opt);
      }
      tb.querySelectorAll('button.contactRemoveBtn').forEach(btn => {
        btn.addEventListener('click', async () => {
          const label = btn.dataset.label;
          if (!confirm('Remove contact `' + label + '`?')) return;
          try {
            await api('POST', '/api/contacts/remove', { label });
            toast('Removed `' + label + '`', 'ok');
            refreshContacts();
          } catch (e) { toast(e.message, 'err'); }
        });
      });
    } catch (e) {
      toast('contacts: ' + e.message, 'err');
    }
  }

  function escapeHtml(s) {
    return String(s || '')
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  $('btn-contacts-add').onclick = async () => {
    const label = $('contacts-add-label').value.trim();
    const address = $('contacts-add-address').value.trim();
    const notes = $('contacts-add-notes').value.trim() || null;
    if (!label || !address) {
      toast('label + address are required', 'err');
      return;
    }
    try {
      await api('POST', '/api/contacts', { label, address, notes });
      $('contacts-add-label').value = '';
      $('contacts-add-address').value = '';
      $('contacts-add-notes').value = '';
      toast('Added `' + label + '`', 'ok');
      refreshContacts();
    } catch (e) { toast(e.message, 'err'); }
  };
  $('btn-contacts-refresh').onclick = refreshContacts;

  $('send-contact-picker').onchange = (ev) => {
    const addr = ev.target.value;
    if (addr) $('send-to').value = addr;
  };

  // ---- Audit-mode view-key export ----
  $('btn-download-view-key').onclick = async () => {
    try {
      const r = await authFetch('/api/wallet/view-key.qvview');
      if (!r.ok) {
        const data = await r.json().catch(() => null);
        throw new Error((data && data.error) || ('http ' + r.status));
      }
      const blob = await r.blob();
      const cd = r.headers.get('content-disposition') || '';
      const m = /filename=\"([^\"]+)\"/.exec(cd);
      const filename = (m && m[1]) || 'view-key.qvview';
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url; a.download = filename;
      document.body.appendChild(a); a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast('View key exported: ' + filename, 'ok');
    } catch (e) {
      toast('view key: ' + e.message, 'err');
    }
  };

  // ---- Selective disclosure panel ----
  let pendingDisclose = null;
  function openDisclosePanel(tx_id, output_index, value) {
    pendingDisclose = { tx_id, output_index, value };
    $('disclose-outpoint').textContent = tx_id.slice(0, 16) + '…' + tx_id.slice(-8) + ':' + output_index;
    $('disclose-value').textContent = value.toLocaleString() + ' units';
    $('disclose-amount').value = '';
    $('disclose-label').value = '';
    $('disclose-panel').style.display = '';
    $('disclose-panel').scrollIntoView({behavior: 'smooth', block: 'start'});
  }
  $('btn-disclose-cancel').onclick = () => {
    $('disclose-panel').style.display = 'none';
    pendingDisclose = null;
  };
  $('btn-disclose-create').onclick = async () => {
    if (!pendingDisclose) return;
    const body = {
      tx_id: pendingDisclose.tx_id,
      output_index: pendingDisclose.output_index,
      label: $('disclose-label').value.trim() || null,
    };
    const amtStr = $('disclose-amount').value.trim();
    if (amtStr) {
      const amt = parseInt(amtStr, 10);
      if (Number.isFinite(amt) && amt >= 0) body.amount = amt;
    }
    try {
      const r = await api('POST', '/api/wallet/disclose', body);
      const blob = new Blob([r.qvdisclose_json], { type: 'application/json' });
      const filename = 'disclose-' + pendingDisclose.tx_id.slice(0, 8) + '-' + pendingDisclose.output_index + '.qvdisclose';
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url; a.download = filename;
      document.body.appendChild(a); a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast('Disclosure saved: ' + filename, 'ok');
      $('disclose-panel').style.display = 'none';
      pendingDisclose = null;
    } catch (e) {
      toast('disclose: ' + e.message, 'err');
    }
  };

  $('btn-download-qvaddr').onclick = async () => {
    try {
      const r = await authFetch('/api/wallet/address.qvaddr');
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
  // ---- QR scanner (camera) ----
  let qrScanState = null;
  let qrScanTimer = null;
  let qrScanStream = null;

  function renderQrPartsStatus() {
    if (!qrScanState) return;
    const captured = qrScanState.collected.size;
    const total = qrScanState.total;
    $('qr-scanner-status').textContent =
        total ? ('Captured ' + captured + ' / ' + total + ' parts')
              : 'Looking for QR codes…';
    const have = Array.from(qrScanState.collected.keys()).sort((a, b) => a - b);
    $('qr-scanner-parts').textContent =
        total ? ('Parts: ' + have.map(k => '#' + k).join(', ') + (have.length < total ? ' (missing others)' : '')) : '';
  }

  async function processQrPayload(raw) {
    if (!raw || !raw.startsWith('QVADDR1:')) return;
    const body = raw.slice('QVADDR1:'.length);
    const colon = body.indexOf(':');
    if (colon < 0) return;
    const kn = body.slice(0, colon);
    const slash = kn.indexOf('/');
    if (slash < 0) return;
    const k = parseInt(kn.slice(0, slash), 10);
    const N = parseInt(kn.slice(slash + 1), 10);
    if (!Number.isFinite(k) || !Number.isFinite(N) || k < 1 || k > N) return;
    if (!qrScanState) return;
    if (qrScanState.total === null) {
      qrScanState.total = N;
    } else if (qrScanState.total !== N) {
      $('qr-scanner-status').textContent = 'Conflicting part-count: expected ' + qrScanState.total + ', got ' + N + ' — close and retry.';
      return;
    }
    if (!qrScanState.collected.has(k)) {
      qrScanState.collected.set(k, raw);
      renderQrPartsStatus();
    }
    if (qrScanState.collected.size === N) {
      stopQrScanner();
      const parts = [];
      for (let i = 1; i <= N; i++) parts.push(qrScanState.collected.get(i));
      try {
        const r = await api('POST', '/api/wallet/qr-reassemble', { parts });
        $('send-to').value = r.address;
        toast('Recipient address loaded from QR (' + N + ' parts)', 'ok');
        $('qr-scanner-panel').style.display = 'none';
      } catch (e) {
        toast('QR reassemble: ' + e.message, 'err');
      }
    }
  }

  function stopQrScanner() {
    if (qrScanTimer) { clearInterval(qrScanTimer); qrScanTimer = null; }
    if (qrScanStream) {
      qrScanStream.getTracks().forEach(t => t.stop());
      qrScanStream = null;
    }
    const video = $('qr-video');
    if (video) video.srcObject = null;
  }

  async function startQrScanner() {
    $('qr-scanner-panel').style.display = '';
    $('qr-scanner-panel').scrollIntoView({behavior: 'smooth', block: 'start'});
    if (!('BarcodeDetector' in window)) {
      $('qr-scanner-unsupported').classList.remove('hidden');
      $('qr-scanner-stage').classList.add('hidden');
      return;
    }
    $('qr-scanner-unsupported').classList.add('hidden');
    $('qr-scanner-stage').classList.remove('hidden');
    qrScanState = { collected: new Map(), total: null };
    renderQrPartsStatus();
    const video = $('qr-video');
    try {
      qrScanStream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: { ideal: 'environment' } },
        audio: false,
      });
    } catch (e) {
      $('qr-scanner-status').textContent = 'Camera error: ' + (e.message || e);
      return;
    }
    video.srcObject = qrScanStream;
    try {
      await video.play();
    } catch (_) { /* autoplay race; harmless */ }
    let detector;
    try {
      detector = new BarcodeDetector({ formats: ['qr_code'] });
    } catch (e) {
      $('qr-scanner-status').textContent = 'BarcodeDetector init failed: ' + (e.message || e);
      stopQrScanner();
      return;
    }
    $('qr-scanner-status').textContent = 'Looking for QR codes…';
    qrScanTimer = setInterval(async () => {
      try {
        const codes = await detector.detect(video);
        for (const c of codes) {
          await processQrPayload(c.rawValue);
        }
      } catch (_) { /* one bad frame, keep going */ }
    }, 250);
  }

  $('btn-scan-qr').onclick = startQrScanner;
  $('btn-qr-scanner-cancel').onclick = () => {
    stopQrScanner();
    $('qr-scanner-panel').style.display = 'none';
  };

  $('btn-copy-mnemonic').onclick = async () => {
    const ok = await copyText($('mnemonic-words').textContent);
    toast(ok ? 'Mnemonic copied' : 'Copy failed - tarayicin secure context degil', ok ? 'ok' : 'err');
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
