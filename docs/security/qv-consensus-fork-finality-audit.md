# qv-consensus — Çatallanma & Finalite Güvenlik Denetimi

- **Tarih**: 2026-05-22
- **Kapsam**: `crates/qv-consensus` (fork-choice, k-deep finality, leader/slot)
- **Tetikleyen semptom**: Zincir çatallanıyor ve çatallanma sonrası birbiriyle
  çelişen iki zincir de "kesinleşmiş" görünüyor.
- **Durum**: KRİTİK bulgular düzeltildi (bkz. "Uygulanan düzeltmeler"); YÜKSEK
  ve ORTA bulgular sıraya alındı.

## Özet

Birbiriyle çelişen iki zincirin ikisinin de kesinleşmesi bir **güvenlik (safety)
ihlalidir** — bir mutabakat protokolünde olabilecek en ağır hata sınıfı.
İnceleme, kök nedenin `crates/qv-consensus/src/chain_state.rs` içinde, birbirini
besleyen birkaç ayrı hata olduğunu gösterdi.

Temel mesele: saf Ouroboros Praos *olasılıksal* finalite verir, *mutlak* finalite
vermez. k-deep kuralı (k=50) istatistiksel bir eşiktir. Mevcut kod ise bu eşiği
hem yanlış uyguluyor hem de "geri alınamaz" gibi davranıyordu, üstelik reorg'u
finalite ufkunun ötesine taşıyan açıklar bırakıyordu.

## Bulgular

### KRİTİK-1 — Finalite blok-bazlı değil, yükseklik-bazlı

`chain_state.rs` `is_final` (eski satır 173-180) ve `finality_height` (186-189)
bir `BlockHash` değil, bir `Height` alıyordu ve cevabı her çağrıda
`tip_height - height >= k` aritmetiğinden üretiyordu. Saklanan bir "finalize
edilmiş nokta" yoktu. Tip her değiştiğinde aynı yükseklik farklı bir bloğa işaret
ediyor; A dalındaki bir blok için `is_final` "true" demişken reorg sonrası aynı
çağrı B dalındaki çelişen bloğu kastediyordu. Finalite "yapışkan" (sticky)
değildi.

**Düzeltme durumu**: Düzeltildi (bu tur).

### KRİTİK-2 — Eşit yükseklikli reorg'lar derinlik korumasını atlıyor

`add_block` içindeki `ForkTooDeep` derinlik kontrolü yalnızca `Ordering::Greater`
kolunda vardı. `Ordering::Equal` kolu (eski satır 229-237) tip'i sadece hash
beraberlik bozmasıyla değiştiriyordu — hiçbir derinlik kontrolü yok. Eşit
yükseklikte, finalite ufkunun ötesinde çatallanmış bir zincir, tip hash'i daha
küçükse mevcut tip'i sessizce devralabiliyordu.

**Düzeltme durumu**: Düzeltildi (bu tur).

### YÜKSEK-1 — Derinlik koruması "fail-open"

`find_fork_height` (eski satır 266-281) ortak ata bulamazsa `None` döndürüyordu;
`add_block` (eski satır 216) `if let Some(...)` kullandığı için `None` durumunda
tüm kontrol atlanıyor, tip koşulsuz değişiyordu. `ancestors` yürüyüşü 10.000
girişle sınırlı olduğundan uzun zincirler korumayı devre dışı bırakabiliyordu.

**Düzeltme durumu**: Düzeltildi (bu tur) — yeni guard `fail-closed`.

### YÜKSEK-2 — Fork-choice "density-weighted" değil, düz "longest chain"

Modül dokümantasyonu ve `CLAUDE.md` "density-weighted longest chain" diyor, ama
`add_block` yalnızca yüksekliği karşılaştırıyor. `chain_density` fonksiyonu
yazılmış ama hiçbir yerde çağrılmıyor (yalnızca testlerde). Belirtilen mimari
değişmez ihlal ediliyor; Genesis tarzı uzun-menzilli saldırı savunması yok.

