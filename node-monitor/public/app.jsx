/* QuantumVault L1 — Node Monitör  (React sürümü)
 * ----------------------------------------------
 * Tarayıcı içi Babel ile derlenir — build adımı / npm kurulumu yoktur.
 * Canlı veri React state üzerinden güncellenir: açık paneller, kaydırma
 * konumu ve seçimler yenileme sırasında korunur (sayfa "sıfırlanmaz").
 */
'use strict';
const { useState, useEffect, useRef, useMemo, useCallback } = React;

// ===========================================================================
// Yardımcılar
// ===========================================================================
const NODE_COLORS = ['#a78bfa', '#38bdf8', '#fbbf24', '#34d399', '#fb7185', '#818cf8'];
const KIND_TR = {
  'block-accepted': 'blok kabul', 'block-rejected': 'blok red', 'tx': 'işlem',
  'peer': 'eş', 'gossip': 'gossip', 'leader': 'lider', 'startup': 'başlangıç', 'other': 'diğer',
};
const TABS = [
  ['overview', 'Genel Bakış'], ['forks', 'Fork Ağacı'], ['consensus', 'Konsensüs & Lider'],
  ['perf', 'Performans'], ['tx', 'İşlemler'], ['logs', 'Loglar'], ['scan', 'Tarama'],
];

const nodeColor = (i) => NODE_COLORS[i % NODE_COLORS.length] || '#888';
const nodeName = (i) => (i == null ? '—' : 'node' + i);

function fmt(n) {
  if (n == null || isNaN(n)) return '—';
  return Math.round(n).toLocaleString('tr-TR');
}
function compact(n) {
  if (n == null || isNaN(n)) return '—';
  const a = Math.abs(n);
  if (a >= 1e12) return (n / 1e12).toFixed(2) + ' T';
  if (a >= 1e9) return (n / 1e9).toFixed(2) + ' B';
  if (a >= 1e6) return (n / 1e6).toFixed(2) + ' M';
  if (a >= 1e3) return (n / 1e3).toFixed(1) + ' K';
  return fmt(n);
}
function shortHash(hx, head) {
  if (!hx) return '—';
  hx = String(hx);
  if (hx.indexOf('orphan:') === 0) return 'orphan ' + hx.slice(7, 7 + 8) + '…';
  head = head || 8;
  return hx.length > head + 6 ? hx.slice(0, head) + '…' + hx.slice(-4) : hx;
}
function clockOf(iso) {
  if (!iso) return '—';
  const m = String(iso).match(/T(\d\d:\d\d:\d\d)/);
  return m ? m[1] : iso;
}
function fmtClock(ms) {
  if (!ms) return '—';
  return new Date(ms).toLocaleTimeString('tr-TR');
}
function reasonLabel(r) {
  return ({ prev_hash: 'prev_hash uyuşmazlığı', merkle: 'merkle hatası',
    proof: 'kanıt geçersiz', gap: 'yükseklik boşluğu', other: 'diğer' })[r] || r || 'diğer';
}
async function fetchJSON(url) {
  const r = await fetch(url, { cache: 'no-store' });
  if (!r.ok) throw new Error('HTTP ' + r.status);
  return r.json();
}

// ===========================================================================
// Statik SVG kuruculara (etkileşimsiz — dangerouslySetInnerHTML ile basılır)
// ===========================================================================
function leaderWheelSvg(n, current) {
  const cx = 84, cy = 84, r = 58;
  let s = `<circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="#2a2d44" stroke-width="1.5"/>`;
  for (let i = 0; i < n; i++) {
    const ang = -Math.PI / 2 + (i / n) * Math.PI * 2;
    const x = cx + Math.cos(ang) * r, y = cy + Math.sin(ang) * r;
    const isCur = i === current;
    const col = nodeColor(i);
    if (isCur) s += `<line x1="${cx}" y1="${cy}" x2="${x}" y2="${y}" stroke="${col}" stroke-width="2"/>`;
    s += `<circle cx="${x}" cy="${y}" r="${isCur ? 17 : 11}" fill="${isCur ? col : '#1d2032'}"
      stroke="${col}" stroke-width="2"${isCur ? ' filter="url(#glow)"' : ''}/>`;
    s += `<text x="${x}" y="${y + 3.5}" text-anchor="middle" font-size="${isCur ? 11 : 9}"
      font-weight="700" fill="${isCur ? '#10101c' : col}">${i}</text>`;
  }
  s += `<text x="${cx}" y="${cy - 2}" text-anchor="middle" font-size="10" fill="#8a8fae">slot</text>
    <text x="${cx}" y="${cy + 12}" text-anchor="middle" font-size="13" font-weight="700" fill="#e6e8f2">lideri</text>`;
  return `<svg viewBox="0 0 168 168" width="168" height="168" xmlns="http://www.w3.org/2000/svg">
    <defs><filter id="glow"><feGaussianBlur stdDeviation="3" result="b"/>
      <feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>${s}</svg>`;
}

function lineChartSvg(series) {
  const W = 960, H = 170, pad = { l: 50, r: 14, t: 12, b: 24 };
  let lo = Infinity, hi = -Infinity, len = 0;
  series.forEach(s => s.data.forEach(v => {
    if (v != null && !isNaN(v)) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
  }));
  series.forEach(s => len = Math.max(len, s.data.length));
  if (lo === Infinity) return '<div class="dim" style="padding:20px">veri yok</div>';
  if (hi === lo) { hi += 1; lo -= 1; }
  const ix = W - pad.l - pad.r, iy = H - pad.t - pad.b;
  const X = i => pad.l + (len <= 1 ? 0 : (i / (len - 1)) * ix);
  const Y = v => pad.t + iy - ((v - lo) / (hi - lo)) * iy;
  let grid = '';
  for (let g = 0; g <= 4; g++) {
    const yy = pad.t + (g / 4) * iy;
    const val = hi - (g / 4) * (hi - lo);
    grid += `<line x1="${pad.l}" y1="${yy}" x2="${W - pad.r}" y2="${yy}" stroke="#2a2d44" stroke-width="1"/>
      <text x="${pad.l - 6}" y="${yy + 3}" text-anchor="end" fill="#5d6385" font-size="9.5">${compact(val)}</text>`;
  }
  let lines = '';
  series.forEach(s => {
    let d = '', started = false;
    s.data.forEach((v, i) => {
      if (v == null || isNaN(v)) { started = false; return; }
      d += (started ? ' L' : 'M') + X(i).toFixed(1) + ',' + Y(v).toFixed(1);
      started = true;
    });
    if (d) lines += `<path d="${d}" fill="none" stroke="${s.color}" stroke-width="2"
      stroke-linejoin="round" stroke-linecap="round"/>`;
  });
  return `<svg viewBox="0 0 ${W} ${H}" xmlns="http://www.w3.org/2000/svg">${grid}${lines}</svg>`;
}

// ===========================================================================
// Küçük ortak bileşenler
// ===========================================================================
function Tile({ label, value, sub }) {
  return (
    <div className="tile">
      <div className="tlabel">{label}</div>
      <div className="tval">{value}</div>
      <div className="tsub">{sub}</div>
    </div>
  );
}

function Banner({ kind, ico, children, style }) {
  return (
    <div className={'banner ' + kind} style={style}>
      <span className="ico">{ico}</span>
      <div>{children}</div>
    </div>
  );
}

// ===========================================================================
// SEKME: Genel Bakış
// ===========================================================================
function Overview({ state }) {
  if (!state || !state.nodes) return <div className="loading">yükleniyor…</div>;
  const nodes = state.nodes;
  const cv = state.convergence || {};
  const cons = state.consensus || {};
  const leader = state.leader || {};

  let banner;
  if (cv.status === 'ok')
    banner = <Banner kind="ok" ico="✓"><b>Uzlaşma sağlandı.</b> Tüm düğümler aynı zincir
      ucunda — yükseklik {fmt(cv.heights && cv.heights[0])}.</Banner>;
  else if (cv.status === 'syncing')
    banner = <Banner kind="warn" ico="⟳"><b>Senkronizasyon.</b> Düğümler 1 blok aralığında —
      yükseklikler {(cv.heights || []).join(', ')}. Kısa süreli fork normaldir.</Banner>;
  else if (cv.status === 'diverged')
    banner = <Banner kind="bad" ico="⚠"><b>Zincir ayrışması.</b> Düğüm yükseklikleri:{' '}
      {(cv.heights || []).join(', ')}. Fork Ağacı sekmesine bakın.</Banner>;
  else
    banner = <Banner kind="bad" ico="○"><b>Düğüm bulunamadı.</b> devnet çalışıyor mu?{' '}
      <span className="mono">devnet/run-devnet.ps1 start</span></Banner>;

  const totGossip = nodes.reduce((s, n) => s + (n.gossipIn || 0), 0);
  const totTx = nodes.reduce((s, n) => s + (n.txReceived || 0), 0);
  const totRej = nodes.reduce((s, n) => s + (n.blocksRejected || 0), 0);
  const leaderVal = leader.currentLeader != null
    ? <span style={{ color: nodeColor(leader.currentLeader) }}>{nodeName(leader.currentLeader)}</span>
    : '—';

  return (
    <>
      {banner}
      <div className="grid tiles" style={{ marginBottom: 16 }}>
        <Tile label="Zincir ucu" value={fmt(cons.globalTip)} sub="global yükseklik" />
        <Tile label="Kesinleşen" value={fmt(cons.finalizedHeight)} sub={'k=' + cons.kFinality + ' derinlik'} />
        <Tile label="Slot lideri" value={leaderVal} sub={'slot ' + (leader.tipSlot != null ? leader.tipSlot : '—')} />
        <Tile label="Gossip mesaj" value={compact(totGossip)} sub="tüm düğümler" />
        <Tile label="Alınan işlem" value={compact(totTx)} sub="tüm düğümler" />
        <Tile label="Reddedilen blok" value={compact(totRej)} sub="fork çekişmesi" />
      </div>
      <div className="grid cards">
        {nodes.map(n => <NodeCard key={n.index} n={n} leader={leader} />)}
      </div>
    </>
  );
}

