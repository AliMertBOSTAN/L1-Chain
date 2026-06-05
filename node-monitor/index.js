#!/usr/bin/env node
/*
 * QuantumVault L1 — Node Monitor GUI
 * ----------------------------------
 * Bağımlılıksız (saf Node.js) bir izleme sunucusu. devnet düğümlerinin
 * JSON-RPC ve Prometheus uçlarını proxy'ler, work4/nodeN.log dosyalarını
 * okuyup ayrıştırır ve tarayıcı paneline JSON API sağlar.
 *
 * Çalıştırma — bu klasörün ("node-monitor") içinden:
 *   npm start                           (önerilen — veya: node index.js)
 *   node index.js --port 8080           (farklı port)
 *   node index.js --work "C:/.../devnet/work4"   (farklı log klasörü)
 * Ardından tarayıcıda: http://127.0.0.1:7070
 *
 * Düğümleri/ portu özelleştirmek için yanına monitor.config.json koyabilirsiniz:
 *   { "port": 7070, "workDir": "...", "nodes": [
 *       { "name": "node0", "host": "127.0.0.1", "rpc": 8545, "met": 9601, "p2p": 17001 } ] }
 */
'use strict';

const http = require('http');
const fs = require('fs');
const path = require('path');

const ROOT = __dirname;
const PUBLIC_DIR = path.join(ROOT, 'public');

// ---------------------------------------------------------------------------
// Yapılandırma
// ---------------------------------------------------------------------------
function parseArgs() {
  const a = process.argv.slice(2), o = {};
  for (let i = 0; i < a.length; i++) {
    if (a[i] === '--work') o.workDir = a[++i];
    else if (a[i] === '--port') o.port = +a[++i];
    else if (a[i] === '--host') o.bindHost = a[++i];
  }
  return o;
}
const ARGS = parseArgs();

// run-devnet.ps1 ile aynı portlar.
const NODES = [
  { name: 'node0', host: '127.0.0.1', rpc: 8545, met: 9601, p2p: 17001 },
  { name: 'node1', host: '127.0.0.1', rpc: 8546, met: 9602, p2p: 17002 },
  { name: 'node2', host: '127.0.0.1', rpc: 8547, met: 9603, p2p: 17003 },
  { name: 'node3', host: '127.0.0.1', rpc: 8548, met: 9604, p2p: 17004 },
];

const CFG = {
  port: ARGS.port || +process.env.QV_MONITOR_PORT || 7070,
  bindHost: ARGS.bindHost || '127.0.0.1',
  workDir: ARGS.workDir || process.env.QV_DEVNET_WORK ||
           path.join(ROOT, '..', 'devnet', 'work4'),
  slotMs: 500,        // config/devnet.toml slot_duration_ms
  epochSlots: 100,    // config/devnet.toml epoch_slots
  kFinality: 5,       // config/devnet.toml k_finality
  pollMs: 2000,
  forkWindow: 48,     // fork ağacında gösterilen yükseklik sayısı
  historyLen: 240,    // grafikler için tutulan örnek sayısı
};

// İsteğe bağlı yapılandırma dosyası.
const cfgFile = path.join(ROOT, 'monitor.config.json');
if (fs.existsSync(cfgFile)) {
  try {
    const j = JSON.parse(fs.readFileSync(cfgFile, 'utf8'));
    if (Array.isArray(j.nodes) && j.nodes.length) {
      NODES.length = 0;
      j.nodes.forEach(n => NODES.push(Object.assign({ host: '127.0.0.1' }, n)));
    }
    if (j.workDir) CFG.workDir = j.workDir;
    if (j.port) CFG.port = j.port;
    if (j.slotMs) CFG.slotMs = j.slotMs;
    if (j.epochSlots) CFG.epochSlots = j.epochSlots;
    if (j.kFinality) CFG.kFinality = j.kFinality;
  } catch (e) {
    console.error('monitor.config.json okunamadı:', e.message);
  }
}

// ---------------------------------------------------------------------------
// JSON-RPC + Prometheus istemcileri
// ---------------------------------------------------------------------------
function rpcRaw(node, payload) {
  return new Promise((resolve) => {
    const body = JSON.stringify(payload);
    const req = http.request({
      host: node.host, port: node.rpc, method: 'POST', path: '/',
      headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body) },
    }, (res) => {
      let d = '';
      res.on('data', c => d += c);
      res.on('end', () => { try { resolve(JSON.parse(d)); } catch (e) { resolve(null); } });
    });
    req.setTimeout(4000, () => { req.destroy(); resolve(null); });
    req.on('error', () => resolve(null));
    req.write(body);
    req.end();
  });
}

async function rpc(node, method, params) {
  const r = await rpcRaw(node, { jsonrpc: '2.0', id: 1, method, params: params || [] });
  if (!r) return null;
  return r.result !== undefined ? r.result : null;
}

// JSON-RPC toplu istek (jsonrpsee batch destekler); başarısız olursa sıralı.
async function rpcBatch(node, calls) {
  if (!calls.length) return [];
  const payload = calls.map((c, i) => ({ jsonrpc: '2.0', id: i, method: c.method, params: c.params || [] }));
  const r = await rpcRaw(node, payload);
  if (Array.isArray(r)) {
    const out = new Array(calls.length).fill(null);
    for (const item of r) {
      if (item && typeof item.id === 'number')
        out[item.id] = item.result !== undefined ? item.result : null;
    }
    return out;
  }
  const out = [];
  for (const c of calls) out.push(await rpc(node, c.method, c.params));
  return out;
}

