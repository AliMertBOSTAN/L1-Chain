# ADR-007: Hibrit X25519 + Kyber Post-Kuantum Handshake (NET-01)

**Durum:** Approved + Uygulandı (2026-05-15)
**Tarih:** 2026-05-15 (yazıldı + impl)
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-net/src/handshake.rs` (protokol + libp2p codec), `crates/qv-net/src/node.rs` (`QvBehaviour.handshake`, `SessionStore` entegrasyonu)

---

## Bağlam

QuantumVault'un mainnet ağı **harvest-now-decrypt-later** sınıfı saldırgana karşı dayanıklı olmak zorunda. libp2p'nin yerleşik güvenli transport'u (TCP + Noise-XX + Yamux) X25519 + ChaCha20-Poly1305 üzerine inşalı; bu klasik şemalar kuantum bilgisayar varlığında forward-secrecy iddiasını kaybeder.

PQC tarafında halihazırda `qv-crypto::hybrid_kem` modülü var — X25519 ECDH ile ML-KEM (Kyber) bir araya getirilip transcript-bound KDF ile birleştiriliyor (`SHA3-256` üzerinden). Modül üretime hazır ve testleri yeşil.

Çözmemiz gereken: bu primitifi her libp2p bağlantısı kurulduğunda otomatik koşan, peer başına saklanabilir bir oturum anahtarı üreten, *kuantum saldırgana* dayanıklı bir uçtan-uca akış haline getirmek.

## Karar

Mevcut Noise-XX katmanını **yerinde** bırakırız ve onun üzerine **uygulama katmanı** bir hibrit handshake protokolü ekleriz:

* Protokol adı: `/quantumvault/handshake/1.0.0`
* Wire: libp2p `request_response::Behaviour<HandshakeCodec>` üzerinden tek roundtrip
* Kripto: `qv-crypto::encapsulate_hybrid` / `decapsulate_hybrid` (ML-KEM-768 = "Level 3")
* Trafik şeması:
  1. Dialer (initiator) → `HandshakeHello { version, initiator_peer_id, hybrid_pk }`
  2. Listener (responder) → `HandshakeAck { version, responder_peer_id, ciphertext, session_binding }`
* `session_binding = SHA3-256(SESSION_BINDING_TAG || shared_secret || initiator_pid_bytes || responder_pid_bytes)`
* İki taraf da `shared_secret`'i türettikten sonra binding'i hesaplar; initiator constant-time karşılaştırır, eşleşmezse bağlantıyı düşürür.
* Sonuç (`shared_secret`, `completed_at`) `SessionStore: Arc<Mutex<HashMap<PeerId, SessionRecord>>>` içine yazılır; bağlantı kapanınca silinir.

### Noise-XX yerine geçmemek niye?

* libp2p 0.54'te Noise pattern'i pluggable değil; KEM yuvası için fork gerekir.
* TLS 1.3 hibrit grup desteği henüz `rustls` mainline'da kararsız.
* Klasik kimlik doğrulama (PeerId ↔ Ed25519 statik anahtar) Noise tarafından zaten yapılıyor; bizim PQC katmanımızın **kimlik doğrulamayı** tekrar yapmasına gerek yok — yalnızca kuantum-güvenli **gizlilik** sağlamak yeterli.
* Uygulama katmanı yaklaşımı Cardano/Mithril hattı ile aynı felsefede: klasik kanal (kimlik) + PQC üst katmanı (gizlilik).

### Kyber seviyesi: Level 3 (ML-KEM-768)

* Public key ≈ 1184 byte, ciphertext ≈ 1088 byte → handshake frame ≤ 8 KiB (`MAX_HANDSHAKE_BYTES`).
* Dilithium L3 ile aynı klasik-eşdeğer güvenlik bandı (~192-bit).
* Wire büyüklüğü TCP MSS'in altında; ekstra paketlere yol açmıyor.

## Sonuçlar

### Olumlu

* Her libp2p bağlantısı **iki güvenlik katmanlı** kurulur: Noise (klasik kimlik) + Hibrit KEM (PQC gizlilik).
* `SessionStore` API'si sayesinde gelecekte encrypted gossip / blok şifreleme bu shared_secret üzerinden anahtarlanabilir.
* `qv-crypto::hybrid_kem` halihazırda audit-hazır; yeni kripto kodu yok.

### Olumsuz

* Bağlantı başına ekstra 1 RTT (handshake mesaj alışverişi).
* Şu sürümde shared_secret henüz application traffic'i şifrelemiyor — sonraki iş.
* KES/VRF ile birlikte üç ayrı kripto katmanı; kod karmaşıklığı artıyor.
* Uygulama katmanı olduğu için Noise sıfır-RTT senaryolarında devre dışı kalabilir (ileride 0-RTT eklenirse handshake gönderilemez; bu noktada uyumluluk değerlendirilmeli).

### Nötr / İleride

* `SessionRecord.completed_at` her epoch sonunda taze handshake için yeniden başlatma stratejisi (forward secrecy refresh) açık iş.
* Hibrit KEM seviyesi `KyberLevel::Level5`'a yükseltilmek istendiğinde sadece `DEFAULT_KEM_LEVEL` sabiti değişir; wire formatı aynı kalır.
* Active MITM (Noise kimlik dahil) için signature-based proof eklemek üst seviye çalışma; bu ADR'in dışı.

## Alternatifler (kısa)

| Alternatif | Neden seçilmedi |
|---|---|
| Custom Noise pattern (`NK1psk`, NoisePQ taslakları) | libp2p-noise pluggable değil; fork bakım yükü |
| TLS 1.3 hibrit (X25519Kyber768Draft00) | rustls hibrit grup desteği henüz upstream değil |
| Yalnızca Kyber (X25519 atla) | Klasik downgrade saldırısı için sigortayı kaybederiz |
| Ring-LWE/RLWE alternatifi (Frodo, NewHope) | NIST PQC finalisti değiller; daha düşük olgunluk |

## Doğrulama / Test

* `qv-net/src/handshake.rs::tests` — 6 unit test:
  * `happy_path_roundtrip_derives_matching_secret`
  * `version_mismatch_rejected`
  * `tampered_binding_rejected`
  * `wrong_initiator_peer_id_breaks_binding`
  * `session_store_basic_lifecycle`
  * `hello_ack_bincode_roundtrip`
  * `stream_protocol_constant_matches_module_const`
* Entegrasyon testi (iki `NetworkNode` arası canlı bağlantı): sonraki iterasyonda — bu sürümde sadece protokol logic + codec doğrulandı.

## Bağlantılı Envanter

* **NET-01** — KAPATILDI (bu ADR ile)
* **NET-02** — Vote variant'ı (handshake'le ilgisiz, ayrı envanter)
* **C-05** — Hybrid KEM seeded keygen (wallet view-key türetimi; bu handshake için gerekli değil — runtime KEM long-lived keypair OS entropy üzerinden üretiyor)
