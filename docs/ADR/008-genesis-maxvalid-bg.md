# ADR-008: Genesis maxvalid-bg — Bootstrap Çatal Seçimi

**Durum:** Kısmen uygulandı (2026-05-22) — çekirdek algoritma kodlandı; sync entegrasyonu açık
**Tarih:** 2026-05-22
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-consensus/src/chain_state.rs` (`maxvalid_bg`, `ChainPreference`, `canonical_chain` — uygulandı); `crates/qv-net` (senkronizasyon entegrasyonu — açık)

---

## Bağlam

`CLAUDE.md` mutabakat çatal seçimini **"density-weighted longest chain"** olarak
listeliyor. 2026-05-22 tarihli fork/finalite denetimi
(`docs/security/qv-consensus-fork-finality-audit.md`, bulgu YÜKSEK-2) ortaya
koydu ki kod aslında bunu uygulamıyor: `add_block` düz **longest-chain by
height** kullanıyor, `chain_density` fonksiyonu yazılmış ama hiçbir yere bağlı
değil. Yani doküman ile kod çelişiyordu.

"Density-weighted longest chain" ifadesi pratikte **Ouroboros Genesis'in
maxvalid-bg** ("maximum valid, by genesis") kuralını tanımlar. Burada iki ayrı
mutabakat senaryosunu net ayırmak gerekir:

1. **Kararlı durum (steady state)** — zaten senkronize, çalışan bir düğüm.
   Bu düğüm için doğru kural longest-chain'dir; üstüne 2026-05-22 denetimiyle
   eklenen **yapışkan k-deep finalite** gelir: finalize edilmiş bloğun ötesine
   reorg artık imkânsız. Bu senaryo şu an doğru çalışıyor; bu ADR'i
   gerektirmiyor.

2. **Bootstrap / uzun süre çevrimdışı kalmış düğüm** — henüz güvendiği bir
   finalize edilmiş çapa noktası olmayan, genesis'ten ya da eski bir
   durumdan senkronize olan düğüm. Bu düğüm, eş'lerin (peer) sunduğu birden
   fazla **tam aday zinciri** arasından seçim yapmak zorunda. Saf
   longest-chain burada zayıftır: bir saldırgan, küçük stake ile uzun ama
   **seyrek** (düşük yoğunluklu) bir zincir üretip yeni düğümü yanlış dala
   kilitleyebilir (eclipse / long-range varyantı). Praos'un güvenlik
   ispatları "düğüm zaten ağdaydı" varsayımına dayanır; bootstrap bu
   varsayımı karşılamaz.

Genesis maxvalid-bg tam olarak (2) senaryosunu, **güvenilir bir checkpoint
gerektirmeden** çözmek için tasarlanmıştır. QuantumVault istemci-tarafı
doğrulamalı ve checkpoint'siz bir L1 hedeflediğinden bu kural mimari niyetin
parçasıdır.

## Karar

Çatal seçimini iki kademeli hale getiririz:

### 1. Kararlı durum (değişiklik yok)

Senkronize bir düğüm artımlı `add_block` ile longest-chain by height +
deterministik tie-break (en düşük blok hash) + yapışkan k-deep finalite
kullanmaya devam eder. Bu yol 2026-05-22 denetiminde düzeltildi ve doğrulandı.

### 2. Bootstrap / aday-zincir seçimi — maxvalid-bg

Bir düğüm, eş'lerden gelen aday zincirler `{C_1, …, C_n}` arasından yerel
zinciri `C_loc` ile karşılaştırırken her `C_i` için:

* `j` = `C_loc` ile `C_i`'nin **son ortak bloğunun** (kesişim / çatal noktası)
  slot'u olsun.
* **Sığ çatal** — `C_i`, `C_loc`'tan `k` bloktan daha derinde *ayrılmıyorsa*:
  standart kural, daha uzun zincir tercih edilir.
* **Derin çatal** — `C_i`, `k` bloktan daha derinde ayrılıyorsa: toplam uzunluk
  **karşılaştırılmaz**. Bunun yerine, `j`'den hemen sonraki **`s` slotluk
  pencerede** her iki zincirin blok sayısı (yoğunluğu) karşılaştırılır; bu
  pencerede daha yoğun olan zincir seçilir.

Sezgi: dürüst stake çoğunluğu, herhangi bir `s`-slotluk pencerede bir
saldırgandan ezici olasılıkla daha fazla blok üretir (zincir-büyüme özelliği).
Bu yüzden çatal noktasından *hemen sonraki* pencere, "hangisi dürüst zincir"
sorusunu uzunluğa bakmadan ayırt eder.

### Parametre `s`

`s` (Genesis "öngörü/forecast" penceresi) güvenlik parametresidir: dürüst
yoğunluk avantajının istatistiksel olarak netleşmesi için yeterince uzun
olmalı; `k` ve aktif slot katsayısı `f` ile ilişkilidir. **Somut değer bu
taslakta sabitlenmemiştir** — `f=0.05`, slot=2 sn, `k=50` parametreleri için
çekirdek mutabakat simülasyonuyla kalibre edilmelidir (kabaca büyüklük: birkaç
`k` mertebesinde slot). Kalibrasyon, ADR onaylanmadan önce yapılacak iş.

### API yüzeyi

* `chain_state.rs`'e `s`-slotluk pencere yoğunluğunu çatal noktasına göre
  hesaplayan bir fonksiyon eklenir. Mevcut `chain_density` tip-göreli bir
  pencere alıyor; maxvalid-bg "çatal noktasından sonraki pencere" istediği
  için bu fonksiyon ya genişletilir ya da yanına çatal-göreli bir varyant
  konur.
* `add_block`'tan ayrı, **aday tam zincirleri karşılaştıran** bir giriş
  noktası tanımlanır (ör. `select_best_chain`). Artımlı `add_block` kararlı
  durum içindir; maxvalid-bg toplu/bootstrap senkronizasyonu içindir.
* `qv-net` senkronizasyon mantığı, bootstrap sırasında eş'lerden gelen
  zincirleri bu giriş noktasıyla seçer. Düğüm bir finalite çapası kurduktan
  sonra kararlı-durum yoluna geçer.

### Yapışkan finalite ile ilişki

Yapışkan k-deep finalite ile maxvalid-bg **çelişmez, tamamlar**: finalite
kararlı durumu (çalışan düğüm asla finalize edileni geri almaz) korur;
maxvalid-bg ise düğüm bir finalite çapasına *sahip olmadan önceki* dönemi
korur. Düğüm bir kez finalize edilmiş bloğa demir attığında maxvalid-bg'nin
derin-çatal kolu zaten devreye giremez (denetimde eklenen guard onu reddeder).

## Uygulama durumu (2026-05-22)

Çekirdek algoritma `crates/qv-consensus/src/chain_state.rs` içinde uygulandı:

- `maxvalid_bg(local, candidate, k, s) -> ChainPreference` — pür fonksiyon;
  sığ çatalda longest-chain, derin çatalda `s`-slot pencere yoğunluğu.
- `ChainPreference` enum'u, `canonical_chain()` yardımcı metodu ve
  `DEFAULT_MAXVALID_WINDOW_SLOTS = 2160` baseline sabiti.
- Tasarım bağımsız referans modeliyle doğrulandı
  (`docs/security/maxvalid_bg_reference.py`): senaryo testleri + rastgele
  dürüst/saldırgan derin-çatal testi (dürüst zincir 364/364 doğru seçildi) +
  `s` penceresi taraması. Rust birim testleri eklendi.

**Açık kalan iş**: (1) `s`'in 51/49 en kötü durum için biçimsel kalibrasyonu;
(2) `s`'in `ConsensusParams` / `genesis.toml`'a parametre olarak taşınması;
(3) `qv-net` senkronizasyon mantığının `maxvalid_bg`'yi bootstrap'ta çağırması
(altyapı tasarımı: ADR-010).

## Sonuçlar

### Olumlu

* `CLAUDE.md`'deki "density-weighted longest chain" niyeti gerçek bir kuralla
  karşılanır; doküman/kod çelişkisi (YÜKSEK-2) kapanır.
* Checkpoint gerektirmeden bootstrap eclipse / long-range seyrek-zincir
  saldırılarına dayanıklılık — istemci-tarafı doğrulama felsefesiyle uyumlu.

### Olumsuz

* Mutabakat karmaşıklığı artar; ikinci bir çatal-seçim yolu bakım yükü.
* `s` parametresi kalibrasyon ve simülasyon gerektirir; yanlış `s` ya
  güvenliği ya canlılığı zayıflatır.
* Derin-çatal yoğunluk hesabı, uzun aday zincirlerde maliyetli olabilir.

### Nötr / İleride

* Alternatif/tamamlayıcı olarak weak-subjectivity checkpoint'leri tartışılabilir
  ama checkpoint'siz tasarım tercih ediliyor.
* Mutlak (BFT) finalite ayrı bir problemdir — ağ bölünmesi altında iki çelişen
  bloğun finalize olmaması — ve ayrı bir ADR konusudur (finality gadget).
* `s`'in epoch parametreleriyle birlikte `config/genesis.toml`'a taşınması.

## Alternatifler (kısa)

| Alternatif | Neden seçilmedi |
|---|---|
| Saf longest-chain (status quo) | Bootstrap'ta seyrek-uzun-zincir saldırısına açık |
| Güvenilir checkpoint'ler | Checkpoint'siz, istemci-tarafı doğrulama hedefiyle çelişir |
| Yalnızca BFT finality gadget | Farklı problemi (mutlak finalite) çözer; bootstrap seçimini çözmez — ayrı ADR |
| Weak subjectivity (sosyal çapa) | Yeni düğüm için güven varsayımı ekler; Genesis bunu gereksiz kılar |

## Doğrulama / Test

* ✅ Referans simülasyonu (`docs/security/maxvalid_bg_reference.py`): dürüst
  yoğun zincire karşı saldırgan seyrek zincir — maxvalid-bg dürüst zinciri
  seçti (364/364 derin-çatal koşusu). Rust birim testleri eklendi.
* Açık: `s` için parametre süpürme (parameter sweep) ve 51/49 en kötü durum
  için güvenlik/canlılık eşik analizi.
* Açık: `proptest` ile rastgele çatal DAG'ları üzerinde özellik testleri.

## Bağlantılı

* `docs/security/qv-consensus-fork-finality-audit.md` — bulgu YÜKSEK-2 (bu ADR'i
  tetikleyen denetim) ve düzeltilen KRİTİK finalite bulguları.
* `crates/qv-consensus/src/chain_state.rs` — kararlı-durum çatal seçimi ve
  yapışkan k-deep finalite (uygulandı).