function scrape(node) {
  return new Promise((resolve) => {
    const req = http.get({ host: node.host, port: node.met, path: '/' }, (res) => {
      let d = '';
      res.on('data', c => d += c);
      res.on('end', () => {
        const out = {};
        for (const line of d.split('\n')) {
          const t = line.trim();
          if (!t || t[0] === '#') continue;
          const sp = t.lastIndexOf(' ');
          if (sp < 0) continue;
          const v = parseFloat(t.slice(sp + 1));
          if (!isNaN(v)) out[t.slice(0, sp)] = v;
        }
        resolve(out);
      });
    });
    req.setTimeout(4000, () => { req.destroy(); resolve({}); });
    req.on('error', () => resolve({}));
  });
}

// İçinde `needle` geçen tüm metrikleri toplar (etiketli serileri birleştirir).
function metricSum(m, needle) {
  let s = 0, found = false;
  for (const k in m) if (k.indexOf(needle) >= 0) { s += m[k]; found = true; }
  return found ? s : null;
}
// `base{label="x"} v` satırlarından label kırılımı çıkarır.
function metricByLabel(m, base, label) {
  const out = {};
  for (const k in m) {
    if (k.indexOf(base) !== 0) continue;
    const mm = k.match(new RegExp(label + '="([^"]*)"'));
    if (mm) out[mm[1]] = (out[mm[1]] || 0) + m[k];
  }
  return out;
}

// ---------------------------------------------------------------------------
// Tip yardımcıları — hash'ler 32-byte dizi olarak gelir, hex'e çeviririz.
// ---------------------------------------------------------------------------
function hashHex(v) {
  if (v == null) return null;
  if (typeof v === 'string') return v;
  if (Array.isArray(v)) return v.map(b => (b & 255).toString(16).padStart(2, '0')).join('');
  if (typeof v === 'object') { const x = Object.values(v); if (x.length === 1) return hashHex(x[0]); }
  return String(v);
}
function num(v) {
  if (v == null) return null;
  if (typeof v === 'number') return v;
  if (typeof v === 'object') { const x = Object.values(v); return x.length ? num(x[0]) : null; }
  const n = +v;
  return isNaN(n) ? null : n;
}
function bytesOf(v) {  // Vec<u8> -> dizi
  if (Array.isArray(v)) return v;
  if (v && typeof v === 'object') { const x = Object.values(v); if (x.length === 1 && Array.isArray(x[0])) return x[0]; }
  return [];
}

// ---------------------------------------------------------------------------
// Blok önbelleği + aralık çekme
// ---------------------------------------------------------------------------
const blockCache = new Map();   // "node|height" -> block JSON
function bkey(node, h) { return node.name + '|' + h; }

async function fetchBlocksRange(node, from, to, safeBelow) {
  const res = new Map();
  const need = [];
  for (let h = from; h <= to; h++) {
    const k = bkey(node, h);
    if (blockCache.has(k)) res.set(h, blockCache.get(k));
    else need.push(h);
  }
  const CH = 24;
  for (let i = 0; i < need.length; i += CH) {
    const chunk = need.slice(i, i + CH);
    const out = await rpcBatch(node, chunk.map(h => ({ method: 'qv_getBlockByHeight', params: [h] })));
    chunk.forEach((h, idx) => {
      const blk = out[idx];
      if (blk) {
        res.set(h, blk);
        if (h <= safeBelow) blockCache.set(bkey(node, h), blk);  // kesinleşmiş blok değişmez
      }
    });
  }
  if (blockCache.size > 6000) {                     // önbelleği sınırla
    const keys = [...blockCache.keys()].slice(0, 2000);
    keys.forEach(k => blockCache.delete(k));
  }
  return res;
}