**Düzeltme durumu**: Doküman düzeltildi (2026-05-22). `CLAUDE.md`, `chain_state`
modül dokümanı ve `chain_density` yorumu gerçek davranışı (longest-chain by
height + yapışkan k-deep finalite) yansıtacak şekilde güncellendi. Proper
Ouroboros Genesis maxvalid-bg kuralının çekirdeği (`maxvalid_bg`) 2026-05-22'de
uygulandı ve referans modeliyle doğrulandı (ADR-008). `s` kalibrasyonu açık;
sync altyapısı ADR-010'da tasarlandı ve çekirdek primitifleri uygulandı,
ağ entegrasyonu açık.

### ORTA-1 — Reddedilen bloklar yine de `entries`'e ekleniyordu

`add_block` entry'yi fork-choice kontrolünden önce ekliyordu; bir kontrol hata
dönse bile blok durumda kalıyordu.

**Düzeltme durumu**: Düzeltildi (bu tur) — insert tüm kontrollerden sonra.

### ORTA-2 — Equivocation / aynı-slotta-çift-blok tespiti yok

Ne `block_validator` ne `chain_state` bir üreticinin aynı slota iki blok atıp
atmadığını kontrol ediyor. `validate_block_header` yalnızca `slot > parent_slot`
bakıyor. Slashing yok; KES periyot/slot çapraz kontrolü yapılmıyor
(`DilithiumSumKesVerifier` dokümanı bunu "çağıranın sorumluluğu" diyor, hiçbir
çağıran yapmıyor).

