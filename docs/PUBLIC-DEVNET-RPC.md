# Public Devnet RPC Node Kurulumu

Bu kılavuz QuantumVault L1 testnet'inin **internete açık** bir JSON-RPC
endpoint'ini kuran sunucu operatörlerine yöneliktir. Cüzdan kullanıcıları
(`docs/INSTALL.md`) sadece tüketici.

Hedef topoloji:

```
İnternet
   │
   │ HTTPS :443
   ▼
┌──────────────┐      Unix socket
│    Caddy     │  ───────────────▶  qv-node :8545 (RPC, loopback'te dinler)
│ (TLS + rate  │
│  limit +     │
│  request     │
│  rewrite)    │
└──────────────┘
   │
   │ HTTPS :9100 (opsiyonel)
   ▼
   Prometheus / Grafana (monitoring)
```

`qv-node` doğrudan internete bakmaz; Caddy onun önünde reverse proxy ve
TLS sonlandırıcı olarak durur, ayrıca rate-limit + erişim logu sağlar.

## İçerik

- [Sistem gereksinimi](#sistem-gereksinimi)
- [Binary kurulum](#binary-kurulum)
- [systemd unit](#systemd-unit)
- [Caddy reverse proxy](#caddy-reverse-proxy)
- [Rate limiting](#rate-limiting)
- [Monitoring (Prometheus)](#monitoring--prometheus)
- [Güvenlik sertleşmesi](#guvenlik-sertlesmesi)
- [Yükseltme / restart](#yukseltme--restart)
- [Troubleshooting](#troubleshooting)

## Sistem gereksinimi

| Bileşen | Minimum | Önerilen |
|---|---|---|
| CPU | 2 vCPU | 4 vCPU |
| RAM | 2 GB | 8 GB |
| Disk | 20 GB SSD | 100 GB NVMe |
| OS | Ubuntu 22.04 LTS / Debian 12 | Ubuntu 24.04 LTS |
| Bant genişliği | 50 Mbps | 200 Mbps |
| Açık port (inbound) | 443/TCP (HTTPS), opsiyonel 17001/TCP (P2P) | aynı |

Açık port konfigürasyonu (UFW örneği):

```bash
sudo ufw allow OpenSSH
sudo ufw allow 443/tcp
sudo ufw allow 17001/tcp comment 'qv-node p2p'
sudo ufw enable
```

## Binary kurulum

1. Release sayfasından **Linux x64** arşivini indir:

   ```bash
   ARCHIVE=quantumvault-v0.1.0-linux-x64.tar.gz
   curl -L -o "$ARCHIVE" \
     "https://github.com/anthropics/quantumvault-l1/releases/download/v0.1.0/$ARCHIVE"
   curl -L -o "$ARCHIVE.sha256" \
     "https://github.com/anthropics/quantumvault-l1/releases/download/v0.1.0/$ARCHIVE.sha256"
   echo "$(cat $ARCHIVE.sha256)  $ARCHIVE" | sha256sum -c -
   ```

   > **Not:** İlk binary sürümleri sadece Windows-x64 + macOS-arm64. Linux
   > sunucu için kaynaktan derlemen gerek. Kısa süre içinde Linux-x64
   > target eklenecek.

2. Sistem dizinine yerleştir:

   ```bash
   sudo mkdir -p /opt/quantumvault/bin
   sudo tar -xzf "$ARCHIVE" -C /opt/quantumvault --strip-components=0
   sudo install -m 0755 /opt/quantumvault/bin/qv-node /usr/local/bin/qv-node
   sudo install -m 0755 /opt/quantumvault/bin/qv-wallet /usr/local/bin/qv-wallet
   qv-node --version
   ```

3. Servis kullanıcısı oluştur:

   ```bash
   sudo useradd --system --no-create-home --shell /usr/sbin/nologin qvnode
   sudo mkdir -p /var/lib/quantumvault /etc/quantumvault /var/log/quantumvault
   sudo chown -R qvnode:qvnode /var/lib/quantumvault /var/log/quantumvault
   sudo chmod 0750 /var/lib/quantumvault /var/log/quantumvault
   ```

4. Genesis state'i initialize et:

   ```bash
   sudo -u qvnode qv-node --init \
     --network devnet \
     --data-dir /var/lib/quantumvault/data
   ```

## systemd unit

`/etc/systemd/system/qv-node.service`:

```ini
[Unit]
Description=QuantumVault L1 full node
Documentation=https://github.com/anthropics/quantumvault-l1
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=qvnode
Group=qvnode
WorkingDirectory=/var/lib/quantumvault
ExecStart=/usr/local/bin/qv-node \
  --network devnet \
  --data-dir /var/lib/quantumvault/data \
  --rpc-addr 127.0.0.1:8545 \
  --metrics-addr 127.0.0.1:9601 \
  --listen-addr /ip4/0.0.0.0/tcp/17001 \
  --log-level info
Restart=always
RestartSec=5s

# Logging
StandardOutput=append:/var/log/quantumvault/qv-node.log
StandardError=append:/var/log/quantumvault/qv-node.err

# Sertleştirme
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
LockPersonality=true
MemoryDenyWriteExecute=true
ReadWritePaths=/var/lib/quantumvault /var/log/quantumvault
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

Etkinleştir + başlat:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now qv-node
sudo systemctl status qv-node
journalctl -u qv-node -f
```

`qv-node` artık `127.0.0.1:8545`'te RPC dinliyor — internete açık değil.

Log rotation:

`/etc/logrotate.d/quantumvault`:

```
/var/log/quantumvault/*.log
/var/log/quantumvault/*.err
{
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    create 0640 qvnode qvnode
    postrotate
        systemctl reload qv-node 2>/dev/null || true
    endscript
}
```

## Caddy reverse proxy

Caddy kullanıyoruz çünkü Let's Encrypt sertifikalarını otomatik alır,
yeniler ve modern HTTPS varsayılanları (HTTP/2, HTTP/3, OCSP stapling)
gelir. Nginx ile de yapılabilir ama Caddy konfigürasyonu çok daha kısa.

### Caddy kur

Debian/Ubuntu:

```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
  | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
  | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update
sudo apt install -y caddy
```

### Caddyfile

`/etc/caddy/Caddyfile`:

```caddy
{
    # Otomatik Let's Encrypt — yönetici e-postası ile.
    email rpc-ops@quantumvault.example
    # Aşırı yük altında ACME challenge limitlerini boş hatırla.
    storage file_system /var/lib/caddy
    # Production: log JSON formatında, prometheus için yapılandırılmış.
    log {
        output file /var/log/caddy/access.log {
            roll_size 100mb
            roll_keep 14
        }
        format json
    }
}

rpc.testnet.quantumvault.example {
    # ---- TLS ----
    tls {
        protocols tls1.3
    }

    # ---- Yalnız POST /rpc (JSON-RPC) ----
    @rpc {
        method POST
        path /rpc /rpc/* /
        header Content-Type application/json*
    }

    # ---- Rate limit ----
    # 1 dakika içinde IP başına 60 istek. Daha sıkı kademeler altta.
    rate_limit @rpc {
        zone rpc_per_ip {
            key {remote_host}
            events 60
            window 1m
        }
    }

    # ---- Reverse proxy ----
    handle @rpc {
        # qv-node'a yönlendir; gerçek istemci IP'sini X-Forwarded-For ile geç.
        reverse_proxy 127.0.0.1:8545 {
            header_up X-Forwarded-For {remote_host}
            header_up X-Forwarded-Proto {scheme}
            transport http {
                read_timeout 30s
                write_timeout 30s
            }
        }
    }

    # ---- Default 404 ----
    handle {
        respond "QuantumVault L1 RPC. POST JSON-RPC to /rpc." 404
    }

    # ---- Erişim log ----
    log {
        output file /var/log/caddy/rpc-access.log {
            roll_size 100mb
            roll_keep 14
        }
        format json
    }
}

# ---- Status sayfası (sağlık kontrolü için public) ----
status.testnet.quantumvault.example {
    handle /healthz {
        # Lokal node'a basit qv_getTip sorgu — başarılıysa 200.
        reverse_proxy 127.0.0.1:8545 {
            transport http {
                read_timeout 5s
            }
            # Caddy basit bir GET → JSON-RPC POST mapping yapamaz; bunun yerine
            # bir cron script'i veya küçük bir wrapper kullan. Detay altta.
        }
    }
    handle {
        respond "QuantumVault L1 testnet status page" 200
    }
}
```

> **Rate-limit modülü.** Caddy'nin core build'inde rate-limit yok;
> [`caddy-ratelimit`](https://github.com/mholt/caddy-ratelimit) modülünü
> ekleyerek build'liyoruz. `xcaddy` ile kolay:
>
> ```bash
> sudo apt install -y golang-go
> go install github.com/caddyserver/xcaddy/cmd/xcaddy@latest
> ~/go/bin/xcaddy build --with github.com/mholt/caddy-ratelimit
> sudo install -m 0755 caddy /usr/bin/caddy
> sudo systemctl restart caddy
> ```

DNS A kaydını `rpc.testnet.quantumvault.example` → sunucu IP'sine çevir
(önce ki, Let's Encrypt HTTP-01 challenge'ı çalışsın), sonra:

```bash
sudo systemctl reload caddy
sudo journalctl -u caddy -f
```

İlk başlatmada Caddy otomatik olarak Let's Encrypt'ten sertifika alır ve
60 günde bir kendiliğinden yeniler.

## Rate limiting

Caddy katmanı IP başına basit bir koruma. Daha sofistike bir limit için
iki kademe öneririz:

### Caddyfile (basic)

Yukarıdaki örnek IP başına dakikada 60 istek.

### fail2ban (anti-abuse)

Sürekli rate-limit yiyen IP'leri 24 saatlik IP-ban'a koy:

`/etc/fail2ban/jail.d/qv-rpc.conf`:

```ini
[qv-rpc]
enabled  = true
port     = 443
filter   = qv-rpc
logpath  = /var/log/caddy/rpc-access.log
maxretry = 100
findtime = 600
bantime  = 86400
backend  = polling
```

`/etc/fail2ban/filter.d/qv-rpc.conf`:

```ini
[Definition]
# Caddy JSON log'undan 429 yiyen IP'leri yakala.
failregex = .*"status":429,.*"remote_ip":"<HOST>".*
ignoreregex =
datepattern = %%Y-%%m-%%dT%%H:%%M:%%S
```

```bash
sudo systemctl restart fail2ban
sudo fail2ban-client status qv-rpc
```

### Application-level limit (qv-wallet faucet)

Public faucet kötüye kullanılırsa: `qv-wallet`'in faucet endpoint'i şu
an kullanıcı başına / IP başına rate-limit içermiyor. Üretim'de
kapatılması veya cüzdan tarafına ekleme yapılması önerilir. ROADMAP B
grubu (sonraki seans) — `Faucet rate limit`.

## Monitoring — Prometheus

`qv-node` Prometheus metrik endpoint'i `:9601`'de açar. Bu portu
**internete açma** — sadece monitoring sunucusu üzerinden eriş.

### node_exporter + qv-node scrape

`/etc/prometheus/prometheus.yml`:

```yaml
scrape_configs:
  - job_name: qv-node
    scrape_interval: 15s
    static_configs:
      - targets: ["127.0.0.1:9601"]
  - job_name: node_exporter
    static_configs:
      - targets: ["127.0.0.1:9100"]
```

Grafana dashboard için topluluk-paylaşımlı bir QuantumVault dashboard'u
yok (henüz). Temel metrikler:

| Metric | Anlamı | Alert eşiği |
|---|---|---|
| `qv_tip_height` | Cluster tip yüksekliği | dakikada artmıyor ⇒ uyarı |
| `qv_blocks_validated_total` | Bu node'un doğruladığı blok sayısı | — |
| `qv_blocks_rejected_total` | Reddedilen | Sıçrama ⇒ uyarı |
| `qv_peers_connected` | Bağlı peer sayısı | < 3 ⇒ uyarı |
| `qv_mempool_size` | Bekleyen tx sayısı | > 5000 ⇒ uyarı |
| `qv_block_validate_seconds` | Doğrulama latency histogramı | p99 > 500ms ⇒ uyarı |

## Güvenlik sertleşmesi

1. **SSH key-only.** `PasswordAuthentication no` `/etc/ssh/sshd_config`.
2. **Firewall.** Yukarıda UFW örneği.
3. **Otomatik güvenlik güncellemeleri.** `unattended-upgrades` paketi.
4. **systemd sandboxing.** Yukarıdaki unit'te uygulandı.
5. **Lokal RPC.** `qv-node --rpc-addr 127.0.0.1:8545` — sadece Caddy
   üzerinden erişilir.
6. **Validator anahtarları.** Bu kılavuz **read-only RPC node** içindir;
   stake pool operatörü değildir. Validator olmak için ayrı bir
   sertleştirme katmanı gerekir (KES anahtarları + cold storage).
7. **Backup.** `/var/lib/quantumvault/data` — periyodik snapshot al
   (önerilen: `restic` ile şifreli S3-uyumlu storage'a günlük).

## Yükseltme / restart

Yeni release çıktığında:

```bash
# Servisi durdur
sudo systemctl stop qv-node

# Yeni binary'yi indir + checksum doğrula
ARCHIVE=quantumvault-v0.2.0-linux-x64.tar.gz
curl -L -o "$ARCHIVE" "https://github.com/anthropics/quantumvault-l1/releases/download/v0.2.0/$ARCHIVE"
sha256sum -c "$ARCHIVE.sha256"

# Eski binary'yi sakla
sudo mv /usr/local/bin/qv-node /usr/local/bin/qv-node.old

# Yenisini koy
sudo tar -xzf "$ARCHIVE"
sudo install -m 0755 bin/qv-node /usr/local/bin/qv-node

# Servisi başlat
sudo systemctl start qv-node
journalctl -u qv-node -f
```

Sürüm uyumsuzluğu varsa (örneğin breaking RPC değişikliği)
`/var/lib/quantumvault/data` ile yeni binary konuşamayabilir. Release
notes'a bak; bazı durumlarda `--migrate` flag'i gerekecek (henüz
implement edilmedi; v1.0 öncesi devnet'te genelde state reset).

## Troubleshooting

### Caddy sertifika alamıyor

```
ERR ts=... msg=challenge failed
```

- DNS A kaydının doğru ve TTL'sinin düşük olduğundan emin ol.
- Port 80 açık ve Caddy ona erişebiliyor mu (HTTP-01 challenge için
  gerekli)? `sudo ss -tlnp | grep :80`
- Let's Encrypt rate-limit'e takıldıysan birkaç saat bekle. `staging`
  endpoint'iyle önce test et: Caddyfile'ın en üstüne `acme_ca https://acme-staging-v02.api.letsencrypt.org/directory`.

### qv-node CPU %100, blok validation yavaş

- Sunucu CPU'su yetersiz olabilir (2 vCPU ile minimum). 4+ vCPU'ya çık.
- RocksDB tunable'ları `--rocksdb-write-buffer-size` vb. ile düşür.
- Disk IOPS sınırı (özellikle network attached storage'da) — `iostat -x 1` ile gözlemle.

### Cüzdan kullanıcıları "connection refused" diyor

- Caddy çalışıyor mu? `systemctl status caddy`
- DNS doğru mu? `dig rpc.testnet.quantumvault.example`
- `qv-node` lokal RPC'yi açtı mı? `curl -X POST http://127.0.0.1:8545 -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'`

### Çok fazla 429 (rate-limit yedik)

- Caddyfile'da `events` artır.
- Belirli IP'ler kötü niyetliyse fail2ban kuralları aktive edildi mi?

## Detay

- Genel mimari: [`SYSTEM_OVERVIEW.md`](SYSTEM_OVERVIEW.md)
- ADR'ler: [`docs/ADR/`](ADR/)
- Validator kılavuzu: [`VALIDATOR_GUIDE.md`](VALIDATOR_GUIDE.md)

## Geri bildirim

Bu kılavuz daha çok production-ready hale getirilecek. Önerilerini
issue olarak aç:

<https://github.com/anthropics/quantumvault-l1/issues>
