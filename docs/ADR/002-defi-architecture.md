# ADR-002: UTXO + CSV Üzerinde DeFi Mimarisi Yaklaşımı

**Durum:** Tartışma aşamasında — karar bekleniyor
**Tarih:** 2026-04-15
**Bağlam:** QuantumVault UTXO + Client-Side Validation felsefesini koruyacak,
ancak DeFi (AMM, lending, türev ürünler) primitiflerini desteklemelidir.
Bu iki ilke doğal çelişir: DeFi paylaşılan zincir üstü durum ister, CSV-UTXO
her çıktıyı tek sahipli ve tek harcanan olarak görür.

---

## Kabul Kriterleri

Seçilecek yaklaşım şunları sağlamalı:

1. **UTXO + Client-Side Validation felsefesini koruma** (L1 kör kalır)
2. **Constant-product AMM** (x·y=k) protokolünü ifade edebilme
3. **Lending pool** gibi çok-kullanıcılı durumu temsil edebilme
4. **Eşzamanlılık sorunlarını** çözme (aynı blokta N kullanıcı aynı havuzu)
5. **PQC uyumluluğu** (Bitcoin Script'in ECDSA varsayımlarını kullanamayız)
6. **Makul blok boyutu** (~7.5 MB/1000 tx hedefini bozmamak)

---

## Aday Yaklaşımlar

### Yaklaşım A — Taproot Assets (Lightning Labs modeli)

**Ne yapar:** Varlıklar Bitcoin UTXO'larının içinde Merkle taahhüdü olarak
yaşar. Transfer = commit güncellemesi + off-chain universe'ye proof. L1 sadece
"bir UTXO harcandı" görür, ne transfer edildiğini bilmez.

**DeFi için implikasyon:**
- ✅ L1 tamamen kör kalır (felsefeye mükemmel uyum)
- ✅ Blok boyutu sabit — varlık miktarı ve tipi L1'de yok
- ❌ AMM için **shared state yok** — iki kullanıcı aynı havuzla etkileşemez
- ❌ Atomic swap'tan öteye gidemez (order book bile zor)
- ❌ DeFi kompozisyonu imkansız — her protokol ayrı universe

**Verdict:** DeFi için **yetersiz**. Sadece token transfer + atomic swap.

---

### Yaklaşım B — BitVM (Robin Linus modeli)

**Ne yapar:** İki taraf arasında arbitrary hesaplama. Bir taraf sonucu iddia
eder, diğeri "itiraz" edebilir; itiraz halinde zincir üstünde binary search
ile yanlışı tespit edilir. Optimistic rollup'un Bitcoin üzerindeki analogu.

**DeFi için implikasyon:**
- ✅ Teorik olarak Turing-complete hesaplama yapabiliyor
- ✅ Mutat durumda zincir üstünde sadece commit var (ucuz)
- ❌ **İki taraflı setup** gerekir — genel DeFi (N kullanıcı) için doğrudan uymaz
- ❌ Challenge period (günler) finansal sistemlerde kabul edilemez
- ❌ Karmaşıklık aşırı yüksek — araştırma aşamasında

**Verdict:** Uzun vadede ilginç ama **MVP için uygun değil**. Belki v3+ için
"hyper-scale computation" katmanı olarak düşünülebilir.

---

### Yaklaşım C — Cardano eUTXO + Plutus (Ölçülü Öneri)

**Ne yapar:** UTXO modelini iki eksen üzerinde genişletir:
- **Datum**: UTXO'ya bağlı ek veri (havuzun x,y rezervleri)
- **Validator script**: UTXO'nun hangi koşullarda harcanabileceğini belirler
- **Redeemer**: harcama anında sunulan input

Script, harcama tx'inin tamamını görebilir (introspection) — böylece yeni
çıktının invariant'ı koruduğunu dayatabilir.

**Shared UTXO Pattern (Cardano DeFi'de üretimde):**
- Havuz = tek UTXO, datum = (x_rezerv, y_rezerv, fee_bps)
- Her swap:
  1. Havuz UTXO'yu tüket
  2. Yeni havuz UTXO'yu üret: datum' = (x', y') invariant korunmuş
  3. Kullanıcı çıktısı

**Eşzamanlılık çözümü (Cardano'da):**
- **Batcher/aggregator** mimarisi: kullanıcılar tx yerine "order UTXO"
  yazar, bir batcher bu order'ları toplu olarak havuza uygular.
- Batcher rolü merkeziyetsizleştirilebilir (stake pool operator gibi).

**DeFi için implikasyon:**
- ✅ Üretimde çalışan kanıtlı model (Minswap, SundaeSwap, WingRiders)
- ✅ UTXO + CSV felsefesine doğal uyum
- ✅ L1 hala "sadece script doğrulama" yapıyor (kör)
- ✅ Lending için Merkle-tree datum genişletmesi mümkün
- ⚠️ Batcher rolü yeni merkeziyetsizlik sorusu (çözülebilir)
- ⚠️ Plutus script boyutu büyük olabilir, optimizasyon gerekir

**Verdict:** **En güçlü aday.** DeFi için üretimde kanıtlı, felsefeyle uyumlu.

---

### Yaklaşım D — Chia CoinSet + Conditions (İlginç Alternatif)

**Ne yapar:** Chia her tx'i "condition" listesi olarak modelller: AGG_SIG_ME,
CREATE_COIN, RESERVE_FEE, ASSERT_MY_PARENT_ID vb. Script dili (Chialisp)
pure fonksiyonel LISP dialect'i.

**DeFi için implikasyon:**
- ✅ Covenant'lar first-class (Bitcoin'deki gibi ek uzantı değil)
- ✅ Introspection yerleşik
- ❌ Chialisp öğrenme eğrisi yüksek
- ❌ DeFi primitif ekosistemi zayıf (Cardano'ya kıyasla)
- ❌ Batcher/aggregator pattern henüz olgun değil

**Verdict:** Cardano eUTXO'nun daha hardcore versiyonu. Mühendislik olarak
zarif ama ekosistem desteği zayıf.

---

## Karşılaştırma Özeti

| Kriter | A (Taproot Assets) | B (BitVM) | C (Cardano eUTXO) | D (Chia) |
|---|---|---|---|---|
| L1 kör kalır | ✅ Tam | ⚠️ Kısmen | ✅ Tam | ✅ Tam |
| AMM mümkün | ❌ | ⚠️ Teorik | ✅ Üretim | ✅ Mümkün |
| Lending mümkün | ❌ | ⚠️ Teorik | ✅ | ✅ |
| Eşzamanlılık | — | ❌ | ✅ Batcher | ⚠️ |
| Üretim kanıtı | ✅ | ❌ | ✅ | ⚠️ |
| Blok şişmesi | Yok | Düşük | Orta | Orta |
| MVP uygunluğu | ❌ | ❌ | ✅ | ⚠️ |
| Felsefe uyumu | ✅✅ | ⚠️ | ✅ | ✅ |

---

## Öneri

**Cardano eUTXO modeli (Yaklaşım C) temelli + QuantumVault'a özel uyarlamalar.**

### Uyarlamalar

1. **PQC imzalar**: Plutus ECDSA varsayar; biz Dilithium + Hybrid KEM kullanırız.
   Script dilinde `OP_CHECKSIG_PQC` primitifi olacak.

2. **Batcher merkezi olmasın**: Batcher rolünü **slot lider** (PoS) ile birleştir.
   Her slot'ta lider o sloğun batch'ini oluşturur. Böylece batcher'lık ayrı bir
   güç merkezi olmaz.

3. **Encrypted mempool entegrasyonu**: MEV kararımız (threshold decryption)
   gereği, order UTXO'lar şifrelenmiş havuzda bekler; slot lider (+komite)
   çözer ve batch'i oluşturur. MEV kökten çözülür.

4. **Lending için Merkle datum**: Borç verici pozisyonları datum içinde
   Merkle kök. Yeni pozisyon ekleme = STARK ispatıyla yeni kök.

5. **Script dili**: Plutus yerine kendi DSL'imizi Haskell yerine Rust-friendly
   syntax ile tasarla. Declarative, stack-based, introspection-first.

---

## Kabul Gerekirse Sonraki Adımlar

1. ADR-003 (MEV / encrypted mempool) yaz — threshold PQC decryption seçimi
2. Script VM için detaylı spec (opcode listesi, gas modeli)
3. "Shared UTXO AMM" için referans implementasyon — test amaçlı
4. Batcher/slot-leader rolü konsensüs bölümünde formalize et

---

## Reddedilen Alternatifler

- **Hesap tabanlı (Ethereum/Solana)**: CSV felsefesinden vazgeçmek demek, ret.
- **L1 UTXO + L2 DeFi rollup**: Mühendislik yükü 2x, faydası marjinal —
  eUTXO zaten DeFi'yi L1'e ölçeklenebilir şekilde getirebiliyor.
