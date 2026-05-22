# ADR-010: Bootstrap Senkronizasyon Altyapısı

**Durum:** Kısmen uygulandı (2026-05-22) — çekirdek primitifler + `SyncManager` iskeleti kodlandı; ağ entegrasyonu açık
**Tarih:** 2026-05-22
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-consensus/src/chain_state.rs` (`build_locator`, `select_headers_for_locator` — uygulandı); `crates/qv-node/src/sync.rs` (`SyncManager` iskeleti — uygulandı); `crates/qv-node/src/network_handler.rs` + `crates/qv-net` (ağ entegrasyonu — açık)

---

## Bağlam

Yeni başlayan ya da uzun süre çevrimdışı kalmış bir düğüm, ağın güncel
zincirine yetişmek (catch-up) zorundadır. Şu an bu altyapı **yok**:

- `qv-net/src/message.rs` sync mesaj tiplerini tanımlıyor (`GetHeadersMsg`,
  `HeadersMsg`, `GetBlocksMsg`) ama bunları işleyen mantık yok.
- `qv-node/src/network_handler.rs` `Headers`/`GetHeaders`/`GetBlocks`
  mesajlarını yalnızca `warn!("unexpected ...")` ile karşılıyor.
- Bir senkronizasyon durum makinesi, locator üretimi, aday-zincir toplama
  veya blok indirme döngüsü bulunmuyor.

ADR-008'de uygulanan `maxvalid_bg` çatal-seçim fonksiyonu tam olarak bu
altyapının **karar noktasında** kullanılmak üzere tasarlandı: düğüm bir
peer'dan aday header zinciri aldığında, onu yerel zinciriyle karşılaştırır.
Bu ADR, `maxvalid_bg`'nin yerleşeceği bootstrap senkronizasyon altyapısını
belirtir.

## Karar

### Sync durum makinesi

Düğüm iki modda çalışır:

- **Syncing** — aktif catch-up. Açılışta bu modda başlanır.
- **Live** — senkronize; yalnızca gossip izlenir (mevcut davranış).

Geçişler: Syncing → Live, yerel tip en iyi peer'ın tip'iyle eşleştiğinde
(ya da header cevapları boş/kısa geldiğinde). Live → Syncing, gossip'ten
yerel tip'ten belirgin biçimde (ör. > 1 blok) ileride bir blok görülürse.

### Faz 1 — Header senkronizasyonu

1. **Locator üretimi.** Yerel zincirden, tip'e yakın yoğun, genesis'e
   doğru üstel seyrekleşen bir blok-hash listesi: `tip, tip−1, tip−2,
   tip−4, tip−8, …, genesis`. `GetHeadersMsg.locator_hashes` bunu taşır;
   `stop_hash = ZERO` → "elinden geldiğince, `MAX_HEADERS_PER_MSG` kadar".
2. Bir peer'a `GetHeaders` gönderilir. Peer, locator'daki bildiği en
   yüksek hash'i bulur ve sonrasındaki header'ları artan yükseklik
   sırasında `HeadersMsg` ile döner.
3. Gelen header'lar, bilinen ortak önek üzerine eklenerek bir **aday
   header zinciri** oluşturulur.

### Faz 2 — Aday-zincir seçimi (maxvalid_bg karar noktası)

Düğüm, yerel kanonik zincirini ([`ChainState::canonical_chain`]) ve aday
zinciri elde ettikten sonra:

```text
maxvalid_bg(local, candidate, k = k_finality, s = DEFAULT_MAXVALID_WINDOW_SLOTS)
```

çağırır. `AdoptCandidate` → Faz 3'e geçilir. `KeepLocal` → o peer'ın
zinciri daha iyi değil; başka peer denenir ya da sync tamamlanır.

Bootstrap sırasında düğümün güvendiği bir finalite çapası yoktur; derin
çatallar beklenir ve maxvalid-bg'nin yoğunluk kuralı, kötü niyetli bir
peer'ın uzun-seyrek zincir beslemesine karşı koruyan şeydir (ADR-008).

Birden çok peer'dan aday geldiğinde düğüm ikili karşılaştırmayı katlar:
en iyi adayı tutar, her yeni adayı onunla karşılaştırır. Eclipse direnci
için **birden fazla peer** sorgulanır, tek peer'a güvenilmez.

### Faz 3 — Blok indirme ve uygulama

1. Benimsenen aday için, henüz elde olmayan bloklar `GetBlocks` ile sırayla
   istenir (`MAX_BLOCK_LOCATORS` parça parça).
2. Her blok mevcut blok-uygulama yoluyla işlenir: yapısal doğrulama →
   konsensüs header doğrulama → UTXO uygula → `chain_state.add_block` →
   diske yaz (bkz. denetimde düzeltilen `handle_block` sırası).
3. **Reorg.** Aday, mevcut tip'in altında çatallanıyorsa, yerel bloklar
   çatal noktasına kadar geri alınır (`qv-storage::revert_block` mevcut),
   sonra aday blokları uygulanır. Yapışkan k-deep finalite, finalize
   edilmiş noktanın ötesine reorg'u reddeder; bootstrap'ta finalite
   noktası genesis'e yakın olduğundan derin reorg'a izin verilir.

### Header doğrulama (epoch-artımlı)

Bir header'ı tam doğrulamak (VRF lider kanıtı, KES) o epoch'un stake
dağılımını ve nonce'unu gerektirir. Bootstrap, zinciri epoch epoch
kurarken bu durumu artımlı türetir. Faz 1'de en azından yapısal +
linkage + slot monotonluğu kontrol edilir; tam VRF/KES doğrulaması
bloklar uygulanırken (Faz 3) yapılır.

### Yeni bileşenler

- **`SyncManager`** (`qv-node/src/sync.rs`) — durum makinesi; locator
  üretir, `GetHeaders`/`GetBlocks` ister, `maxvalid_bg` ile karar verir,
  blok uygulama yolunu sürer. `ChainState`'e erişir.
- **`NodeEvent` eklentileri** — ör. `HeadersReceived`, `BlocksReceived`,
  `SyncTick`.
- **`NetworkHandler` yönlendirmesi** — `GetHeaders`/`Headers`/`GetBlocks`
  artık `warn!` yerine `SyncManager`'a yönlendirilir (request-response
  kanalı, gossip değil).
- **Sunucu tarafı** — bir `GetHeaders` alan düğüm, locator'dan ortak
  noktayı bulup kendi zincirinden `HeadersMsg` ile yanıt verir.

### Parametreler

- `k` = `ConsensusParams.k_finality`.
- `s` = `chain_state::DEFAULT_MAXVALID_WINDOW_SLOTS` (2160 slot, ADR-008
  baseline). İleride `ConsensusParams`/`genesis.toml`'a taşınmalı.

### DoS / sağlamlık

`MAX_HEADERS_PER_MSG` ve `MAX_BLOCK_LOCATORS` sınırları zorlanır; aşırı
büyük cevaplar reddedilir; her istek için zaman aşımı; tek peer'a
güvenilmez; geçersiz header/blok dönen peer cezalandırılır/düşürülür.

## Uygulama durumu (2026-05-22)

Çekirdek — saf, derleyici-bağımsız doğrulanabilir — parçalar kodlandı:

- `qv-consensus/src/chain_state.rs`: `build_locator` (locator üretimi) ve
  `select_headers_for_locator` (sunucu tarafı `GetHeaders` seçim mantığı)
  saf fonksiyonları + birim testleri.
- `qv-node/src/sync.rs`: `SyncManager` saf durum makinesi iskeleti
  (`SyncState`, `SyncAction`; `on_tick` / `on_headers` kararları
  `build_locator` ve `maxvalid_bg`'yi kullanır) + birim testleri. Hiç
  `async` / libp2p I/O içermez — bu yüzden tamamen birim-test edilebilir.

**Açık kalan iş**: aşağıdaki ağ wiring'i. libp2p `NetworkBehaviour`, codec ve
swarm olay eşleştirmesi tip/sürüm açısından hassas olduğundan bu adımlar
derleyici ve çalışan bir ağ ortamında uygulanmalıdır.

### Ağ wiring — adım adım uygulama planı

1. **qv-net — sync request-response protokolü.** `handshake.rs`'teki
   `request_response::Behaviour` desenini taklit ederek `/quantumvault/sync/1.0.0`
   protokolünü `QvBehaviour`'a ekle. İstek/yanıt zarfı: `SyncRequest`
   (`GetHeaders` | `GetBlocks`) ve `SyncResponse` (`Headers` | `Blocks`).
   Codec mevcut bincode codec'inden türetilebilir.
2. **qv-net — `NetEvent` + komut kanalı.** `NetEvent`'e `SyncRequest { peer,
   channel, msg }` ve `SyncResponse { peer, msg }` ekle. Düğümün istek
   gönderebilmesi için `command_sender` benzeri bir sync-komut kanalı
   (`request_response::Behaviour::send_request`'i sarar); yanıt için
   `send_response(channel, ...)`.
3. **qv-node — `NodeEvent`.** Ekle: `SyncTick`, `HeadersReceived { peer,
   headers }`, `BlocksReceived { blocks }`, `GetHeadersRequested { peer,
   channel, msg }`.
4. **qv-node — `SyncManager`'ı `Node`'a koy.** `Node` struct'ına
   `sync: SyncManager` alanı; `SyncManager::new(consensus.k_finality,
   DEFAULT_MAXVALID_WINDOW_SLOTS)` ile kur.
5. **qv-node — olay döngüsü kolları** (`run()` içindeki `select!`):
   - `tokio::time::interval` kolu → `SyncTick` → `sync.on_tick(chain_state
     .canonical_chain())`; `RequestHeaders` ise bir peer'a `GetHeaders` gönder.
   - `HeadersReceived` → gelen `BlockHeader`'ları `ChainEntry` aday zincirine
     çevir → `sync.on_headers(local, candidate)`; `RequestBlocks` ise
     `GetBlocks` gönder.
   - `BlocksReceived` → blokları sırayla `handle_block` yoluyla uygula;
     gerekirse `qv-storage::revert_block` ile çatal noktasına reorg.
6. **qv-node — sunucu tarafı.** `GetHeadersRequested` kolu →
   `select_headers_for_locator(chain_state.canonical_chain(),
   &msg.locator_hashes, msg.stop_hash, MAX_HEADERS_PER_MSG)` → her hash için
   `block_store.get_block(&hash)?.header` → `HeadersMsg` yanıtı.
7. **Live geçişi.** Header yanıtı boş/kısa gelince `sync.mark_caught_up()`;
   gossip'ten tip'ten belirgin ileride blok görülünce `sync.trigger_resync()`.
8. **network_handler.rs.** `Headers`/`GetHeaders`/`GetBlocks` artık `warn!`
   yerine 3. adımdaki `NodeEvent`'lere yönlendirilir.

DoS: her istek için zaman aşımı; `MAX_HEADERS_PER_MSG`/`MAX_BLOCK_LOCATORS`
sınırları; tek peer'a güvenme — birden çok peer'a sor.

## Sonuçlar

### Olumlu

- `maxvalid_bg` gerçek bir karar noktasına bağlanır; checkpoint'siz,
  eclipse'e dirençli bootstrap mümkün olur.
- Sync mesaj tipleri (`GetHeaders`/`Headers`/`GetBlocks`) nihayet işlevsel
  hâle gelir.

### Olumsuz

- Yeni bir alt sistem — durum makinesi, request-response plumbing, reorg
  uygulama; mutabakat/ağ-kritik, dikkatli test gerektirir.
- Bootstrap sırasında header doğrulama (epoch-artımlı stake/nonce) inceliklidir.

### Nötr / İleride

- `s`'in `ConsensusParams`'a taşınması (ADR-008 ile ortak iş).
- Hızlı senkronizasyon (snapshot/state-sync) ileride ayrı bir ADR olabilir.
- Bir BFT finality gadget eklenirse, sync sonrası finalize edilmiş bloklar
  güvenilir bir çapa sağlar (ayrı ADR).

## Alternatifler (kısa)

| Alternatif | Neden seçilmedi |
|---|---|
| Güvenilir checkpoint ile sync | Checkpoint'siz, istemci-tarafı doğrulama hedefiyle çelişir |
| Tek peer'dan indir | Eclipse saldırısına açık; çoklu peer + maxvalid-bg gerekli |
| Snapshot/state-sync (header zinciri yerine) | Daha hızlı ama UTXO snapshot güveni gerektirir; ileride ayrı ADR |

## Doğrulama / Test (planlanan)

- `SyncManager` durum makinesi birim testleri (Syncing↔Live geçişleri).
- Locator üretimi ve sunucu tarafı `GetHeaders` yanıtı testleri.
- Çok düğümlü entegrasyon testi: yeni düğüm, dürüst ve saldırgan peer'lar
  arasından doğru zincire yetişmeli (`maxvalid_bg` devrede).
- Reorg testi: aday zincir mevcut tip'in altında çatallanınca doğru
  geri-alma + uygulama.

## Bağlantılı

- ADR-008 — Genesis maxvalid-bg; bu altyapının Faz 2 karar noktası.
- `docs/security/qv-consensus-fork-finality-audit.md` — yapışkan k-deep
  finalite ve düzeltilen `handle_block` blok-uygulama sırası.
- `crates/qv-net/src/message.rs` — mevcut sync mesaj tipleri.