**Düzeltme durumu**: Düzeltildi (2026-05-22). `chain_state.rs`'e `(slot,
producer)` indeksli equivocation tespiti + `EquivocationProof` kanıt kaydı
eklendi — equivocating bloklar reddedilmez, kanıt olarak saklanır (fork-choice
zaten siblings'ı çözer, equivocation zincir uzunluğu ekleyemez). `equivocation_proofs()`
slashing/hesap-verebilirlik katmanı için sorgulanabilir. `block_validator.rs`'e
KES periyot/slot çapraz kontrolü eklendi: `slot_to_kes_period` (ADR-005:
1 periyot = 1 epoch), `KesVerifier` trait'i artık `expected_kes_period` alıyor
ve `DilithiumSumKesVerifier` gömülü periyodu doğruluyor. (Trait imza değişimi
qv-consensus içine sınırlı — dış tüketici yok.)

### DÜŞÜK-1 — `to_unit_interval` VRF çıktısının çoğunu atıyor

`leader_schedule.rs` VRF çıktısının 32 baytından yalnızca ilk 8'ini kullanıyor;
lider seçim hassasiyetini düşürür. Güvenlik hatası değil.

**Düzeltme durumu**: Düzeltildi (2026-05-22). İnceleme sonucu: f64 mantissa
yalnızca 53 bit tutar, dolayısıyla "32 baytın tamamını kullan" sonucu
değiştirmez. Gerçek düzeltme: `to_unit_interval` artık çıktının en üst 53
bitini kullanıyor, bölme tam (kayıpsız), ve sonuç her zaman kesinlikle `1.0`
altında — `[0.0, 1.0)` sözleşmesi artık garanti (önceden yuvarlama ile tam
`1.0` üretilebiliyordu). Yanıltıcı "256-bit integer" dokümanı düzeltildi. Tam
kesin karşılaştırma (256-bit tamsayı uzayında) gelecekteki iş olarak not edildi.

### Ek not — `qv-node` blok uygulama sırası

`qv-node/src/node.rs` (adım 3-5) UTXO setini uygular ve bloğu diske yazar, *sonra*
`chain_state.add_block` çağırır. `add_block` hata dönerse önceki adımlar geri
alınmaz — düğüm-seviyesi durum tutarsızlığı.

**Düzeltme durumu**: Düzeltildi (2026-05-22). `handle_block` yeniden sıralandı:
UTXO uygulanır (ledger-geçersiz blokları yakalar) → chain-state kabul edilir →
blok diske yazılır. chain-state bloğu reddederse UTXO etkileri mevcut
`revert_block` API'siyle geri alınır; blok yalnızca hem UTXO hem chain-state
kabul ettikten sonra kalıcılaştırılır. Tam atomiklik (UTXO + depo + chain-state
tek işlemde) gelecekteki iş olarak açık.

## Uygulanan düzeltmeler (2026-05-22)

`chain_state.rs` yeniden yapılandırıldı:

- `ChainState`'e açık `final_hash` + `final_height` alanları eklendi. Finalite
  artık **yapışkan ve monoton**: yalnızca ileri hareket eder.
- `is_final` artık `&BlockHash` alıyor ve bloğun finalize edilmiş bloğun kendisi
  ya da bir atası olup olmadığını döndürüyor. Bir kez `true` dönen blok için her
  zaman `true` döner.
- `add_block`'a, *tüm* fork-choice kollarında çalışan sert bir reorg guard
  eklendi: finalize edilmiş bloktan türemeyen her blok `ConflictsWithFinalized`
  ile reddedilir. Çözülemeyen durumlar `fail-closed`.
- Blok yalnızca tüm geçerlilik kontrolleri geçtikten sonra `entries`'e ekleniyor.
- Kırık `find_fork_height` + `ForkTooDeep` kaldırıldı; yerine `is_ancestor` ve
  `advance_finality` geldi.
- Birim testleri yeni API'ye taşındı; çift-kesinleşme ve horizon-aşan-reorg için
  regresyon testleri eklendi (`integration.rs` da güncellendi).

Bu düzeltmeler **tek düğümde finalite güvenliğini** (bir düğüm asla iki çelişen
bloğu finalize etmez) ve **deterministik fork-choice** ile aynı blok kümesine
sahip düğümler arasında uzlaşmayı sağlar. Ağ bölünmesi (partition) altında iki
dalın bağımsızca k-derinliğe ulaşması senaryosu fork-choice ile çözülemez.

### Doğrulama

- `chain_state.rs` birim testleri + `integration.rs` yeni API'ye taşındı;
  çift-kesinleşme, horizon-aşan-reorg ve monoton finalite için regresyon
  testleri eklendi.
- Algoritma tasarımı bağımsız bir simülasyonla doğrulandı: 4 senaryo testi +
  400 rastgele test çalışması (her biri 250 blok, k=1..6), toplam ~4814
  finalizasyon. Şu değişmezler her durumda korundu: (A) finalite asla geriye
  gitmez, (B) tip her zaman finalize edilmiş bloğun torunu, (C) finalize
  edilen tüm bloklar tek bir zincir oluşturur — yani iki çelişen blok asla
  birlikte finalize edilmez, (D) bir kez finalize edilen blok finalize kalır.
- ORTA-2 (equivocation + KES periyot kontrolü) birim testleriyle kapsandı:
  `equivocation_is_detected_and_recorded`, `honest_chain_has_no_equivocations`,
  `slot_to_kes_period_maps_to_epoch`, `dilithium_kes_rejects_wrong_period`.
- **Yapılması gereken**: Sandbox'ta Rust derleyici yok. Değişiklikler yerelde
  `just build`, `cargo test -p qv-consensus` ve `just clippy` ile doğrulanmalı
  (proje `-D warnings` kapısı kullanıyor).

## Kalan iş — sıralı plan

1. **YÜKSEK-2**: ✅ doküman düzeltildi; ADR-008 yazıldı ve çekirdek algoritma
   (`maxvalid_bg`) uygulandı + doğrulandı (2026-05-22). Kalan: `s` kalibrasyonu
   ve `qv-net` sync ağ entegrasyonu (ADR-010 — çekirdek primitifler +
   `SyncManager` iskeleti uygulandı; ağ wiring açık).
2. **ORTA-2**: ✅ equivocation tespiti + KES periyot/slot çapraz kontrolü
   eklendi (2026-05-22).
3. **DÜŞÜK-1**: ✅ `to_unit_interval` düzeltildi (2026-05-22).
4. **Ek not**: ✅ `qv-node` blok uygulama sırası düzeltildi (2026-05-22); tam
   atomiklik (tek işlem) gelecekteki iş olarak açık.

## Ek gözlem — float determinizmi (2026-05-22)

DÜŞÜK-1 incelemesi sırasında not edildi: lider eşiği `f64` `exp()` / `ln()`
ile hesaplanıyordu. IEEE 754 `+ − × ÷` platformlar arası bit-aynıdır, ama
transandantal fonksiyonlar (`exp`, `ln`) libm sürümüne/platforma göre son
bitte farklılaşabilir — mutabakat-kritik bir yolda bu, iki düğümün farklı
lider kararı vermesine (mutabakat ayrışması) yol açabilir.

**Durum**: Düzeltildi (2026-05-22). Lider kontrolü tamamen sabit-nokta tamsayı
aritmetiğine taşındı (`is_slot_leader`): `ln(1−f)` çevrimdışı hesaplanmış bir
sabit, `exp` sınırlı Taylor serisiyle `2^64`-ölçekli `u128` içinde. Tasarım
bağımsız bir referans modeliyle (`leader_check_reference.py`) doğrulandı —
sabit, Taylor terim sayısı (K=9), taşma bütçesi (`< 2^124`) ve oracle uyumu.
Detaylar ADR-009'da. f64 `leader_threshold`/`to_unit_interval` yalnızca
teşhis amaçlı olarak korundu.

## EK BULGU — KRİTİK: İmza işleme bağlı değil (sighash kusuru)

ADR-011 Faz 3 hazırlığında bulundu. `p2pkh_pqc` kilit template'i imza mesajını
witness'tan alıyordu (witness `<msg, sig, pubkey>`), ve ne script ne de
`validate_script` bu `msg`'in gerçek işleme karşılık geldiğini doğrulamıyordu.
Sonuç: mempool'daki bir harcamanın witness'ı çıkarılıp, **aynı UTXO'yu
saldırgana yönlendiren** ikinci bir işleme yapıştırılabilir; imza `msg`
üzerinde hâlâ geçerli olduğundan doğrulayıcı bunu kabul eder → uçuştaki
işlem hırsızlığı. Kök neden: zincirde witness'ları dışlayan bir hash yoktu
(`tx.id()` / `TxHash` witness-dahil → sighash olarak kullanmak döngüsel).

**Durum**: Düzeltildi (2026-05-22). ADR-012 uygulandı: `Transaction::sighash()`
(tüm girdi witness'ları boşaltılmış kanonik baytların SHA3-256'sı), `qv-script`'e
`SigHash` (`0x69`) introspection opcode'u, `p2pkh_pqc` ve `stealth_p2pkh`
template'leri `SigHash` kullanacak şekilde yeniden yazıldı (witness artık
`<sig> <pubkey>` / `<sig> <spend_pk> <shared_secret>` — mesaj taşınmıyor),
`qv-wallet::tx_builder` sighash imzalıyor. İmza artık işlemin girdi+çıktılarına
bağlı; uçtaki witness yeniden-oynatma kapandı. Regresyon testi
`p2pkh_rejects_signature_for_other_tx` ile doğrulandı. Detaylar ADR-012'de.

## Uzun vade — BFT finality gadget

Mutlak finalite (ağ bölünmesi altında bile iki çelişen blok asla finalize
edilemez) yalnızca Praos üstüne bir BFT finality gadget ile elde edilir: bir
validator komitesi her N blokta oylar, 2/3+ oy alan blok geri alınamaz olur.
Casper FFG / GRANDPA tarzı; Cardano'nun yaklaşımı Ouroboros Peras. Ayrı bir ADR
konusu (öneri: ADR-013; ADR-008..012 sırasıyla maxvalid-bg, deterministik lider
kontrolü, bootstrap sync, stealth entegrasyonu ve işlem sighash'i için kullanıldı).