function NodeCard({ n, leader }) {
  const col = nodeColor(n.index);
  const isLeader = leader && leader.currentLeader === n.index;
  const peerMax = 3;
  const peerPct = Math.min(100, ((n.peers || 0) / peerMax) * 100);
  const rej = n.rejectReasons || {};
  const rejTxt = Object.keys(rej).length
    ? Object.entries(rej).map(([k, v]) => k + ' ' + fmt(v)).join(', ') : '—';
  return (
    <div className={'ncard ' + (n.up ? '' : 'down')} style={{ '--nc': col }}>
      {isLeader && <span className="leader-tag">★ LİDER</span>}
      <div className="nhead">
        <span className="nname"><span className={'sdot ' + (n.up ? 'up' : 'down')}></span>{n.name}</span>
        <span className="pill">rpc {n.rpc}</span>
      </div>
      {n.up ? (
        <>
          <div className="height-row"><span className="big">{fmt(n.height)}</span><span className="muted">blok</span></div>
          <div className="kv">
            <span className="k">Eşler</span><span className="v">{n.peers != null ? n.peers : '—'}/{peerMax}</span>
            <span className="k full"><span className="bar"><i style={{ width: peerPct + '%' }}></i></span></span>
            <span className="k">Mempool</span><span className="v">{n.mempoolClear != null ? fmt(n.mempoolClear) : '—'}</span>
            <span className="k">Bekleyen tx</span><span className="v">{n.pending != null ? fmt(n.pending) : '—'}</span>
            <span className="k">Doğrulanan</span><span className="v">{compact(n.blocksValidated)}</span>
            <span className="k">Reddedilen</span><span className="v">{compact(n.blocksRejected)}</span>
            <span className="k">Gossip giriş</span><span className="v">{compact(n.gossipIn)}</span>
            <span className="k">Alınan tx</span><span className="v">{compact(n.txReceived)}</span>
            <span className="k">Doğrulama</span><span className="v">{n.validateAvgMs != null ? n.validateAvgMs.toFixed(2) + ' ms' : '—'}</span>
            <span className="k">Slot</span><span className="v">{n.tipSlot != null ? n.tipSlot : '—'}</span>
            <span className="k">Tip</span><span className="v full mono dim">{shortHash(n.hash, 14)}</span>
            <span className="k">Red nedeni</span><span className="v full dim" style={{ fontSize: 11 }}>{rejTxt}</span>
          </div>
        </>
      ) : <div className="empty" style={{ padding: '24px 0' }}>RPC {n.rpc} yanıt vermiyor</div>}
    </div>
  );
}

// ===========================================================================
// SEKME: Fork Ağacı
// ===========================================================================
function forkLayout(chain, finalized) {
  const rows = chain.heights.slice().sort((a, b) => a.height - b.height);
  const colW = 84, boxW = 60, boxH = 44, laneH = 70;
  const idxOf = {};
  rows.forEach((r, i) => idxOf[r.height] = i);
  const lane = {};
  let maxUp = 0, maxDown = 0;
  rows.forEach(r => {
    const canon = r.blocks.filter(b => b.canonical);
    const forks = r.blocks.filter(b => !b.canonical && !b.rejected);
    const rej = r.blocks.filter(b => b.rejected);
    canon.forEach(b => lane[r.height + ':' + b.hash] = 0);
    forks.forEach((b, i) => { lane[r.height + ':' + b.hash] = i + 1; maxUp = Math.max(maxUp, i + 1); });
    rej.forEach((b, i) => { lane[r.height + ':' + b.hash] = -(i + 1); maxDown = Math.max(maxDown, i + 1); });
  });
  const midY = 16 + maxUp * laneH + boxH / 2;
  const svgH = midY + maxDown * laneH + boxH / 2 + 34;
  const svgW = rows.length * colW + 30;
  const colX = h => 16 + idxOf[h] * colW;
  const pos = {};
  rows.forEach(r => r.blocks.forEach(b => {
    const ln = lane[r.height + ':' + b.hash] || 0;
    pos[r.height + ':' + b.hash] = { x: colX(r.height), y: midY - ln * laneH - boxH / 2, b };
  }));
  const canonAt = {};
  rows.forEach(r => r.blocks.forEach(b => {
    if (b.canonical) canonAt[r.height] = pos[r.height + ':' + b.hash];
  }));
  const edges = [];
  rows.forEach(r => r.blocks.forEach(b => {
    const me = pos[r.height + ':' + b.hash];
    let parent = pos[(r.height - 1) + ':' + b.prevHash];
    if (!parent && b.rejected) parent = canonAt[r.height - 1];
    if (!parent) return;
    const x1 = parent.x + boxW, y1 = parent.y + boxH / 2, x2 = me.x, y2 = me.y + boxH / 2;
    const mx = (x1 + x2) / 2;
    const canon = b.canonical && parent.b.canonical;
    edges.push({
      d: `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`,
      stroke: b.rejected ? '#f87171' : (canon ? '#5b6088' : '#8a8fae'),
      width: canon ? 2.4 : 1.6, dash: b.rejected ? '4 3' : (canon ? null : '5 4'),
      op: b.rejected ? 0.7 : 0.9,
    });
  }));
  const boxes = rows.flatMap(r => r.blocks.map(b => {
    const me = pos[r.height + ':' + b.hash];
    return { key: r.height + ':' + b.hash, b, x: me.x, y: me.y };
  }));
  const axis = [];
  rows.forEach((r, i) => {
    if (i % 4 === 0 || i === rows.length - 1)
      axis.push({ x: colX(r.height) + boxW / 2, y: svgH - 8, t: r.height });
  });
  let finality = null;
  if (finalized != null && idxOf[finalized] != null)
    finality = { x: colX(finalized) + boxW + (colW - boxW) / 2, y2: svgH - 26 };
  return { svgW, svgH, boxW, boxH, edges, boxes, axis, finality };
}

function ForkChainSvg({ chain, nodeCount, finalized, onHover, onLeave, onBlock }) {
  const L = useMemo(() => forkLayout(chain, finalized), [chain, finalized]);
  return (
    <svg viewBox={`0 0 ${L.svgW} ${L.svgH}`} width={L.svgW} height={L.svgH}
      xmlns="http://www.w3.org/2000/svg" fontFamily="Segoe UI, sans-serif">
      {L.edges.map((e, i) => (
        <path key={'e' + i} d={e.d} fill="none" stroke={e.stroke} strokeWidth={e.width}
          strokeDasharray={e.dash || undefined} opacity={e.op} />
      ))}
      {L.finality && (
        <g>
          <line x1={L.finality.x} y1="6" x2={L.finality.x} y2={L.finality.y2}
            stroke="#34d399" strokeWidth="1.4" strokeDasharray="3 4" opacity="0.55" />
          <text x={L.finality.x + 4} y="16" fill="#34d399" fontSize="9.5">kesinleşme</text>
        </g>
      )}
      {L.axis.map((a, i) => (
        <text key={'a' + i} x={a.x} y={a.y} textAnchor="middle" fill="#5d6385" fontSize="9">{a.t}</text>
      ))}
      {L.boxes.map(bx => {
        const b = bx.b;
        const lc = b.leader != null ? nodeColor(b.leader) : '#3a3d55';
        const fill = b.rejected ? '#3a1f24' : lc;
        const txt = b.rejected ? '#f0a9a9' : '#10101c';
        const strokeC = b.rejected ? '#f87171' : (b.canonical ? '#e6e8f2' : '#9aa0c4');
        const dash = b.canonical && !b.rejected ? undefined : '4 3';
        const line3 = b.rejected ? 'reddedildi'
          : (b.txCount > 0 ? b.txCount + ' tx · ' : '') + (b.nodes ? b.nodes.length + '/' + nodeCount : '');
        return (
          <g key={bx.key} className="blk" style={{ cursor: 'pointer' }}
            onMouseMove={e => onHover(b, e)} onMouseLeave={onLeave} onClick={() => onBlock(b)}>
            <rect className="blk-box" x={bx.x} y={bx.y} width={L.boxW} height={L.boxH} rx="8"
              fill={fill} stroke={strokeC} strokeWidth={b.canonical ? 2 : 1.5} strokeDasharray={dash} />
            <text x={bx.x + L.boxW / 2} y={bx.y + 17} textAnchor="middle" fill={txt}
              fontSize="12.5" fontWeight="700">#{b.height}</text>
            <text x={bx.x + L.boxW / 2} y={bx.y + 30} textAnchor="middle" fill={txt}
              fontSize="9" opacity="0.85">{b.slot != null ? 'slot ' + b.slot : 'orphan'}</text>
            <text x={bx.x + L.boxW / 2} y={bx.y + 40} textAnchor="middle" fill={txt}
              fontSize="8.5" opacity="0.7">{line3}</text>
          </g>
        );
      })}
    </svg>
  );
}