// ---------------------------------------------------------------------------
// Log okuma ve ayrıştırma
// ---------------------------------------------------------------------------
function ansiStrip(s) { return s.replace(/\x1b\[[0-9;]*m/g, ''); }

function readTail(file, maxBytes) {
  try {
    const st = fs.statSync(file);
    const start = Math.max(0, st.size - maxBytes);
    const len = st.size - start;
    const fd = fs.openSync(file, 'r');
    const buf = Buffer.alloc(len);
    fs.readSync(fd, buf, 0, len, start);
    fs.closeSync(fd);
    return buf.toString('utf8');
  } catch (e) { return null; }
}

function cleanWrapped(v) {            // "Height(793)" -> "793", "BlockHash(ab..cd)" -> "ab..cd"
  if (v == null) return v;
  const m = String(v).match(/^[A-Za-z0-9_]+\((.*)\)$/);
  return m ? m[1] : v;
}

function parseLogLine(raw, nodeName) {
  const line = ansiStrip(raw).trim();
  if (!line) return null;
  let ts = null, level = 'INFO', target = '', msg = line;
  const m = line.match(/^(\d{4}-\d\d-\d\dT[\d:.]+Z)\s+(INFO|WARN|ERROR|DEBUG|TRACE)\s+\S+\s+([\w:]+):\s*(.*)$/);
  if (m) { ts = m[1]; level = m[2]; target = m[3]; msg = m[4]; }
  else {
    const m2 = line.match(/^(\d{4}-\d\d-\d\dT[\d:.]+Z)\s+(INFO|WARN|ERROR|DEBUG|TRACE)\s+(.*)$/);
    if (m2) { ts = m2[1]; level = m2[2]; msg = m2[3]; }
  }
  const low = msg.toLowerCase();
  let kind = 'other';
  if (low.indexOf('block accepted') === 0) kind = 'block-accepted';
  else if (low.indexOf('failed to process block') === 0) kind = 'block-rejected';
  else if (low.indexOf('transaction accepted') >= 0) kind = 'tx';
  else if (low.indexOf('connected peer') >= 0 || low.indexOf('dialing seed') >= 0) kind = 'peer';
  else if (low.indexOf('failed to publish') >= 0 || low.indexOf('not publishing') >= 0) kind = 'gossip';
  else if (low.indexOf('slot leader schedule') >= 0) kind = 'leader';
  else if (low.indexOf('genesis') >= 0 || low.indexOf('node starting') >= 0 ||
           low.indexOf('initialized') >= 0 || low.indexOf('listening') >= 0 ||
           low.indexOf('main loop') >= 0) kind = 'startup';

  const fields = {};
  let fm; const re = /(\w+)=(\S+)/g;
  while ((fm = re.exec(msg))) fields[fm[1]] = cleanWrapped(fm[2]);
  const rm = msg.match(/expected ([0-9a-f]{8,64}), got ([0-9a-f]{8,64})/);
  if (rm) { fields.expected = rm[1]; fields.got = rm[2]; }
  return { node: nodeName, ts, level, target, kind, msg, fields };
}

function readNodeEvents(node, maxEvents) {
  const file = path.join(CFG.workDir, node.name + '.log');
  const text = readTail(file, 220 * 1024);
  if (text == null) return [];
  const out = [];
  const lines = text.split('\n');
  for (let i = Math.max(0, lines.length - maxEvents * 3); i < lines.length; i++) {
    const ev = parseLogLine(lines[i], node.name);
    if (ev && ev.ts) out.push(ev);
  }
  return out.slice(-maxEvents);
}

// ---------------------------------------------------------------------------
// Canlı durum (arka plan yoklayıcı tarafından beslenir)
// ---------------------------------------------------------------------------
const state = {
  ts: 0,
  nodes: [],
  consensus: {},
  leader: {},
  convergence: {},
  history: [],            // grafikler için halka tampon
  producerMap: {},        // producer_key_hash hex -> node index
  hashHeightMap: {},      // kabul edilen blok hash hex -> yükseklik
  forkHistoryMap: new Map(), // "height|got" -> kalıcı fork kaydı
  txHistoryMap: new Map(),   // txId -> kalıcı işlem kaydı (mempool'da görülenler)
  workDirExists: fs.existsSync(CFG.workDir),
};

// Bir red mesajından fork nedenini ve kısa açıklamasını çıkarır.
function reasonInfo(msg) {
  const low = (msg || '').toLowerCase();
  if (low.indexOf('prev_hash mismatch') >= 0)
    return { reason: 'prev_hash', text: 'İki slot lideri birbirine çok yakın anlarda blok üretti. ' +
      'Gelen blok, bu düğümün zincir ucundan farklı bir üst bloğa bağlıydı; yoğunluk ağırlıklı en uzun ' +
      'zincir kuralı kazanan dalı seçince bu dal terk edildi (orphan).' };
  if (low.indexOf('merkle') >= 0)
    return { reason: 'merkle', text: 'Blok gövdesi başlıktaki Merkle köküyle uyuşmadı — bozuk ya da ' +
      'değiştirilmiş blok reddedildi.' };
  if (low.indexOf('signature') >= 0 || low.indexOf('vrf') >= 0 || low.indexOf('kes') >= 0)
    return { reason: 'proof', text: 'Bloğun VRF/KES kanıtı doğrulanamadı — üretici o slotun geçerli ' +
      'lideri değildi.' };
  if (low.indexOf('gap') >= 0 || low.indexOf('non-sequential') >= 0)
    return { reason: 'gap', text: 'Blok yüksekliği zincirle ardışık değildi — eksik bir ara blok var.' };
  return { reason: 'other', text: 'Blok bu düğümün zincirini geçerli biçimde uzatmadığı için reddedildi.' };
}

// Reddedilen bir blok olayını kalıcı fork geçmişine ekler (idempotent).
function recordFork(ev) {
  const f = ev.fields || {};
  if (!f.got) return;
  let height = null;
  if (f.expected && state.hashHeightMap[f.expected] != null)
    height = state.hashHeightMap[f.expected] + 1;       // reddeden düğümün ucu + 1
  else if (state.hashHeightMap[f.got] != null)
    height = state.hashHeightMap[f.got] + 1;
  const key = (height != null ? height : '?') + '|' + f.got;
  let e = state.forkHistoryMap.get(key);
  if (!e) {
    const ri = reasonInfo(ev.msg);
    e = { key, height, got: f.got, expected: f.expected || null,
          reason: ri.reason, reasonText: ri.text,
          firstTs: ev.ts, lastTs: ev.ts, nodes: [] };
    state.forkHistoryMap.set(key, e);
  } else if (height != null && e.height == null) {
    e.height = height;
  }
  if (ev.node && e.nodes.indexOf(ev.node) < 0) e.nodes.push(ev.node);
  if (ev.ts && (!e.firstTs || ev.ts < e.firstTs)) e.firstTs = ev.ts;
  if (ev.ts && (!e.lastTs || ev.ts > e.lastTs)) e.lastTs = ev.ts;
}

// Tüm düğüm loglarını tarar: kabul edilen blokların hash->yükseklik haritası
// ve reddedilen blokların kalıcı fork geçmişi. Her yoklamada çağrılır.
function scanForkLogs() {
  for (const n of NODES) {
    for (const ev of readNodeEvents(n, 800)) {
      const f = ev.fields || {};
      if (ev.kind === 'block-accepted' && f.hash && f.height != null) {
        if (/^[0-9a-f]{64}$/.test(String(f.hash))) {
          const ht = +f.height;
          if (!isNaN(ht)) state.hashHeightMap[String(f.hash)] = ht;
        }
      } else if (ev.kind === 'block-rejected') {
        recordFork(ev);
      }
    }
  }
  const hk = Object.keys(state.hashHeightMap);     // bellek sınırı
  if (hk.length > 9000) {
    hk.sort((a, b) => state.hashHeightMap[a] - state.hashHeightMap[b]);
    for (let i = 0; i < 3000; i++) delete state.hashHeightMap[hk[i]];
  }
  if (state.forkHistoryMap.size > 1500) {
    const arr = [...state.forkHistoryMap.entries()].sort((a, b) => (a[1].height || 0) - (b[1].height || 0));
    for (let i = 0; i < 400; i++) state.forkHistoryMap.delete(arr[i][0]);
  }
}

async function pollOnce() {
  const per = [];
  const pendingByNode = [];
  for (let i = 0; i < NODES.length; i++) {
    const n = NODES[i];
    const [tip, mem, met, pending] = await Promise.all([
      rpc(n, 'qv_getTip'),
      rpc(n, 'qv_getMempoolStatus'),
      scrape(n),
      rpc(n, 'qv_getPendingTransactions'),
    ]);
    pendingByNode.push({ idx: i, ids: Array.isArray(pending) ? pending : [] });
    // Tip bloğunu önce HASH ile çek (block_store doğrudan lookup — uçtaki blok
    // için güvenilir); bulunamazsa yükseklik indeksinden dene.
    let tipBlock = null;
    if (tip && tip.block_hash)
      tipBlock = await rpc(n, 'qv_getBlockByHash', [tip.block_hash]);
    if (!tipBlock && tip && typeof tip.height === 'number')
      tipBlock = await rpc(n, 'qv_getBlockByHeight', [tip.height]);

    let tipSlot = null, producer = null;
    if (tipBlock && tipBlock.header) {
      tipSlot = num(tipBlock.header.slot);
      producer = hashHex(tipBlock.header.producer_key_hash);
      if (tipSlot != null && producer && !/^0+$/.test(producer))
        state.producerMap[producer] = tipSlot % NODES.length;
    }

    const valSum = metricSum(met, 'block_validate_seconds_sum');
    const valCnt = metricSum(met, 'block_validate_seconds_count');
    per.push({
      index: i, name: n.name, rpc: n.rpc, p2p: n.p2p, met: n.met,
      up: tip != null,
      height: tip ? tip.height : null,
      hash: tip ? tip.block_hash : null,
      tipTimestamp: tip ? tip.timestamp : null,
      tipSlot,
      mempoolClear: mem ? mem.clear_pool_size : null,
      mempoolEnc: mem ? mem.encrypted_pool_size : null,
      mempoolValue: mem ? mem.total_value : null,
      pending: Array.isArray(pending) ? pending.length : null,
      peers: metricSum(met, 'peers_connected'),
      blocksValidated: metricSum(met, 'blocks_validated'),
      blocksRejected: metricSum(met, 'blocks_rejected_reason') ?? metricSum(met, 'blocks_rejected'),
      rejectReasons: metricByLabel(met, 'blocks_rejected_reason', 'reason'),
      gossipIn: metricSum(met, 'gossip_messages_in'),
      txReceived: metricSum(met, 'tx_received'),
      txRejected: metricSum(met, 'tx_rejected'),
      tipHeightMetric: metricSum(met, 'tip_height'),
      mempoolSizeMetric: metricSum(met, 'mempool_size'),
      validateAvgMs: (valSum != null && valCnt) ? (valSum / valCnt) * 1000 : null,
    });
  }

  // Konsensüs (ilk erişilebilir düğümden).
  let stake = null, nonce = null;
  for (const n of NODES) {
    if (!stake) stake = await rpc(n, 'qv_getStakeDistribution');
    if (!nonce) nonce = await rpc(n, 'qv_getEpochNonce');
    if (stake && nonce) break;
  }

  const heights = per.filter(p => p.up && p.height != null).map(p => p.height);
  const hashes = per.filter(p => p.up && p.hash).map(p => p.hash);
  const globalTip = heights.length ? Math.max(...heights) : null;

  // Lider — en yüksek düğümün tip slotundan round-robin.
  let leader = {};
  const top = per.filter(p => p.up && p.height === globalTip && p.tipSlot != null)[0];
  if (top) {
    const s = top.tipSlot;
    leader = {
      tipSlot: s,
      tipHeight: globalTip,
      currentLeader: s % NODES.length,
      schedule: [],
      epochSlot: s % CFG.epochSlots,
      slotsToEpoch: CFG.epochSlots - (s % CFG.epochSlots),
    };
    for (let d = -6; d <= 16; d++) {
      const slot = s + d;
      if (slot < 0) continue;
      leader.schedule.push({ slot, leader: slot % NODES.length, offset: d, current: d === 0 });
    }
  }

  let conv = 'unknown';
  const hs = [...new Set(heights)], uh = [...new Set(hashes)];
  if (heights.length && uh.length === 1 && hs.length === 1) conv = 'ok';
  else if (heights.length && Math.max(...hs) - Math.min(...hs) <= 1) conv = 'syncing';
  else if (heights.length) conv = 'diverged';

  state.ts = Date.now();
  state.nodes = per;
  state.consensus = {
    stake: stake || null,
    nonce: nonce || null,
    globalTip,
    finalizedHeight: globalTip != null ? Math.max(0, globalTip - CFG.kFinality) : null,
    kFinality: CFG.kFinality,
    slotMs: CFG.slotMs,
    epochSlots: CFG.epochSlots,
  };
  state.leader = leader;
  state.convergence = { status: conv, heights: hs.sort((a, b) => a - b), distinctHashes: uh.length };
  state.workDirExists = fs.existsSync(CFG.workDir);

  // Kalıcı fork geçmişini güncelle (loglardan).
  try { scanForkLogs(); } catch (e) { /* log yoksa sessiz geç */ }

  // Kalıcı işlem geçmişini güncelle (mempool'da görülen işlemler).
  try { await updateTxHistory(pendingByNode); } catch (e) { /* sessiz geç */ }

  // Grafik geçmişi.
  state.history.push({
    t: state.ts,
    nodes: per.map(p => ({
      h: p.height, peers: p.peers, bv: p.blocksValidated, br: p.blocksRejected,
      gin: p.gossipIn, txr: p.txReceived, mem: p.mempoolClear,
    })),
  });
  if (state.history.length > CFG.historyLen) state.history.shift();
}

async function pollLoop() {
  try { await pollOnce(); }
  catch (e) { console.error('yoklama hatası:', e.message); }
  setTimeout(pollLoop, CFG.pollMs);
}

// ---------------------------------------------------------------------------
// Fork ağacı kurucu
// ---------------------------------------------------------------------------
async function buildForks(count) {
  const tips = [];
  for (const n of NODES) tips.push(await rpc(n, 'qv_getTip'));
  const liveIdx = [];
  tips.forEach((t, i) => { if (t && typeof t.height === 'number') liveIdx.push(i); });
  if (!liveIdx.length)
    return { ok: false, chains: [], offline: NODES.map((_, i) => i), forkHistory: [], rejectedSeen: 0, nodeCount: NODES.length };

  const globalTip = Math.max(...liveIdx.map(i => tips[i].height));
  const safeBelow = globalTip - CFG.kFinality - 2;

  // Her düğüm için KENDİ ucuna göre pencere çek (geride kalan zincir de görünür).
  const win = {};
  for (const i of liveIdx) {
    const n = NODES[i], t = tips[i], nodeTip = t.height;
    const from = Math.max(0, nodeTip - count);
    const blocks = await fetchBlocksRange(n, from, nodeTip, safeBelow);
    const hashAt = new Map();   // height -> hash (çocuğun prev_hash'inden türetilir)
    for (let h = from; h <= nodeTip; h++) {
      const blk = blocks.get(h);
      if (!blk || !blk.header) continue;
      let hash;
      if (h === nodeTip) hash = t.block_hash;
      else {
        const child = blocks.get(h + 1);
        hash = child && child.header ? hashHex(child.header.prev_hash) : null;
      }
      if (hash) hashAt.set(h, hash);
    }
    win[i] = { idx: i, tip: nodeTip, tipHash: t.block_hash, from, blocks, hashAt };
  }

  // Düğümleri zincirlere kümele: ortak (yükseklik, hash) paylaşan iki düğüm
  // aynı zincirdedir. union-find.
  const parent = {};
  liveIdx.forEach(i => parent[i] = i);
  const find = x => { while (parent[x] !== x) { parent[x] = parent[parent[x]]; x = parent[x]; } return x; };
  for (let a = 0; a < liveIdx.length; a++) {
    for (let b = a + 1; b < liveIdx.length; b++) {
      const wa = win[liveIdx[a]], wb = win[liveIdx[b]];
      const lo = Math.max(wa.from, wb.from), hi = Math.min(wa.tip, wb.tip);
      let same = false;
      for (let h = hi; h >= lo; h--) {
        const ha = wa.hashAt.get(h), hb = wb.hashAt.get(h);
        if (ha && hb && ha === hb) { same = true; break; }
      }
      if (same) parent[find(wa.idx)] = find(wb.idx);
    }
  }
  const groupMap = {};
  for (const i of liveIdx) { const r = find(i); (groupMap[r] = groupMap[r] || []).push(i); }
  // En yüksek uçtan en düşüğe sırala (ana zincir önce).
  const groups = Object.values(groupMap).sort((g1, g2) =>
    Math.max(...g2.map(i => win[i].tip)) - Math.max(...g1.map(i => win[i].tip)));

  // Her grup (zincir) için alt-grafik kur.
  const chains = groups.map((gnodes, ci) => {
    const tip = Math.max(...gnodes.map(i => win[i].tip));
    const from = Math.max(0, tip - count);
    const perHeight = new Map();
    for (const i of gnodes) {
      const w = win[i];
      for (let h = Math.max(from, w.from); h <= w.tip; h++) {
        const blk = w.blocks.get(h);
        const hash = w.hashAt.get(h);
        if (!blk || !blk.header || !hash) continue;
        const hdr = blk.header, slot = num(hdr.slot);
        const em = perHeight.get(h) || new Map();
        let e = em.get(hash);
        if (!e) {
          e = {
            height: h, hash, prevHash: hashHex(hdr.prev_hash), slot,
            leader: slot != null ? slot % NODES.length : null,
            producer: hashHex(hdr.producer_key_hash),
            txCount: (blk.transactions || []).length,
            timestamp: num(hdr.timestamp), nodes: [], rejected: false,
          };
          em.set(hash, e);
        }
        if (e.nodes.indexOf(i) < 0) e.nodes.push(i);
        perHeight.set(h, em);
      }
    }
    // Kanonik — grubun en yüksek ucundan geriye yürü.
    const topNode = gnodes.reduce((a, b) => (win[a].tip >= win[b].tip ? a : b));
    const canonical = new Set();
    let cH = win[topNode].tip, cHash = win[topNode].tipHash;
    while (cHash && cH >= from) {
      const em = perHeight.get(cH);
      if (!em || !em.has(cHash)) break;
      canonical.add(cH + ':' + cHash);
      cHash = em.get(cHash).prevHash;
      cH--;
    }
    // Bu zincirin penceresine düşen terk edilmiş (orphan) dalları yerleştir.
    for (const e of state.forkHistoryMap.values()) {
      if (e.height == null || e.height < from || e.height > tip) continue;
      if (!perHeight.has(e.height) && !perHeight.has(e.height - 1)) continue;
      const em = perHeight.get(e.height) || new Map();
      const id = 'orphan:' + e.got.slice(0, 16);
      if (!em.has(id)) {
        em.set(id, {
          height: e.height, hash: id, prevHash: e.got, slot: null, leader: null,
          producer: null, txCount: 0, timestamp: null, nodes: e.nodes.slice(),
          rejected: true, reason: e.reason, reasonText: e.reasonText,
          firstTs: e.firstTs, lastTs: e.lastTs,
        });
        perHeight.set(e.height, em);
      }
    }
    const heightRows = [], forkHeights = [];
    for (let h = from; h <= tip; h++) {
      const em = perHeight.get(h);
      if (!em) continue;
      const blocks = [...em.values()].map(e => ({ ...e, canonical: canonical.has(h + ':' + e.hash) }));
      if (blocks.filter(b => !b.rejected).length > 1 || blocks.some(b => b.rejected))
        forkHeights.push(h);
      heightRows.push({ height: h, blocks });
    }
    return {
      id: ci, label: String.fromCharCode(65 + ci),     // A, B, C...
      nodes: gnodes.slice().sort((a, b) => a - b),
      tipHeight: tip, from, to: tip,
      heights: heightRows, forkHeights,
    };
  });

  const offline = [];
  NODES.forEach((_, i) => { if (liveIdx.indexOf(i) < 0) offline.push(i); });

  const forkHistory = [...state.forkHistoryMap.values()]
    .sort((a, b) => (b.height || 0) - (a.height || 0))
    .slice(0, 250);

  return {
    ok: true, nodeCount: NODES.length,
    partitioned: chains.length > 1,
    chains, offline, globalTip,
    forkHistory, rejectedSeen: state.forkHistoryMap.size,
  };
}

// ---------------------------------------------------------------------------
// İşlem ayrıntıları
// ---------------------------------------------------------------------------
function txDetail(tx) {
  const inputs = (tx.inputs || []).map(inp => {
    const op = (inp && inp.prev_output) || {};
    return {
      txid: hashHex(op.tx_id),
      index: num(op.index),
      witnessLen: bytesOf(inp && inp.witness).length,
    };
  });
  const outputs = (tx.outputs || []).map(o => {
    const sc = bytesOf(o && o.locking_script);
    return {
      value: num(o && o.value),
      scriptLen: sc.length,
      scriptHex: sc.slice(0, 16).map(b => (b & 255).toString(16).padStart(2, '0')).join(''),
      hasDatum: o && o.datum != null,
      hasStealth: o && o.stealth_info != null,
    };
  });
  return {
    version: num(tx.version),
    fee: num(tx.fee),
    lockTime: num(tx.lock_time),
    inputs, outputs,
    totalOut: outputs.reduce((s, o) => s + (o.value || 0), 0),
  };
}

// Mempool'daki bekleyen (henüz bir bloğa girmemiş) işlemler.
async function buildPending() {
  const map = new Map();                 // txId -> { txId, nodes: [] }
  for (let i = 0; i < NODES.length; i++) {
    const ids = await rpc(NODES[i], 'qv_getPendingTransactions');
    if (!Array.isArray(ids)) continue;
    for (const id of ids) {
      if (typeof id !== 'string') continue;
      let e = map.get(id);
      if (!e) { e = { txId: id, nodes: [] }; map.set(id, e); }
      if (e.nodes.indexOf(i) < 0) e.nodes.push(i);
    }
  }
  const out = [];
  for (const e of map.values()) {
    let tx = null;
    for (const ni of e.nodes) { tx = await rpc(NODES[ni], 'qv_getTx', [e.txId]); if (tx) break; }
    out.push(Object.assign({ txId: e.txId, nodes: e.nodes }, tx ? txDetail(tx) : {}));
  }
  return out;
}

// Kalıcı işlem geçmişi: monitör çalışırken mempool'da görülen her işlem
// kaydedilir; mempool'dan ayrılınca akıbeti belirlenir (zincire girdi / düştü)
// ve kayıt — bloğa girse de düşse de — listede kalır.
async function updateTxHistory(pendingByNode) {
  const nowTs = Date.now();
  const current = new Map();               // txId -> [node idx]
  for (const pn of pendingByNode) {
    for (const id of pn.ids) {
      if (typeof id !== 'string') continue;
      if (!current.has(id)) current.set(id, []);
      const arr = current.get(id);
      if (arr.indexOf(pn.idx) < 0) arr.push(pn.idx);
    }
  }
  // Mevcut bekleyenleri kaydet / güncelle.
  for (const [txId, nodes] of current) {
    let e = state.txHistoryMap.get(txId);
    if (!e) {
      e = { txId, status: 'pending', firstSeen: nowTs, lastSeen: nowTs,
            resolvedAt: null, nodes: nodes.slice(), detail: null };
      state.txHistoryMap.set(txId, e);
    } else {
      e.lastSeen = nowTs;
      if (e.status === 'pending') e.nodes = nodes.slice();
    }
    if (!e.detail) {
      for (const ni of nodes) {
        const tx = await rpc(NODES[ni], 'qv_getTx', [txId]);
        if (tx) { e.detail = txDetail(tx); break; }
      }
    }
  }
  // Mempool'dan ayrılan 'pending' kayıtların akıbetini belirle.
  for (const e of state.txHistoryMap.values()) {
    if (e.status !== 'pending' || current.has(e.txId)) continue;
    let found = null;
    for (const n of NODES) { found = await rpc(n, 'qv_getTx', [e.txId]); if (found) break; }
    if (found) {
      e.status = 'confirmed';                // hâlâ bulunabiliyor → bir bloğa girdi
      if (!e.detail) e.detail = txDetail(found);
    } else {
      e.status = 'dropped';                  // bulunamıyor → bloğa girmeden düştü
    }
    e.resolvedAt = nowTs;
  }
  if (state.txHistoryMap.size > 10000) {     // bellek sınırı (artırıldı)
    const arr = [...state.txHistoryMap.entries()].sort((a, b) => a[1].firstSeen - b[1].firstSeen);
    for (let i = 0; i < 2000; i++) state.txHistoryMap.delete(arr[i][0]);
  }
}

// Blok zincirindeki txleri kalıcı geçmişe yazar — pollarda atlanan
// (mempoola hiç girmemiş gibi görünen) hızlı tx'leri yakalamak için.
// `confirmed` statüsüyle kaydedilirler; mempoolda görülmüş olanlar
// dokunulmaz (durumları zaten doğru).
function recordBlockTxs(blocks) {
  const nowTs = Date.now();
  for (const [h, blk] of blocks.entries()) {
    if (!blk || !blk.transactions) continue;
    const slot = num(blk.header && blk.header.slot);
    for (const tx of blk.transactions) {
      const txId = tx && (tx.tx_id || tx.id);
      // qv-core tx'leri tx_id alanı bincode/JSON'da sıkça atlandığı
      // için hash hesaplamayı doğru yapamayız; bu yüzden detail içinde
      // outputs/inputs ile eşleme zor. Bunun yerine her tx'i kendi
      // outpoint'i (input[0]) üzerinden yaklaşık benzersiz key'le
      // saklarız. Sadece detail (içerik) yakalamak istiyoruz.
      const inp0 = (tx.inputs || [])[0];
      const op = inp0 && inp0.prev_output;
      const key = op
        ? 'blk:' + h + ':' + hashHex(op.tx_id) + ':' + num(op.index)
        : 'blk:' + h + ':#' + Math.random().toString(36).slice(2, 10);
      if (state.txHistoryMap.has(key)) continue;
      state.txHistoryMap.set(key, {
        txId: key,
        status: 'confirmed',
        firstSeen: nowTs, lastSeen: nowTs, resolvedAt: nowTs,
        height: h, slot,
        nodes: [],
        detail: txDetail(tx),
      });
    }
  }
}

async function buildTransactions(count) {
  const top = state.nodes.filter(p => p.up && p.height != null).sort((a, b) => b.height - a.height)[0];
  if (!top) return { ok: false, blocks: [], pending: [], txHistory: [] };
  const node = NODES[top.index];
  const tip = top.height;
  const from = Math.max(0, tip - count);
  const safeBelow = tip - CFG.kFinality - 2;
  const blocks = await fetchBlocksRange(node, from, tip, safeBelow);
  if (!blocks.has(0)) {
    const g = await rpc(node, 'qv_getBlockByHeight', [0]);
    if (g) blocks.set(0, g);
  }
  // Bu pencerede görülen tüm tx'leri kalıcı geçmişe kopyala — pollarda
  // mempool'a düşmeden zincire giren tx'leri yakalamak için.
  recordBlockTxs(blocks);

  const out = [];
  for (const [h, blk] of [...blocks.entries()].sort((a, b) => b[0] - a[0])) {
    const txs = blk.transactions || [];
    if (!txs.length) continue;
    const slot = num(blk.header.slot);
    out.push({
      height: h, slot,
      leader: slot != null ? slot % NODES.length : null,
      producer: hashHex(blk.header.producer_key_hash),
      timestamp: num(blk.header.timestamp),
      txCount: txs.length,
      isGenesis: h === 0,
      txs: txs.map(txDetail),
    });
  }
  const pending = await buildPending();
  const txHistory = [...state.txHistoryMap.values()]
    .sort((a, b) => (b.lastSeen || 0) - (a.lastSeen || 0))
    .slice(0, 2000);
  return { ok: true, blocks: out, pending, txHistory, tip, scannedFrom: from };
}

// ---------------------------------------------------------------------------
// Zincir tarama (explorer) — blok / işlem / UTXO arama + genesis dağıtımı
// ---------------------------------------------------------------------------
function firstLiveNode() {
  for (const p of state.nodes) if (p.up) return NODES[p.index];
  return NODES[0] || null;
}

// Bir Block JSON'undan paneller için düz bir ayrıntı nesnesi kurar.
function blockDetail(blk) {
  const h = blk.header || {};
  const slot = num(h.slot);
  return {
    height: num(h.height), slot,
    leader: slot != null ? slot % NODES.length : null,
    prevHash: hashHex(h.prev_hash),
    merkleRoot: hashHex(h.merkle_root),
    utxoCommitment: hashHex(h.utxo_commitment),
    producer: hashHex(h.producer_key_hash),
    timestamp: num(h.timestamp),
    version: num(h.version),
    vrfLen: bytesOf(h.vrf_proof).length,
    kesLen: bytesOf(h.kes_sig).length,
    txCount: (blk.transactions || []).length,
    txs: (blk.transactions || []).map(txDetail),
  };
}

async function buildScan(kind, q) {
  const node = firstLiveNode();
  if (!node) return { ok: false, error: 'Hiçbir düğüm çevrimiçi değil.' };
  q = (q || '').trim();
  if (!q) return { ok: false, error: 'Arama kutusu boş.' };

  if (kind === 'block') {
    let blk = null;
    if (/^\d+$/.test(q)) blk = await rpc(node, 'qv_getBlockByHeight', [+q]);
    else if (/^[0-9a-fA-F]{64}$/.test(q)) blk = await rpc(node, 'qv_getBlockByHash', [q.toLowerCase()]);
    else return { ok: false, error: 'Blok için yükseklik (sayı) ya da 64 haneli hash girin.' };
    if (!blk || !blk.header) return { ok: true, kind, q, found: false };
    return { ok: true, kind: 'block', q, found: true, block: blockDetail(blk) };
  }
  if (kind === 'tx') {
    if (!/^[0-9a-fA-F]{64}$/.test(q)) return { ok: false, error: 'İşlem kimliği 64 haneli hex olmalı.' };
    const tx = await rpc(node, 'qv_getTx', [q.toLowerCase()]);
    if (!tx) return { ok: true, kind, q, found: false };
    return { ok: true, kind: 'tx', q, found: true, tx: txDetail(tx) };
  }
  if (kind === 'utxo') {
    if (!/^[0-9a-fA-F]{64}#\d+$/.test(q))
      return { ok: false, error: 'UTXO outpoint biçimi: <işlem_id>#<indeks>  (örn. fa9ea5…#0)' };
    const u = await rpc(node, 'qv_getUtxo', [q.toLowerCase()]);
    if (!u) return { ok: true, kind, q, found: false };   // harcanmış ya da hiç olmamış
    return { ok: true, kind: 'utxo', q, found: true, utxo: u };
  }
  return { ok: false, error: 'Bilinmeyen arama türü.' };
}

// Genesis dağıtımı: ağın ilk fonlanmış UTXO'ları ve adlı cüzdanlar.
async function buildScanOverview() {
  const node = firstLiveNode();
  if (!node) return { ok: false, error: 'Hiçbir düğüm çevrimiçi değil.' };
  const g = await rpc(node, 'qv_getBlockByHeight', [0]);
  let outputs = [];
  if (g && g.transactions && g.transactions[0]) {
    outputs = (g.transactions[0].outputs || []).map((o, i) => ({
      index: i, value: num(o.value),
      hasDatum: o.datum != null, hasStealth: o.stealth_info != null,
    }));
  }
  // wallets.json (proje kökü) — genesis_txid + adlı cüzdanlar.
  let genesisTxid = null;
  const named = {};
  try {
    const wp = path.join(ROOT, '..', 'wallets.json');
    if (fs.existsSync(wp)) {
      const w = JSON.parse(fs.readFileSync(wp, 'utf8'));
      if (w.genesis_txid) genesisTxid = String(w.genesis_txid);
      for (const [name, ww] of Object.entries(w)) {
        if (ww && typeof ww === 'object' && ww.genesis_outpoint) {
          const idx = +String(ww.genesis_outpoint).split('#')[1];
          if (!isNaN(idx)) named[idx] = { name, pubkeyHash: ww.pubkey_hash || null, account: ww.account };
        }
      }
    }
  } catch (e) { /* yoksa geç */ }
  // genesis_txid biliniyorsa her çıktının canlı UTXO durumunu çek.
  if (genesisTxid) {
    for (const o of outputs) {
      o.outpoint = genesisTxid + '#' + o.index;
      const u = await rpc(node, 'qv_getUtxo', [o.outpoint]);
      o.unspent = u != null;
      o.liveValue = u ? num(u.value) : null;
      const nm = named[o.index];
      if (nm) { o.wallet = nm.name; o.pubkeyHash = nm.pubkeyHash; o.account = nm.account; }
    }
  }
  return { ok: true, genesisTxid, outputs, namedCount: Object.keys(named).length };
}

// ---------------------------------------------------------------------------
// HTTP sunucusu
// ---------------------------------------------------------------------------
const MIME = {
  '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8', '.json': 'application/json; charset=utf-8',
  '.jsx': 'application/javascript; charset=utf-8',
  '.svg': 'image/svg+xml', '.ico': 'image/x-icon',
};

function sendJSON(res, obj, code) {
  const body = JSON.stringify(obj);
  res.writeHead(code || 200, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(body);
}
function sendFile(res, file) {
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404); res.end('not found'); return; }
    res.writeHead(200, { 'Content-Type': MIME[path.extname(file)] || 'application/octet-stream' });
    res.end(data);
  });
}

