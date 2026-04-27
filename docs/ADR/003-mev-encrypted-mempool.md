# ADR-003: MEV Stratejisi — Encrypted Mempool + Threshold Decryption

**Durum:** Tartışma aşamasında
**Tarih:** 2026-04-15
**Bağlam:** DeFi protokollerinin en büyük değer çıkışı Maximal Extractable
Value (MEV). Kullanıcı ne kadar "doğru" işlem yaparsa yapsın, ara katmanlar
(validatör, builder, searcher) işlem sırasını manipüle ederek ekstra kâr
çıkarır. Mevcut çözümler (Flashbots) sadece kurumsallaştırır, ortadan kaldırmaz.

QuantumVault kararı: **Encrypted mempool + threshold decryption**. İşlemler
mempool'a şifrelenmiş girer, slot lideri + komite blokta çözer ve işler.
Validator hiçbir işlemi blokta yer almadan önce okuyamaz → ne front-run
edebilir ne back-run, ne sandwich.

---

## Gereksinimler

1. İşlemler mempool'da **şifreli**, kimse tek başına çözemez
2. Slot lideri blok önerdiğinde **komite eşiği (t-of-n)** ile çözülür
3. Çözme süreci **deterministik** olmalı (hangi tx'in alındığı sonradan doğrulanabilir)
4. Şifreleme şeması **PQC-güvenli** olmalı
5. Komite rotasyonu PoS epoch'larıyla uyumlu
6. Gecikme kabul edilebilir (<1 slot = <2 saniye)

---

## Tasarım Seçenekleri

### Seçenek 1 — Ferveo (Anoma projesi) benzeri: Threshold ElGamal on BLS12-381

**Nasıl çalışır:**
- Validator komitesi BLS DKG ile eşik anahtar üretir
- Kullanıcı tx'i komite açık anahtarına threshold ElGamal ile şifreler
- Slot lideri blokta tx'leri toplar, komite t imzayla çözme payını yayınlar
- Çözme birleştirilir, tx'ler açılır, yürütülür

**Sorun:** BLS12-381 pairing tabanlı, **Shor ile kırılabilir**. PQC değil.

**Verdict:** PQC felsefesine aykırı, reddedildi.

---

### Seçenek 2 — Lattice-based Threshold Encryption (LWE/RLWE)

**Nasıl çalışır:**
- Komite RLWE (Ring-LWE) bazlı DKG yürütür
- Şifreleme: BFV/CKKS benzeri lattice şeması
- Threshold decryption: her komite üyesi noise + payını yayınlar
- Toplam noise budget'ı yönetmek zor (ama çözümlenmiş)

**Aday kütüphaneler:** `openfhe-rust`, `lattigo` (Go, port edilecek)

**Olgunluk:** Akademik kanıtlı (Cryptographic Group Actions, LWE), henüz
blockchain'de production yok.

**Verdict:** PQC uyumlu, **ana aday**. Mühendislik riski yüksek.

---

### Seçenek 3 — Kyber Tabanlı Distributed KEM + Hash Commitments

**Nasıl çalışır:**
- Her slot için komite "one-shot" Kyber anahtarı üretir (DKG ile shard'lanmış)
- Kullanıcı tx'i bu ephemeral anahtara Kyber ile şifreler
- Slot sonunda t-of-n komite üyesi shard'larını yayınlar → Kyber secret key
  yeniden kurulur → tüm tx'ler çözülür
- Her slot anahtarı tek kullanımlık — bir kez çözülünce artık değer taşımaz

**Avantajlar:**
- Zaten kullandığımız Kyber/ML-KEM'e yaslanır, yeni kriptografik varsayım yok
- Çözme = standart Kyber decaps × t
- Shamir secret sharing ile shard'lanır

**Dezavantajlar:**
- Kyber KEM'in threshold DKG'si yeni — literatürde var ama production'da yok
- Her slot DKG pahalı olabilir (2sn slot'ta zor)

**Verdict:** **Aday**. Daha az saldırı yüzeyi.

---

### Seçenek 4 — Time-Lock Puzzle (VDF-based)

**Nasıl çalışır:**
- Kullanıcı tx'i, ~1 slot süresinde çözülebilecek time-lock puzzle ile şifreler
- Puzzle çözümü iteratif, paralelleştirilemez (VDF)
- Blok üretildikten sonra herkes puzzle'ı çözebilir

**Avantajlar:**
- Komite/DKG gerekmez, tamamen decentralize
- Threshold cryptography karmaşıklığı yok

**Dezavantajlar:**
- PQC-güvenli VDF aktif araştırma alanı (henüz üretim seviyesinde yok)
- Puzzle'ı erken çözme saldırısı (büyük hesap gücüyle) riski
- Ekstra CPU yükü

**Verdict:** Uzun vadede ilginç, şimdilik **ikincil**.

---

## Öneri

**Seçenek 3 (Kyber Distributed KEM) — ana yaklaşım.**
**Seçenek 2 (Lattice Threshold) — Seçenek 3 çalışmazsa fallback.**

### Gerekçe

1. Zaten Kyber KEM kullanıyoruz — yeni kriptografik primitif eklemiyoruz
2. Threshold Kyber üzerinde akademik çalışma var (Gentry ve ark.), tatmin edici
3. DKG her slot değil, her **epoch başında** (12 saat) bir kez — amortise edilir
4. Komite = slot lider + rastgele seçilmiş N-1 validatör
5. Komite ihanet ederse (t üye colluded): o slot'un tx'leri erken görülür ama
   mevcut slot'a eklenir, bir sonraki slota taşınamaz → MEV fırsatı minimal

---

## Eşzamanlılık ve Batcher Rolü İle Entegrasyon

ADR-002'deki "Shared UTXO Batcher" rolünü encrypted mempool üstüne oturtuyoruz:

1. Kullanıcılar "order UTXO" tx'lerini **şifrelenmiş** olarak gossip eder
2. Slot lider + komite çözdüğünde tüm order'lar görünür
3. Slot lider bu order'ları **deterministik sıralama** (time-based veya lexicographic)
   ile batch'ler ve havuz UTXO'sunu güncelleyen tx'i üretir
4. Determinizm → validatör manipülasyon yapamaz
5. Her batch işlemi doğrulanabilir — yanlış sıralama = slashing

---

## Açık Sorular

1. **Komite boyutu** n ve eşik t ne olmalı? (Örn: n=64, t=43 = 2/3+1)
2. **Komite rotasyonu** her epoch mu, yoksa daha sık mı?
3. **Slashing koşulları**: komite üyesi çözme payını yayınlamazsa ne olur?
4. **Liveness**: Kötü niyetli komite t+1 kişiyle blokluyorsa? (Graceful
   degradation — encrypted mempool düşer, fallback clear mempool)

---

## Reddedilen Alternatifler

- **Flashbots-style PBS**: MEV'i kurumsallaştırır, çözmez. Ret.
- **Ethereum SUAVE**: SGX bazlı, trust model sorunlu. Ret.
- **SGX enclave'ler**: Hardware trust assumption + geçmiş güvenlik sorunları. Ret.