function ForkTab({ forks, state, forkWindow, setForkWindow, onHover, onLeave, onBlock }) {
  const kFin = state && state.consensus && state.consensus.kFinality != null ? state.consensus.kFinality : 5;
  const controls = (
    <div className="fork-controls">
      <label className="muted">Pencere:{' '}
        <select value={forkWindow} onChange={e => setForkWindow(+e.target.value)}>
          {[24, 48, 96, 150].map(v => <option key={v} value={v}>{v} blok</option>)}
        </select>
      </label>
      <div className="legend">
        <span><span className="sw" style={{ background: '#a78bfa' }}></span>node0</span>
        <span><span className="sw" style={{ background: '#38bdf8' }}></span>node1</span>
        <span><span className="sw" style={{ background: '#fbbf24' }}></span>node2</span>
        <span><span className="sw" style={{ background: '#34d399' }}></span>node3</span>
        <span><span className="sw" style={{ background: 'transparent', border: '2px solid #e6e8f2' }}></span>kanonik zincir</span>
        <span><span className="sw" style={{ background: 'transparent', border: '2px dashed #8a8fae' }}></span>fork dalı</span>
        <span><span className="sw" style={{ background: 'transparent', border: '2px dashed #f87171' }}></span>terk edilmiş (orphan)</span>
      </div>
    </div>
  );

  if (!forks || !forks.ok || !forks.chains || !forks.chains.length)
    return <>{controls}<div className="empty">Blok verisi yok — devnet çalışmıyor olabilir.</div></>;

  const chains = forks.chains;
  const topTip = chains[0].tipHeight;
  let banner;
  if (forks.partitioned)
    banner = <Banner kind="bad" ico="⑂" style={{ marginBottom: 14 }}>
      <b>AĞ BÖLÜNMESİ — {chains.length} ayrı zincir tespit edildi.</b> Düğümler ortak genesis
      dışında birbirinden kopmuş zincirler izliyor. Her zincir aşağıda ayrı bir şerit olarak
      gösteriliyor. Bu zincirler kendi kendine birleşemez — düzeltmek için devnet'i temiz
      şekilde yeniden başlatmak gerekir.</Banner>;
  else {
    const fn = chains[0].forkHeights.length;
    banner = fn
      ? <Banner kind="warn" ico="⑂" style={{ marginBottom: 14 }}>Tek zincir, ancak <b>{fn}</b>{' '}
          yükseklikte dallanma izi var. Kırmızı kesik kutular terk edilmiş dalları gösterir.</Banner>
      : <Banner kind="ok" ico="✓" style={{ marginBottom: 14 }}>Tüm düğümler tek bir zincirde —
          görüntülenen pencerede dallanma yok.</Banner>;
  }

  return (
    <>
      {controls}
      {banner}
      <div className="grid tiles" style={{ marginBottom: 14 }}>
        <Tile label="Aktif zincir" value={String(chains.length)} sub={chains.length > 1 ? 'ağ bölünmüş' : 'tek zincir'} />
        <Tile label="En uzun uç" value={fmt(topTip)} sub="global yükseklik" />
        <Tile label="Reddedilen blok" value={compact(forks.rejectedSeen || 0)} sub="kalıcı fork geçmişi" />
        <Tile label="Çevrimdışı düğüm" value={String((forks.offline || []).length)}
          sub={(forks.offline || []).map(nodeName).join(', ') || 'yok'} />
      </div>
      {chains.map(ch => {
        const behind = topTip - ch.tipHeight;
        const fin = Math.max(0, ch.tipHeight - kFin);
        return (
          <div className="panel chainpanel" key={ch.id} style={{ marginBottom: 14 }}>
            <div className="chainhead">
              <span className="chainbadge">Zincir {ch.label}</span>
              <span className="chipwrap">
                {ch.nodes.map(i => (
                  <span key={i} className="nchip" style={{ background: nodeColor(i) }}>{nodeName(i)}</span>
                ))}
              </span>
              <span className="muted">uç <b>{fmt(ch.tipHeight)}</b></span>
              <span className="muted">pencere {ch.from}–{ch.to}</span>
              {behind > 0
                ? <span className="pill bad">{fmt(behind)} blok geride</span>
                : <span className="pill ok">en uzun zincir</span>}
              <span className="dim">{ch.forkHeights.length} iç dallanma</span>
            </div>
            <div className="fork-scroll" onMouseLeave={onLeave}>
              <ForkChainSvg chain={ch} nodeCount={forks.nodeCount} finalized={fin}
                onHover={onHover} onLeave={onLeave} onBlock={onBlock} />
            </div>
          </div>
        );
      })}
      <p className="dim" style={{ margin: '4px 0', fontSize: 12 }}>
        Bir bloğa tıklayarak ayrıntıları görebilirsiniz. Renk = bloğu üreten lider düğüm.
        Kırmızı kesik kutular = geçmişte dallanıp terk edilmiş bloklar (iptal olsalar bile kalır).
      </p>
      <ForkHistory forkHistory={forks.forkHistory} />
    </>
  );
}

function ForkHistory({ forkHistory }) {
  const hist = forkHistory || [];
  return (
    <div className="panel" style={{ marginTop: 14 }}>
      <h2>Fork Geçmişi — neden dallandı? ({hist.length} kayıt)</h2>
      <p className="dim" style={{ fontSize: 12, marginBottom: 12 }}>
        Oturum boyunca düğüm loglarından biriken tüm fork olayları. Bu dallar kanonik zincire
        girmese (iptal/orphan olsa) bile listede kalır. devnet'te round-robin lider planı +
        500&nbsp;ms slotlar nedeniyle iki lider sık sık neredeyse aynı anda blok üretir.
      </p>
      <div className="fhlist">
        {hist.length ? hist.map((e, i) => {
          const nodes = (e.nodes || []).map(n => typeof n === 'number' ? nodeName(n) : n);
          return (
            <div className="fhrow" key={e.key || i}>
              <div className="fh-h">#{e.height != null ? e.height : '?'}</div>
              <div className="fh-main">
                <div className="fh-top">
                  <span className="pill bad">⑂ {reasonLabel(e.reason)}</span>
                  <span className="muted">{nodes.length} düğüm reddetti{nodes.length ? ': ' + nodes.join(', ') : ''}</span>
                  <span className="dim" style={{ marginLeft: 'auto' }}>
                    {clockOf(e.firstTs)}{e.lastTs && e.lastTs !== e.firstTs ? ' – ' + clockOf(e.lastTs) : ''}
                  </span>
                </div>
                <div className="fh-why">{e.reasonText || ''}</div>
                <div className="fh-hash mono dim">terk edilen üst blok: {shortHash(e.got, 18)}</div>
              </div>
            </div>
          );
        }) : <div className="empty">Henüz reddedilmiş blok kaydı yok — zincir temiz ilerliyor.</div>}
      </div>
    </div>
  );
}