async function readLogs(query) {
  const filterNode = query.node;
  const count = Math.min(+query.count || 200, 600);
  let all = [];
  for (const n of NODES) {
    if (filterNode && n.name !== filterNode) continue;
    all = all.concat(readNodeEvents(n, 300));
  }
  all.sort((a, b) => (a.ts || '').localeCompare(b.ts || ''));
  return {
    ok: true, workDir: CFG.workDir, workDirExists: fs.existsSync(CFG.workDir),
    events: all.slice(-count),
  };
}

const server = http.createServer(async (req, res) => {
  const u = new URL(req.url, 'http://x');
  const p = u.pathname;
  const q = Object.fromEntries(u.searchParams);
  try {
    if (p === '/api/state') {
      return sendJSON(res, {
        ok: true, ts: state.ts, config: {
          nodes: NODES, slotMs: CFG.slotMs, kFinality: CFG.kFinality,
          epochSlots: CFG.epochSlots, workDir: CFG.workDir,
        },
        nodes: state.nodes, consensus: state.consensus, leader: state.leader,
        convergence: state.convergence, history: state.history,
        producerMap: state.producerMap, workDirExists: state.workDirExists,
      });
    }
    if (p === '/api/forks') return sendJSON(res, await buildForks(Math.min(+q.count || CFG.forkWindow, 200)));
    if (p === '/api/transactions') return sendJSON(res, await buildTransactions(Math.min(+q.count || 600, 5000)));
    if (p === '/api/logs') return sendJSON(res, await readLogs(q));
    if (p === '/api/block') {
      const h = +q.height;
      let blk = null;
      for (const n of NODES) { blk = await rpc(n, 'qv_getBlockByHeight', [h]); if (blk) break; }
      if (!blk || !blk.header) return sendJSON(res, { ok: false }, 404);
      return sendJSON(res, Object.assign({ ok: true }, blockDetail(blk)));
    }
    if (p === '/api/scan') {
      if (q.kind === 'overview') return sendJSON(res, await buildScanOverview());
      return sendJSON(res, await buildScan(q.kind, q.q));
    }
    // statik dosyalar
    let file = p === '/' ? 'index.html' : p.replace(/^\/+/, '');
    file = path.normalize(file).replace(/^(\.\.[/\\])+/, '');
    const full = path.join(PUBLIC_DIR, file);
    if (full.startsWith(PUBLIC_DIR) && fs.existsSync(full) && fs.statSync(full).isFile())
      return sendFile(res, full);
    res.writeHead(404); res.end('not found');
  } catch (e) {
    console.error('istek hatası', p, e);
    sendJSON(res, { ok: false, error: e.message }, 500);
  }
});

server.listen(CFG.port, CFG.bindHost, () => {
  console.log('');
  console.log('  QuantumVault L1 — Node Monitor');
  console.log('  ------------------------------');
  console.log('  Panel : http://' + CFG.bindHost + ':' + CFG.port);
  console.log('  Düğüm : ' + NODES.map(n => n.name + '(rpc ' + n.rpc + ')').join(', '));
  console.log('  Loglar: ' + CFG.workDir + (state.workDirExists ? '' : '  [bulunamadı]'));
  console.log('');
  pollLoop();
});