// ===========================================================================
// SEKME: Konsensüs & Lider
// ===========================================================================
function Consensus({ state }) {
  if (!state || !state.consensus) return <div className="loading">yükleniyor…</div>;
  const cons = state.consensus, leader = state.leader || {};
  const nodeN = (state.config && state.config.nodes ? state.config.nodes.length : 4);
  const cur = leader.currentLeader;
  const nonce = cons.nonce || {};
  const epoch = nonce.epoch != null ? nonce.epoch : (cons.stake ? cons.stake.epoch : '—');
  const stake = cons.stake;
  const tot = stake && stake.pools ? (stake.total_stake || stake.pools.reduce((s, p) => s + (p.stake || 0), 0)) : 0;

  return (
    <>
      <div className="panel" style={{ marginBottom: 14 }}>
        <h2>Slot Lideri</h2>
        <div className="leader-hero">
          <div className="wheel" dangerouslySetInnerHTML={{ __html: leaderWheelSvg(nodeN, cur) }} />
          <div className="leader-now">
            <div className="ltitle">Bu slotun lideri</div>
            <div className="lname" style={{ color: cur != null ? nodeColor(cur) : '#888' }}>{nodeName(cur)}</div>
            <div className="lwhy">
              Slot <b>{leader.tipSlot != null ? leader.tipSlot : '—'}</b> · epoch içi slot{' '}
              <b>{leader.epochSlot != null ? leader.epochSlot : '—'}</b>/{cons.epochSlots}<br />
              <b>Neden bu düğüm?</b> devnet, deterministik <b>round-robin</b> lider planı kullanır:
              lider = slot mod {nodeN}.{' '}
              {leader.tipSlot != null && `${leader.tipSlot} mod ${nodeN} = ${leader.tipSlot % nodeN} → ${nodeName(cur)}.`}<br />
              Ana ağda bu seçim <b>VRF</b> ile stake-orantılı yapılır.
            </div>
          </div>
        </div>
      </div>

      <div className="panel" style={{ marginBottom: 14 }}>
        <h2>Lider Planı (geçmiş → gelecek slotlar)</h2>
        <div className="sched">
          {(leader.schedule || []).length
            ? (leader.schedule || []).map((s, i) => (
                <div className={'slot ' + (s.current ? 'current' : '')} key={i}>
                  <div className="ss">slot</div>
                  <div className="sn" style={{ color: nodeColor(s.leader) }}>{s.slot}</div>
                  <div className="sd" style={{ background: nodeColor(s.leader) }}></div>
                  <div className="ss" style={{ marginTop: 3 }}>{nodeName(s.leader)}</div>
                </div>
              ))
            : <span className="dim">veri yok</span>}
        </div>
      </div>

      <div className="panel" style={{ marginBottom: 14 }}>
        <h2>Lider Seçimi Nasıl Çalışır?</h2>

        <div className="lsel">
          <span className="lsel-tag now">ŞU AN · devnet</span>
          <h3>Deterministik Round-Robin</h3>
          <p>devnet, öngörülebilirlik için en basit yöntemi kullanır: bir slotun lideri, slot
            numarasının havuz sayısına bölümünden kalandır. Stake'e bakılmaz, rastgelelik
            yoktur, VRF çalıştırılmaz.</p>
          <div className="formula">lider(slot) = slot mod N</div>
          <div className="lsel-vars">
            <span><b>slot</b> — sıralı zaman dilimi; her biri {cons.slotMs} ms (devnet)</span>
            <span><b>N</b> — havuz / düğüm sayısı = {nodeN}</span>
          </div>
          {leader.tipSlot != null && (
            <div className="lsel-ex">Örnek — şu anki slot {leader.tipSlot}:{' '}
              {leader.tipSlot} mod {nodeN} = <b>{leader.tipSlot % nodeN}</b> →{' '}
              <b style={{ color: nodeColor(leader.tipSlot % nodeN) }}>
                {nodeName(leader.tipSlot % nodeN)}</b> bu slotun lideridir.</div>
          )}
          <p className="dim">Her slotta tam bir lider çıkar — boş slot ya da çok-lider çekişmesi
            olmaz; bu yüzden geliştirme ve hata ayıklama için idealdir
            (<span className="mono">round_robin_leader = true</span>). Ancak gerçek güvenlik
            sağlamaz: kimin ne zaman lider olacağı herkesçe önceden bilinir, dolayısıyla
            sansür ya da saldırı hedefi seçmek kolaydır.</p>
        </div>

        <div className="lsel">
          <span className="lsel-tag future">GELECEK · mainnet — Ouroboros Praos</span>
          <h3>VRF ile Stake-Orantılı Gizli Piyango</h3>
          <p>Ana ağda lider seçimi <b>özel ama doğrulanabilir</b> bir piyangoya dönüşür. Her
            slot için her havuz, kendi gizli VRF anahtarıyla sözde-rastgele bir değer üretir;
            bu değer havuzun stake'iyle orantılı bir eşiğin altına düşerse o havuz, o slotun
            lideridir.</p>
          <div className="formula">y = VRF( sk<sub>vrf</sub> , η ‖ slot )</div>
          <div className="formula">havuz lider  ⇔  ŷ &lt; φ(σ)</div>
          <div className="formula">φ(σ) = 1 − (1 − f)<sup>σ</sup></div>
          <div className="lsel-vars">
            <span><b>VRF</b> — Verifiable Random Function: çıktısı gizli anahtara bağlıdır ama herkes açık anahtarla doğrulayabilir</span>
            <span><b>ŷ</b> — VRF çıktısının [0, 1) aralığına ölçeklenmiş hâli</span>
            <span><b>η</b> (nonce) — epoch rastgelelik tohumu; bir önceki epoch'un VRF çıktılarından türetilir</span>
            <span><b>σ</b> — havuzun göreli stake'i = havuz stake ÷ toplam stake</span>
            <span><b>f</b> — aktif slot katsayısı (devnet: 0.05) — bir slotun "dolu" olma hedef oranı</span>
            <span><b>φ(σ)</b> — bir havuzun tek bir slotta lider olma olasılığı</span>
          </div>
          <div className="lsel-ex">Örnek — {nodeN} eşit havuz (göreli stake σ = {(1 / nodeN).toFixed(2)}),
            f = 0.05 için: φ = 1 − 0.95<sup>{(1 / nodeN).toFixed(2)}</sup> ≈{' '}
            <b>{(1 - Math.pow(0.95, 1 / nodeN)).toFixed(4)}</b> → her havuz her slotta{' '}
            ~%{((1 - Math.pow(0.95, 1 / nodeN)) * 100).toFixed(2)} ihtimalle lider olur;
            slotların ~%5'i dolu olur.</div>
          <div className="lsel-props">
            <div className="lsel-prop"><b>Stake-orantılı.</b> Stake arttıkça lider olunan slot
              sayısı orantılı artar; blok ödülü stake ile orantılıdır. Kimlik çoğaltmak (Sybil)
              işe yaramaz — stake bölününce toplam şans değişmez.</div>
            <div className="lsel-prop"><b>Gizli ama doğrulanabilir.</b> Havuz, bloğu yayınlayana
              kadar lider olduğunu belli etmez. Blok başlığındaki{' '}
              <span className="mono">vrf_proof</span> ile herkes, üreticinin o slotun gerçek
              lideri olduğunu gizli anahtarı görmeden doğrular.</div>
            <div className="lsel-prop"><b>Boş ve çok-lider slotlar.</b> Seçim havuz-bağımsız
              olduğundan bir slotta 0, 1 veya birkaç lider olabilir. Boş slot atlanır;
              çok-lider kısa bir fork yaratır, yoğunluk ağırlıklı en uzun zincir kazananı seçer.</div>
            <div className="lsel-prop"><b>Grinding'e karşı epoch nonce.</b> η bir önceki epoch'un
              rastgeleliğinden üretildiğinden, saldırgan gelecekteki slot sonuçlarını önceden
              hesaplayıp kendine avantajlı çıktı "öğütemez".</div>
            <div className="lsel-prop"><b>KES imzaları.</b> Blok ayrıca zamanla evrilen
              (ileri-güvenli) bir KES anahtarıyla imzalanır; eski anahtarlar geri
              getirilemediğinden çalınan anahtar geçmiş blokları sahteleştiremez — uzun menzilli
              saldırılara karşı koruma.</div>
          </div>
        </div>

        <p className="dim" style={{ fontSize: 11.5, marginTop: 10 }}>
          Hedef tasarım: Ouroboros Praos (saf Nakamoto PoS). devnet şu an round-robin kullanıyor
          çünkü VRF seçimi boş ve çok-lider slotlar üretir — bu da geliştirme sırasında zinciri
          öngörülemez kılar.
        </p>
      </div>

      <div className="panel">
        <h2>Epoch & Kesinlik</h2>
        <div className="grid tiles">
          <Tile label="Epoch" value={String(epoch)} sub={'devnet ' + cons.epochSlots + ' slot'} />
          <Tile label="Epoch içi slot"
            value={(leader.epochSlot != null ? leader.epochSlot : '—') + '/' + cons.epochSlots}
            sub={'sonraki epoch ' + (leader.slotsToEpoch != null
              ? (leader.slotsToEpoch * cons.slotMs / 1000).toFixed(0) + ' sn' : '—')} />
          <Tile label="Zincir ucu" value={fmt(cons.globalTip)} sub="yükseklik" />
          <Tile label="Kesinleşen" value={fmt(cons.finalizedHeight)}
            sub={'k=' + cons.kFinality + ' (' + (cons.kFinality * cons.slotMs / 1000) + ' sn)'} />
        </div>
        <div className="detail-kv" style={{ marginTop: 14 }}>
          <span className="k">Epoch nonce</span><span className="v mono">{nonce.nonce_hex || '—'}</span>
          <span className="k">Slot süresi</span><span className="v">{cons.slotMs} ms</span>
          <span className="k">Fork seçimi</span>
          <span className="v">yoğunluk ağırlıklı en uzun zincir (k={cons.kFinality})</span>
        </div>
      </div>

      {stake && stake.pools && stake.pools.length && (
        <div className="panel" style={{ marginTop: 14 }}>
          <h2>Stake Dağılımı — Epoch {stake.epoch}</h2>
          {stake.pools.map((p, i) => {
            const pct = tot ? (p.stake / tot * 100) : 0;
            return (
              <div key={i}>
                <div className="stakebar-row">
                  <span className="sl" style={{ color: nodeColor(i) }}>Havuz {i + 1}</span>
                  <span className="st"><i style={{ width: pct.toFixed(1) + '%', background: nodeColor(i) }}></i></span>
                  <span className="sv">{compact(p.stake)} · {pct.toFixed(1)}%</span>
                </div>
                <div className="dim mono" style={{ fontSize: 10.5, margin: '-4px 0 8px 110px' }}>
                  {shortHash(p.pool_id, 16)}
                </div>
              </div>
            );
          })}
          <div className="dim" style={{ fontSize: 11.5, marginTop: 6 }}>
            Toplam stake {compact(tot)}. devnet'te tüm havuzlar eşit stake'e sahiptir, bu yüzden
            lider sırası round-robin'e indirgenir.
          </div>
        </div>
      )}
    </>
  );
}

// ===========================================================================
// SEKME: Performans
// ===========================================================================
function gossipRate(hist, i) {
  if (hist.length < 2) return null;
  const a = hist[0].nodes[i], b = hist[hist.length - 1].nodes[i];
  if (!a || !b || a.gin == null || b.gin == null) return null;
  const dt = (hist[hist.length - 1].t - hist[0].t) / 60000;
  return dt > 0 ? (b.gin - a.gin) / dt : null;
}

function Perf({ state }) {
  if (!state || !state.history) return <div className="loading">yükleniyor…</div>;
  const hist = state.history;
  const nodes = state.nodes || [];
  if (hist.length < 2)
    return <div className="empty">Grafik verisi toplanıyor — birkaç saniye içinde görünecek.</div>;
  const series = (field) => nodes.map((n, i) => ({
    name: n.name, color: nodeColor(i),
    data: hist.map(s => (s.nodes[i] ? s.nodes[i][field] : null)),
  }));
  const charts = [
    ['Zincir Yüksekliği', 'h', 'blok üretim hızını eğim gösterir'],
    ['Bağlı Eşler', 'peers', 'p2p ağ sağlığı'],
    ['Mempool Boyutu', 'mem', 'bekleyen temiz işlemler'],
    ['Gossip Giriş (kümülatif)', 'gin', 'alınan gossip mesajları'],
    ['Reddedilen Blok (kümülatif)', 'br', 'fork çekişmesi yoğunluğu'],
  ];
  return (
    <>
      {charts.map(([title, field, sub]) => (
        <div className="panel" style={{ marginBottom: 14 }} key={field}>
          <h2>{title}</h2>
          <div className="chartwrap" dangerouslySetInnerHTML={{ __html: lineChartSvg(series(field)) }} />
          <div className="chart-legend">
            {nodes.map((n, i) => (
              <span key={i}><i style={{ background: nodeColor(i) }}></i>{n.name}</span>
            ))}
          </div>
          <div className="dim" style={{ fontSize: 11.5, marginTop: 4 }}>{sub}</div>
        </div>
      ))}
      <div className="panel">
        <h2>Düğüm Performans Özeti</h2>
        <table>
          <thead><tr>
            <th>Düğüm</th><th className="num">Yükseklik</th><th className="num">Doğrulama (ort)</th>
            <th className="num">Doğrulanan</th><th className="num">Reddedilen</th>
            <th className="num">Gossip/dk</th><th className="num">Eşler</th>
          </tr></thead>
          <tbody>
            {nodes.map((n, i) => {
              const rate = gossipRate(hist, i);
              return (
                <tr key={i}>
                  <td style={{ color: nodeColor(i), fontWeight: 700 }}>{n.name}</td>
                  <td className="num">{fmt(n.height)}</td>
                  <td className="num">{n.validateAvgMs != null ? n.validateAvgMs.toFixed(2) + ' ms' : '—'}</td>
                  <td className="num">{compact(n.blocksValidated)}</td>
                  <td className="num">{compact(n.blocksRejected)}</td>
                  <td className="num">{rate != null ? fmt(rate) : '—'}</td>
                  <td className="num">{n.peers != null ? n.peers : '—'}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}

// ===========================================================================
// SEKME: İşlemler  (tüm detaylar her zaman açık — aç/kapat yok)
// ===========================================================================
function TxItem({ t, i, b }) {
  const lc = b.leader != null ? nodeColor(b.leader) : '#888';
  return (
    <div className="tx-item">
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'baseline' }}>
        <b>İşlem {i + 1}</b>
        <span className="pill" style={{ color: lc, borderColor: lc }}>
          zincire ekleyen: {nodeName(b.leader)}{b.slot != null ? ' (slot ' + b.slot + ' lideri)' : ''}
        </span>
        <span className="muted">sürüm {t.version != null ? t.version : '—'}</span>
        <span className="muted">ücret {fmt(t.fee)}</span>
        <span className="muted">kilit slotu {t.lockTime != null ? t.lockTime : '—'}</span>
        <span className="muted">toplam çıktı <b>{fmt(t.totalOut)}</b></span>
      </div>
      <div className="tx-io">
        <div className="iolist">
          <div className="iohead">Girdiler — harcanan UTXO ({t.inputs.length})</div>
          {t.inputs.length ? t.inputs.map((inp, j) => (
            <div className="io-row" key={j}>
              <span className="mono">{shortHash(inp.txid, 12)}<span className="dim">#{inp.index}</span></span>
              <span className="dim">tanık {inp.witnessLen} B</span>
            </div>
          )) : <div className="dim" style={{ padding: '4px 0' }}>girdi yok (coinbase / genesis)</div>}
        </div>
        <div className="iolist">
          <div className="iohead">Çıktılar — yeni UTXO ({t.outputs.length})</div>
          {t.outputs.length ? t.outputs.map((o, j) => (
            <div className="io-row" key={j}>
              <span>#{j} · <b>{fmt(o.value)}</b> birim</span>
              <span className="dim">script {o.scriptLen}B{o.hasDatum ? ' · datum' : ''}{o.hasStealth ? ' · stealth' : ''}</span>
            </div>
          )) : <div className="dim" style={{ padding: '4px 0' }}>çıktı yok</div>}
        </div>
      </div>
    </div>
  );
}

function TxBlock({ b, nodeN }) {
  const lc = b.leader != null ? nodeColor(b.leader) : '#888';
  let why;
  if (b.isGenesis)
    why = <>Bu, <b>genesis bloğu</b> — ağ ilk başladığında oluşturulur, bir slot lideri
      tarafından üretilmez. İçindeki işlem başlangıç bakiyelerini dağıtır.</>;
  else if (b.slot != null && b.leader != null)
    why = <><b style={{ color: lc }}>{nodeName(b.leader)}</b> bu bloğu üretti çünkü{' '}
      <b>slot {b.slot}</b>'in lideriydi. devnet deterministik <b>round-robin</b> lider planı
      kullanır: lider = slot mod {nodeN} = {b.slot} mod {nodeN} = <b>{b.slot % nodeN}</b> →{' '}
      {nodeName(b.leader)}. Bloktaki {b.txCount} işlem bu düğüm tarafından zincire işlendi.</>;
  else why = 'Lider bilgisi yok.';

  return (
    <div className="txblock open">
      <div className="txhead" style={{ cursor: 'default' }}>
        <span className="bh">Blok #{b.height}</span>
        {b.isGenesis && <span className="pill ok">genesis</span>}
        <span className="pill" style={{ color: lc, borderColor: lc }}>üretici {nodeName(b.leader)}</span>
        <span className="muted">slot {b.slot != null ? b.slot : '—'}</span>
        <span className="muted">
          {b.timestamp ? new Date(b.timestamp * 1000).toLocaleString('tr-TR') : 'zaman damgası 0'}
        </span>
        <span style={{ marginLeft: 'auto' }} className="pill">{b.txCount} işlem</span>
      </div>
      <div className="txbody">
        <div className="txwhy"><span className="txwhy-ico" style={{ background: lc }}></span><div>{why}</div></div>
        <div className="dim mono" style={{ fontSize: 11, marginTop: 8 }}>
          üretici anahtar (lider): {shortHash(b.producer, 20)}
        </div>
        {b.txs.map((t, i) => <TxItem key={i} t={t} i={i} b={b} />)}
      </div>
    </div>
  );
}

function PendingTxRow({ p }) {
  const has = p.inputs != null;
  return (
    <div className="tx-item" style={{ borderColor: 'rgba(251,191,36,.35)' }}>
      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'baseline' }}>
        <b>Bekleyen işlem</b>
        <span className="pill warn">mempool · bloğa girmedi</span>
        <span className="muted mono">tx {shortHash(p.txId, 16)}</span>
        {has && <span className="muted">ücret {fmt(p.fee)}</span>}
        {has && <span className="muted">toplam çıktı <b>{fmt(p.totalOut)}</b></span>}
        <span className="muted">mempool'da: {(p.nodes || []).map(nodeName).join(', ') || '—'}</span>
      </div>
      {has ? (
        <div className="tx-io">
          <div className="iolist">
            <div className="iohead">Girdiler — harcanan UTXO ({p.inputs.length})</div>
            {p.inputs.length ? p.inputs.map((inp, j) => (
              <div className="io-row" key={j}>
                <span className="mono">{shortHash(inp.txid, 12)}<span className="dim">#{inp.index}</span></span>
                <span className="dim">tanık {inp.witnessLen} B</span>
              </div>
            )) : <div className="dim" style={{ padding: '4px 0' }}>girdi yok</div>}
          </div>
          <div className="iolist">
            <div className="iohead">Çıktılar — yeni UTXO ({p.outputs.length})</div>
            {p.outputs.map((o, j) => (
              <div className="io-row" key={j}>
                <span>#{j} · <b>{fmt(o.value)}</b> birim</span>
                <span className="dim">script {o.scriptLen}B{o.hasDatum ? ' · datum' : ''}{o.hasStealth ? ' · stealth' : ''}</span>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <div className="dim" style={{ marginTop: 6, fontSize: 12 }}>
          İşlem ayrıntısı düğümden alınamadı — yine de mempool'da bekliyor.
        </div>
      )}
    </div>
  );
}

function TxHistoryRow({ e }) {
  const d = e.detail;
  const badge = e.status === 'pending'
    ? <span className="pill warn">mempool'da bekliyor</span>
    : e.status === 'confirmed'
      ? <span className="pill ok">zincire girdi</span>
      : <span className="pill bad">düştü</span>;
  return (
    <div className="throw">
      <div className="th-st">{badge}</div>
      <div className="th-main">
        <div className="mono" style={{ fontSize: 12 }}>tx {shortHash(e.txId, 20)}</div>
        <div className="dim" style={{ fontSize: 11.5, marginTop: 3 }}>
          {d
            ? 'ücret ' + fmt(d.fee) + ' · ' + d.inputs.length + ' girdi → ' +
              d.outputs.length + ' çıktı · toplam çıktı ' + fmt(d.totalOut)
            : 'işlem ayrıntısı alınamadı'}
        </div>
        {e.status === 'dropped' && (
          <div style={{ fontSize: 11.5, marginTop: 3, color: '#e7a3a3' }}>
            Bir bloğa girmeden mempool'dan atıldı — büyük olasılıkla süresi doldu
            (tx_ttl_slots) ya da blok kurulurken reddedildi.
          </div>
        )}
      </div>
      <div className="th-when dim">
        ilk görülme {fmtClock(e.firstSeen)}
        {e.resolvedAt ? <><br />sonuç {fmtClock(e.resolvedAt)}</> : null}
      </div>
    </div>
  );
}

function TxTab({ tx, state }) {
  if (!tx || !tx.ok)
    return <div className="empty">İşlem verisi yok — devnet çalışmıyor olabilir.</div>;
  const nodeN = (state && state.config && state.config.nodes) ? state.config.nodes.length : 4;
  const pending = tx.pending || [];
  const blocks = tx.blocks || [];
  const history = tx.txHistory || [];
  const dropped = history.filter(e => e.status === 'dropped').length;
  return (
    <>
      <div className="grid tiles" style={{ marginBottom: 12 }}>
        <Tile label="Bekleyen (mempool)" value={String(pending.length)} sub="henüz bloğa girmedi" />
        <Tile label="İşlem geçmişi" value={String(history.length)}
          sub={dropped ? dropped + ' tanesi düştü' : 'oturum boyunca görülen'} />
        <Tile label="Zincirdeki işlemli blok" value={String(blocks.length)} sub={tx.scannedFrom + '–' + tx.tip + ' arası'} />
        <Tile label="Zincirdeki işlem" value={fmt(blocks.reduce((s, b) => s + b.txCount, 0))} sub="genesis dahil" />
      </div>
      <p className="dim" style={{ fontSize: 12, marginBottom: 12 }}>
        Yeni gönderilen bir işlem önce <b>mempool</b>'a düşer. Bir slot lideri onu bloğa dahil
        edince <b>zincire girmiş</b> olur. Mempool ve blok pencereleri zamanla kaydığından
        aşağıdaki <b>İşlem Geçmişi</b>, görülen her işlemi akıbetiyle birlikte kalıcı tutar —
        böylece bir işlem listeden kaybolmaz.
      </p>

      {pending.length > 0 ? (
        <div className="panel" style={{ marginBottom: 14, borderColor: 'rgba(251,191,36,.45)' }}>
          <h2>Bekleyen İşlemler — mempool ({pending.length})</h2>
          <p className="dim" style={{ fontSize: 12, marginBottom: 10 }}>
            Bu işlemler düğümlerin mempool'unda; henüz bir bloğa girmediler.
          </p>
          {pending.map(p => <PendingTxRow key={p.txId} p={p} />)}
        </div>
      ) : (
        <Banner kind="ok" ico="✓" style={{ marginBottom: 14 }}>
          Mempool boş — bekleyen (henüz bloğa girmemiş) işlem yok.
        </Banner>
      )}

      {history.length > 0 && (
        <div className="panel" style={{ marginBottom: 14 }}>
          <h2>İşlem Geçmişi — görülen tüm işlemler ({history.length})</h2>
          <p className="dim" style={{ fontSize: 12, marginBottom: 10 }}>
            Monitör çalışırken mempool'da görülen her işlem burada <b>kalıcı</b> tutulur — bir
            bloğa girse de, süresi dolup düşse de listeden kaybolmaz.{' '}
            <b style={{ color: 'var(--ok)' }}>zincire girdi</b> = bloğa alındı, kalıcılaştı ·{' '}
            <b style={{ color: 'var(--warn)' }}>mempool'da bekliyor</b> = henüz bloğa girmedi ·{' '}
            <b style={{ color: 'var(--bad)' }}>düştü</b> = bloğa girmeden mempool'dan atıldı.
          </p>
          <div className="thlist">
            {history.map(e => <TxHistoryRow key={e.txId} e={e} />)}
          </div>
        </div>
      )}

      <h2 className="txsection">Zincire girmiş işlemler (blok blok)</h2>
      {blocks.length
        ? blocks.map(b => <TxBlock key={b.height} b={b} nodeN={nodeN} />)
        : <Banner kind="warn" ico="○">Zincire girmiş (bir bloğa dahil edilmiş) işlem yok. Bir
            işlem göndermek için:{' '}
            <span className="mono">cargo run -p qv-node --example send_tx</span></Banner>}
    </>
  );
}

// ===========================================================================
// SEKME: Loglar
// ===========================================================================
function LogsTab({ logs, state, filterNode, setFilterNode, filterKind, setFilterKind }) {
  const nodes = (state && state.config && state.config.nodes) || [];
  let events = (logs && logs.events) || [];
  if (filterKind) events = events.filter(e => e.kind === filterKind);
  events = events.slice().reverse();

  return (
    <>
      <div className="logfilter">
        <label className="muted">Düğüm:{' '}
          <select value={filterNode} onChange={e => setFilterNode(e.target.value)}>
            <option value="">tümü</option>
            {nodes.map(n => <option key={n.name} value={n.name}>{n.name}</option>)}
          </select>
        </label>
        <label className="muted">Tür:{' '}
          <select value={filterKind} onChange={e => setFilterKind(e.target.value)}>
            <option value="">tümü</option>
            {Object.entries(KIND_TR).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
          </select>
        </label>
        <span className="dim" style={{ marginLeft: 'auto' }}>{events.length} olay</span>
      </div>
      {!logs || !logs.ok
        ? <div className="empty">Log okunamadı.</div>
        : !logs.workDirExists
          ? <Banner kind="warn" ico="⚠">Log klasörü bulunamadı:{' '}
              <span className="mono">{logs.workDir}</span>. Sunucuyu{' '}
              <span className="mono">--work</span> ile doğru klasöre yönlendirin.</Banner>
          : (
            <div className="logbox">
              {events.length ? events.map((e, i) => {
                const f = e.fields || {};
                let extra = '';
                if (e.kind === 'block-accepted' && f.height != null) extra = ' · yükseklik ' + f.height;
                if (e.kind === 'block-rejected' && f.got) extra = ' · got ' + String(f.got).slice(0, 12) + '…';
                const idx = (String(e.node).match(/(\d+)/) || [])[1];
                return (
                  <div className={'logrow lvl-' + e.level} key={i}>
                    <span className="lt">{clockOf(e.ts)}</span>
                    <span className="ln" style={{ color: nodeColor(idx ? +idx : 0) }}>{e.node}</span>
                    <span className={'lk k' + e.kind}>{KIND_TR[e.kind] || e.kind}</span>
                    <span className="lm">{e.msg}{extra}</span>
                  </div>
                );
              }) : <div className="empty">eşleşen olay yok</div>}
            </div>
          )}
    </>
  );
}

// ===========================================================================
// SEKME: Tarama (zincir explorer'ı)
// ===========================================================================
function ScanResult({ result }) {
  if (!result) return null;
  if (!result.ok)
    return <Banner kind="bad" ico="⚠" style={{ marginTop: 12 }}>{result.error || 'Hata'}</Banner>;
  if (result.found === false) {
    const msg = result.kind === 'utxo'
      ? 'Bu UTXO bulunamadı — ya harcanmış (artık bir cüzdanın elinde değil) ya da hiç var olmamış.'
      : result.kind === 'tx'
        ? 'Bu kimlikle işlem bulunamadı. (qv_getTx yalnızca mempool ile son ~50 bloğu tarar.)'
        : 'Bu yükseklik / hash ile blok bulunamadı.';
    return <Banner kind="warn" ico="○" style={{ marginTop: 12 }}>{msg}</Banner>;
  }
  if (result.kind === 'block') {
    const b = result.block;
    return (
      <div className="detail-kv" style={{ marginTop: 12 }}>
        <span className="k">Yükseklik</span><span className="v">{b.height}</span>
        <span className="k">Slot</span><span className="v">{b.slot}</span>
        <span className="k">Üretici lider</span>
        <span className="v" style={{ color: nodeColor(b.leader) }}>{nodeName(b.leader)}</span>
        <span className="k">Zaman damgası</span>
        <span className="v">{b.timestamp ? new Date(b.timestamp * 1000).toLocaleString('tr-TR') : '0'}</span>
        <span className="k">Üst blok</span><span className="v mono">{b.prevHash}</span>
        <span className="k">Merkle kök</span><span className="v mono">{b.merkleRoot}</span>
        <span className="k">UTXO taahhüdü</span><span className="v mono">{b.utxoCommitment}</span>
        <span className="k">İşlem sayısı</span><span className="v">{b.txCount}</span>
        <span className="k">VRF / KES</span><span className="v">{b.vrfLen} B / {b.kesLen} B</span>
      </div>
    );
  }
  if (result.kind === 'tx') {
    const t = result.tx;
    return (
      <div className="tx-item" style={{ marginTop: 12 }}>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'baseline' }}>
          <b>İşlem bulundu</b>
          <span className="muted">sürüm {t.version}</span>
          <span className="muted">ücret {fmt(t.fee)}</span>
          <span className="muted">toplam çıktı <b>{fmt(t.totalOut)}</b></span>
        </div>
        <div className="tx-io">
          <div className="iolist">
            <div className="iohead">Girdiler — harcanan UTXO ({t.inputs.length})</div>
            {t.inputs.length ? t.inputs.map((inp, j) => (
              <div className="io-row" key={j}>
                <span className="mono">{shortHash(inp.txid, 12)}<span className="dim">#{inp.index}</span></span>
                <span className="dim">tanık {inp.witnessLen} B</span>
              </div>
            )) : <div className="dim" style={{ padding: '4px 0' }}>girdi yok (coinbase / genesis)</div>}
          </div>
          <div className="iolist">
            <div className="iohead">Çıktılar — yeni UTXO ({t.outputs.length})</div>
            {t.outputs.map((o, j) => (
              <div className="io-row" key={j}>
                <span>#{j} · <b>{fmt(o.value)}</b> birim</span>
                <span className="dim">script {o.scriptLen}B{o.hasDatum ? ' · datum' : ''}{o.hasStealth ? ' · stealth' : ''}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    );
  }
  if (result.kind === 'utxo') {
    const u = result.utxo;
    return (
      <div style={{ marginTop: 12 }}>
        <Banner kind="ok" ico="✓">Bu UTXO <b>harcanmamış</b> — değeri hâlâ bir cüzdanın elinde.</Banner>
        <div className="detail-kv">
          <span className="k">Outpoint</span><span className="v mono">{result.q}</span>
          <span className="k">Değer</span><span className="v"><b>{fmt(u.value)}</b> birim</span>
          <span className="k">Kilit script hash</span><span className="v mono">{u.script_hash}</span>
          <span className="k">Datum</span><span className="v">{u.has_datum ? 'var' : 'yok'}</span>
          <span className="k">Stealth</span><span className="v">{u.has_stealth ? 'var — gizli alıcı' : 'yok'}</span>
        </div>
      </div>
    );
  }
  return null;
}

function Scan() {
  const [kind, setKind] = useState('block');
  const [query, setQuery] = useState('');
  const [result, setResult] = useState(null);
  const [busy, setBusy] = useState(false);
  const [ov, setOv] = useState(null);

  useEffect(() => {
    let alive = true;
    fetchJSON('/api/scan?kind=overview').then(d => { if (alive) setOv(d); }).catch(() => {});
    return () => { alive = false; };
  }, []);

  const runSearch = async (k, qv) => {
    qv = (qv || '').trim();
    if (!qv) return;
    setBusy(true); setResult(null);
    try { setResult(await fetchJSON('/api/scan?kind=' + k + '&q=' + encodeURIComponent(qv))); }
    catch (e) { setResult({ ok: false, error: e.message }); }
    setBusy(false);
  };
  const lookupUtxo = (op) => { setKind('utxo'); setQuery(op); runSearch('utxo', op); };

  const placeholder = kind === 'block' ? 'blok yüksekliği (örn. 42) ya da 64 haneli blok hash'
    : kind === 'tx' ? '64 haneli işlem kimliği (İşlemler sekmesindeki tx ID\'leri kullanabilirsin)'
    : 'outpoint:  <işlem_id>#<indeks>   örn. fa9ea5…#0';

  const outs = (ov && ov.outputs) || [];

  return (
    <>
      <div className="panel" style={{ marginBottom: 14 }}>
        <h2>UTXO Modeli — Hash, Bakiye, Cüzdan Nedir?</h2>
        <p className="dim" style={{ fontSize: 12.8, lineHeight: 1.6, marginBottom: 10 }}>
          QuantumVault <b>hesap (account)</b> modeli kullanmaz — Bitcoin gibi <b>UTXO</b> modeli
          kullanır. Zincirde "şu cüzdanın bakiyesi şudur" diye saklanan bir sayı <b>yoktur</b>.
          Para, <b>harcanmamış işlem çıktıları</b> hâlinde ayrık parçalar olarak durur.
        </p>
        <div className="lsel-vars">
          <span><b>UTXO</b> (Unspent Transaction Output) — bir işlemin henüz harcanmamış çıktısı; zincirdeki "para parçası". Her UTXO'nun bir değeri, bir kilit script'i (onu kimin açıp harcayabileceği) ve isteğe bağlı datum/stealth bilgisi vardır.</span>
          <span><b>Outpoint</b> — tek bir UTXO'nun adresi; biçimi <span className="mono">&lt;işlem_id&gt;#&lt;indeks&gt;</span> (örn. fa9ea5…#0 = şu işlemin 0. çıktısı).</span>
          <span><b>İşlem</b> — bazı UTXO'ları <b>girdi</b> olarak harcar (yok eder), yeni UTXO'lar <b>çıktı</b> olarak yaratır. Kural: girdiler toplamı = çıktılar toplamı + ücret.</span>
          <span><b>Hash</b> — SHA3-256 kriptografik parmak izi; bir bloğun/işlemin içeriğinin tek ve değiştirilemez kimliği. Bloklar ve işlemler hash'leriyle aranır.</span>
          <span><b>Bakiye</b> — bir cüzdanın bakiyesi = kilit script'i o cüzdanın anahtarlarıyla açılabilen tüm UTXO'ların değerleri <b>toplamı</b>. Bakiye zincirde durmaz, UTXO'lar taranarak hesaplanır.</span>
          <span><b>Cüzdan</b> — bir anahtar kümesi. Bir UTXO'ya "sahip olmak" = onun kilit script'ini açabilen anahtara sahip olmak.</span>
        </div>
      </div>

      <div className="panel" style={{ marginBottom: 14 }}>
        <h2>Zincirde Ara</h2>
        <div className="scan-bar">
          <select value={kind} onChange={e => { setKind(e.target.value); setResult(null); }}>
            <option value="block">Blok</option>
            <option value="tx">İşlem</option>
            <option value="utxo">UTXO</option>
          </select>
          <input value={query} placeholder={placeholder}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') runSearch(kind, query); }} />
          <button onClick={() => runSearch(kind, query)}>{busy ? 'Aranıyor…' : 'Ara'}</button>
        </div>
        <ScanResult result={result} />
      </div>

      <div className="panel">
        <h2>Genesis Dağıtımı — Ağın İlk Cüzdanları</h2>
        <p className="dim" style={{ fontSize: 12, marginBottom: 10 }}>
          Ağ ilk doğduğunda genesis bloğu, başlangıç fonlarını bir dizi UTXO olarak dağıttı —
          aşağıdaki her satır bir başlangıç UTXO'su (bir "cüzdan kesesi"). "harcanmamış" =
          orijinal sahibi hâlâ tutuyor; "harcanmış" = bir işlemle başka cüzdanlara aktarılmış.
          {ov && ov.genesisTxid && <> Genesis işlem kimliği: <span className="mono">{shortHash(ov.genesisTxid, 16)}</span>.</>}
        </p>
        {!ov
          ? <div className="loading">yükleniyor…</div>
          : outs.length === 0
            ? <div className="empty">Genesis verisi alınamadı — devnet çalışıyor mu?</div>
            : outs.map(o => (
                <div className="gout" key={o.index}>
                  <span className="gout-i">#{o.index}</span>
                  {o.wallet
                    ? <span className="pill" style={{ color: 'var(--accent)', borderColor: 'var(--accent)' }}>{o.wallet}{o.account != null ? ' · hesap ' + o.account : ''}</span>
                    : <span className="dim" style={{ fontSize: 11 }}>adsız çıktı</span>}
                  <span className="muted">değer <b>{fmt(o.value)}</b> birim</span>
                  {o.outpoint != null && (o.unspent
                    ? <span className="pill ok">harcanmamış</span>
                    : <span className="pill bad">harcanmış</span>)}
                  {o.pubkeyHash && <span className="dim mono" style={{ fontSize: 10.5 }}>pk {shortHash(o.pubkeyHash, 12)}</span>}
                  {o.outpoint && <button className="gout-btn" onClick={() => lookupUtxo(o.outpoint)}>UTXO'yu ara →</button>}
                </div>
              ))}
        {ov && !ov.genesisTxid && outs.length > 0 && (
          <p className="dim" style={{ fontSize: 11.5, marginTop: 8 }}>
            Not: <span className="mono">wallets.json</span> bulunamadığından genesis işlem
            kimliği bilinmiyor — UTXO'ların canlı harcanma durumu gösterilemiyor, yalnızca
            başlangıç değerleri listeleniyor.
          </p>
        )}
      </div>
    </>
  );
}

// ===========================================================================
// İpucu balonu + Modal
// ===========================================================================
function Tooltip({ hover }) {
  if (!hover) return null;
  const b = hover.b;
  let x = hover.x + 14, y = hover.y + 14;
  if (typeof window !== 'undefined') {
    if (x + 300 > window.innerWidth) x = hover.x - 300;
    if (y + 200 > window.innerHeight) y = hover.y - 200;
  }
  const role = b.rejected ? 'Terk edilmiş dal (orphan)' : (b.canonical ? 'Kanonik blok' : 'Aktif fork dalı');
  return (
    <div className="tip-tooltip" style={{ left: x, top: y }}>
      <div className="tt-h">#{b.height} · {role}</div>
      {b.rejected ? (
        <>
          <div className="row"><span>terk edilen üst</span><b className="mono">{shortHash(b.prevHash, 10)}</b></div>
          <div className="row"><span>reddeden düğüm</span>
            <b>{b.nodes && b.nodes.length ? b.nodes.map(n => typeof n === 'number' ? nodeName(n) : n).join(', ') : '—'}</b></div>
          {b.reasonText && <div className="tt-why">⑂ {b.reasonText}</div>}
        </>
      ) : (
        <>
          <div className="row"><span>hash</span><b className="mono">{shortHash(b.hash, 10)}</b></div>
          <div className="row"><span>üst blok</span><b className="mono">{shortHash(b.prevHash, 10)}</b></div>
          <div className="row"><span>slot</span><b>{b.slot != null ? b.slot : '—'}</b></div>
          <div className="row"><span>lider</span>
            <b style={{ color: b.leader != null ? nodeColor(b.leader) : '#888' }}>{nodeName(b.leader)}</b></div>
          <div className="row"><span>üretici anahtar</span><b className="mono">{shortHash(b.producer, 8)}</b></div>
          <div className="row"><span>işlem</span><b>{b.txCount || 0}</b></div>
          <div className="row"><span>tutan düğüm</span><b>{b.nodes ? b.nodes.map(nodeName).join(', ') : '—'}</b></div>
        </>
      )}
    </div>
  );
}

function OrphanModal({ block }) {
  const b = block;
  return (
    <>
      <h3>Terk edilmiş dal · yükseklik {b.height}</h3>
      <p className="muted">Bu blok bir düğümün loglarında reddedilmiş ve kanonik zincire
        girmemiş (orphan). RPC ile sorgulanamaz; bilgiler düğüm loglarından gelir.</p>
      <Banner kind="warn" ico="⑂" style={{ margin: '12px 0' }}>
        {b.reasonText || 'Blok bu düğümün zincirini geçerli biçimde uzatmadığı için reddedildi.'}
      </Banner>
      <div className="detail-kv">
        <span className="k">Yükseklik</span><span className="v">{b.height}</span>
        <span className="k">Terk edilen üst blok</span><span className="v mono">{b.prevHash}</span>
        <span className="k">Reddeden düğümler</span>
        <span className="v">{b.nodes && b.nodes.length
          ? b.nodes.map(n => typeof n === 'number' ? nodeName(n) : n).join(', ') : '—'}</span>
        {b.firstTs && <><span className="k">İlk görülme</span><span className="v">{clockOf(b.firstTs)}</span></>}
      </div>
    </>
  );
}

function BlockModal({ height, nodeN }) {
  const [b, setB] = useState(null);
  const [err, setErr] = useState(null);
  useEffect(() => {
    let alive = true;
    fetchJSON('/api/block?height=' + height)
      .then(d => { if (alive) setB(d); })
      .catch(e => { if (alive) setErr(e.message); });
    return () => { alive = false; };
  }, [height]);
  if (err) return <><h3>Hata</h3><p className="muted">{err}</p></>;
  if (!b) return <div className="loading">blok #{height} yükleniyor…</div>;
  if (!b.ok) return <h3>Blok bulunamadı</h3>;
  return (
    <>
      <h3>Blok #{b.height} ayrıntıları</h3>
      <div className="detail-kv">
        <span className="k">Yükseklik</span><span className="v">{b.height}</span>
        <span className="k">Slot</span><span className="v">{b.slot}</span>
        <span className="k">Lider düğüm</span>
        <span className="v" style={{ color: nodeColor(b.leader) }}>
          {nodeName(b.leader)} (slot mod {nodeN})
        </span>
        <span className="k">Sürüm</span><span className="v">{b.version}</span>
        <span className="k">Zaman damgası</span>
        <span className="v">{b.timestamp ? new Date(b.timestamp * 1000).toLocaleString('tr-TR') : '0'}</span>
        <span className="k">Üst blok</span><span className="v mono">{b.prevHash}</span>
        <span className="k">Merkle kök</span><span className="v mono">{b.merkleRoot}</span>
        <span className="k">UTXO taahhüdü</span><span className="v mono">{b.utxoCommitment}</span>
        <span className="k">Üretici anahtar</span><span className="v mono">{b.producer}</span>
        <span className="k">VRF kanıtı</span><span className="v">{b.vrfLen} bayt</span>
        <span className="k">KES imzası</span><span className="v">{b.kesLen} bayt</span>
        <span className="k">İşlem sayısı</span><span className="v">{b.txCount}</span>
      </div>
      {b.txs.map((t, i) => (
        <div className="tx-item" style={{ marginTop: 8 }} key={i}>
          <b>İşlem {i + 1}</b> — {t.inputs.length} girdi, {t.outputs.length} çıktı,
          ücret {fmt(t.fee)}, toplam çıktı {fmt(t.totalOut)}
        </div>
      ))}
    </>
  );
}

function Modal({ modal, nodeN, onClose }) {
  if (!modal) return null;
  return (
    <div className="modal" onClick={e => { if (e.target.classList.contains('modal')) onClose(); }}>
      <div className="modal-box">
        <button className="modal-x" onClick={onClose}>×</button>
        <div>
          {modal.kind === 'orphan'
            ? <OrphanModal block={modal.block} />
            : <BlockModal height={modal.height} nodeN={nodeN} />}
        </div>
      </div>
    </div>
  );
}

// ===========================================================================
// Başlık & sekme çubuğu
// ===========================================================================
function Header({ state, connOk, auto, setAuto, onRefresh }) {
  const nodes = (state && state.nodes) || [];
  const up = nodes.filter(n => n.up).length;
  const connClass = 'conn ' + (!connOk ? 'down' : (up === nodes.length && up > 0 ? 'up' : (up > 0 ? '' : 'down')));
  const connText = !connOk ? 'sunucuya ulaşılamıyor' : up + '/' + nodes.length + ' düğüm çevrimiçi';
  const wd = (state && state.config && state.config.workDir) || '';
  const sub = state
    ? 'devnet · ' + nodes.length + ' düğüm · loglar: ' +
      (state.workDirExists ? wd : wd + ' (bulunamadı)') + ' · güncelleme ' + fmtClock(state.ts)
    : 'devnet izleniyor…';
  return (
    <header>
      <div className="brand">
        <span className="logo">◈</span>
        <div>
          <h1>QuantumVault&nbsp;L1 — Node Monitör</h1>
          <div className="sub">{sub}</div>
        </div>
      </div>
      <div className="hdr-right">
        <div className={connClass}><span className="dot"></span><span>{connText}</span></div>
        <label className="auto">
          <input type="checkbox" checked={auto} onChange={e => setAuto(e.target.checked)} /> otomatik
        </label>
        <button id="refreshBtn" title="Şimdi yenile" onClick={onRefresh}>↻</button>
      </div>
    </header>
  );
}

function Nav({ active, onSelect }) {
  return (
    <nav>
      {TABS.map(([id, label]) => (
        <button key={id} className={id === active ? 'active' : ''} onClick={() => onSelect(id)}>
          {label}
        </button>
      ))}
    </nav>
  );
}

// ===========================================================================
// Kök bileşen
// ===========================================================================
function App() {
  const [appState, setAppState] = useState(null);
  const [connOk, setConnOk] = useState(false);
  const [activeTab, setActiveTab] = useState('overview');
  const [forks, setForks] = useState(null);
  const [tx, setTx] = useState(null);
  const [logs, setLogs] = useState(null);
  const [forkWindow, setForkWindow] = useState(48);
  const [logFilterNode, setLogFilterNode] = useState('');
  const [logFilterKind, setLogFilterKind] = useState('');
  const [auto, setAuto] = useState(true);
  const [modal, setModal] = useState(null);
  const [hover, setHover] = useState(null);

  // Tek yoklama turu — etkin sekmenin ağır verisini de çeker.
  const doPoll = useCallback(async () => {
    try {
      const s = await fetchJSON('/api/state');
      setAppState(s); setConnOk(true);
    } catch (e) { setConnOk(false); }
    try {
      if (activeTab === 'forks') setForks(await fetchJSON('/api/forks?count=' + forkWindow));
      else if (activeTab === 'tx') setTx(await fetchJSON('/api/transactions'));
      else if (activeTab === 'logs')
        setLogs(await fetchJSON('/api/logs?count=350' +
          (logFilterNode ? '&node=' + encodeURIComponent(logFilterNode) : '')));
    } catch (e) { /* ağır uç geçici hatası — sessiz geç */ }
  }, [activeTab, forkWindow, logFilterNode]);

  // Canlı güncelleme: React state üzerinden — DOM yerinde yamanır, sayfa sıfırlanmaz.
  useEffect(() => {
    let alive = true;
    const run = () => { if (alive) doPoll(); };
    run();
    if (!auto) return () => { alive = false; };
    const id = setInterval(run, 2500);
    return () => { alive = false; clearInterval(id); };
  }, [doPoll, auto]);

  // Fork verisi yenilenince ipucu balonunu kapat (eski bloğa takılı kalmasın).
  useEffect(() => { setHover(null); }, [forks]);

  const nodeN = (appState && appState.config && appState.config.nodes)
    ? appState.config.nodes.length : 4;

  const onForkBlock = useCallback((b) => {
    setHover(null);
    if (b.rejected || !b.hash || String(b.hash).indexOf('orphan:') === 0)
      setModal({ kind: 'orphan', block: b });
    else
      setModal({ kind: 'block', height: b.height });
  }, []);

  return (
    <>
      <Header state={appState} connOk={connOk} auto={auto} setAuto={setAuto} onRefresh={doPoll} />
      <Nav active={activeTab} onSelect={setActiveTab} />
      <main>
        {activeTab === 'overview' && <Overview state={appState} />}
        {activeTab === 'forks' && (
          <ForkTab forks={forks} state={appState} forkWindow={forkWindow} setForkWindow={setForkWindow}
            onHover={(b, e) => setHover({ b, x: e.clientX, y: e.clientY })}
            onLeave={() => setHover(null)} onBlock={onForkBlock} />
        )}
        {activeTab === 'consensus' && <Consensus state={appState} />}
        {activeTab === 'perf' && <Perf state={appState} />}
        {activeTab === 'tx' && <TxTab tx={tx} state={appState} />}
        {activeTab === 'logs' && (
          <LogsTab logs={logs} state={appState}
            filterNode={logFilterNode} setFilterNode={setLogFilterNode}
            filterKind={logFilterKind} setFilterKind={setLogFilterKind} />
        )}
        {activeTab === 'scan' && <Scan />}
      </main>
      <footer>
        QuantumVault L1 devnet monitörü · veriler JSON-RPC, Prometheus metrikleri ve düğüm
        loglarından toplanır · React
      </footer>
      <Modal modal={modal} nodeN={nodeN} onClose={() => setModal(null)} />
      <Tooltip hover={hover} />
    </>
  );
}

if (typeof document !== 'undefined' && document.getElementById('root')) {
  ReactDOM.createRoot(document.getElementById('root')).render(<App />);
}
