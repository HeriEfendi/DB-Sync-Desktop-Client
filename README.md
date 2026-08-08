# 🗄️ DB-Sync Desktop Client

![Version](https://img.shields.io/badge/version-0.8.2-emerald)
![Tauri](https://img.shields.io/badge/Tauri-v2-blue?logo=tauri)
![Vue](https://img.shields.io/badge/Vue.js-3.5-brightgreen?logo=vuedotjs)
![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![License](https://img.shields.io/badge/license-MIT-blue)

**DB-Sync Desktop Client** adalah aplikasi desktop modern dan efisien berbasis **Tauri v2 (Rust)** dan **Vue 3** yang dirancang untuk melakukan **sinkronisasi data incremental & bulk upsert** dari server web **PhpMyAdmin (PMA) Remote** langsung ke **Database MySQL / MariaDB Lokal** (Port 3306).

Aplikasi ini mempermudah pengembang dan *database administrator* dalam menyinkronkan data transaksi, master data, atau log dari server produksi/staging ke lingkungan pengujian/pengembangan lokal secara real-time, otomatis, atau terjadwal tanpa perlu melakukan export-import database manual secara berulang.

---

## ✨ Fitur Utama

- 🔄 **Incremental & Fresh Sync Modes**:
  - **Incremental Sync**: Menarik baris data baru atau yang diperbarui secara bertahap berdasarkan *Primary Key* (`WHERE primary_key > last_local_id`) atau timestamp `updated_at`.
  - **Fresh Sync**: Sinkronisasi ulang data dari awal dengan opsi reset state atau pembersihan data lokal.
- ⚡ **Native Rust Bulk Upsert Engine (`sqlx` & Tokio)**: Eksekusi pengolahan batch data berkecepatan tinggi ke database lokal yang ditangani langsung oleh *native engine* Rust menggunakan query `ON DUPLICATE KEY UPDATE` dan *connection pool* `sqlx`.
- 📡 **Native PMA Exporter & HTTP Client (`pma_export.rs`)**: Penanganan HTTP request, manajemen cookie sesi, ekstraksi CSRF token, gzip decompression streaming, dan otomatisasi ekstraksi tabel remote langsung melalui layer backend Rust.
- 📋 **Multi-Table & Column Configuration**: Mendukung pemilihan beberapa tabel remote sekaligus, pemetaan kolom, dan penyesuaian nama *Primary Key* per tabel.
- ⏱️ **Auto-Sync Scheduler**: Penjadwalan otomatis dengan pilihan interval fleksibel (Non-Aktif/Manual, 5 Detik, 10 Detik, 30 Detik, 1 Menit, 5 Menit).
- 🛑 **Graceful Sync Stop & Live Progress**:
  - Tombol **Hentikan Sinkronisasi** yang aman (*graceful stop*) tanpa merusak data yang sedang dimasukkan.
  - **Live Banner & Progress Bar**: Visualisasi status tabel aktif, jumlah baris dimasukkan, dan total baris ter-sync per sesi.
- 💾 **Per-Table Sync State Tracking**: Pencatatan otomatis `lastSyncedId` dan `lastSyncTime` per tabel. Dilengkapi kontrol untuk **Reset State per Tabel** maupun **Reset Semua State**.
- 🧪 **Dual Connection Tester**: Fitur pengujian independen untuk memvalidasi akses koneksi ke **Remote PhpMyAdmin** (HTTP/HTTPS) dan **MySQL Lokal** (Port 3306).
- 📊 **Data Inspector & Local Table Truncate**: Pratinjau sampel data hasil sinkronisasi lokal secara interaktif dan modal konfirmasi pembersihan (*Truncate*) tabel lokal.
- 🖥️ **Live Log Console**: Console monitor aktivitas sinkronisasi real-time dengan tingkatan badge (Success, Info, Warning, Error) serta opsi pembersihan log (*Clear Console*).
- 🛡️ **SQL Injection Protection & Sanitization**: Sanitasi *SQL Identifiers* (nama tabel dan kolom) dilakukan di layer Rust untuk menjamin keamanan query.
- 📦 **Automated Multi-Format Packaging**: Skrip otomatisasi rilis untuk paket rilis Linux (`.deb`, `.rpm`, `.pkg.tar.zst` Arch Linux) dan Windows (`.msi`, `.exe`).

---

## 🏗️ Arsitektur & Alur Kerja Sinkronisasi

```
  +-----------------------+              +-----------------------------+
  |  PhpMyAdmin Remote    |              | DB-Sync Desktop Client UI   |
  |  (Web / HTTP Server)  |              | Vue 3 + Lucide Icons        |
  +-----------+-----------+              +--------------+--------------+
              ^                                         |
              | HTTP / Gzip Stream                      | IPC Invocation
              v                                         v
  +-----------+-----------+              +--------------+--------------+
  | Rust PMA Exporter     | ------------>| Sync Engine Manager         |
  | (CSRF, Cookie, Stream)|              | (Auto-Sync & State Store)   |
  +-----------------------+              +--------------+--------------+
                                                        |
                                                        | Native IPC
                                                        v
                                         +--------------+--------------+
                                         | Rust Backend Engine         |
                                         | (Tauri v2 + SQLx Tokio)     |
                                         +--------------+--------------+
                                                        |
                                                        | Native TCP Connection
                                                        v
                                         +--------------+--------------+
                                         | Local MySQL / MariaDB       |
                                         | (127.0.0.1:3306)            |
                                         +-----------------------------+
```

### Langkah Kerja Sync Engine:
1. **Check Local State**: Engine mengambil nilai *Primary Key* maksimum (`MAX(primary_key)`) atau state terakhir (`lastSyncedId`) dari tabel MySQL lokal via Rust `sqlx`.
2. **Authenticate & Stream PMA**: Inisialisasi sesi HTTP ke PhpMyAdmin remote, ekstraksi cookie sesi dan CSRF token, kemudian membaca daftar tabel atau mengekspor data.
3. **Fetch Incremental Rows**: Mengeksekusi query incremental (`SELECT * FROM table WHERE pk > last_id ORDER BY pk ASC LIMIT N`) dari PMA remote.
4. **Native Bulk Upsert**: Batch data dikirim ke backend Rust untuk disisipkan/diperbarui (*Upsert*) ke MySQL lokal menggunakan `ON DUPLICATE KEY UPDATE`.
5. **Update State**: State `lastSyncedId` dan `lastSyncTime` disimpan ke `LocalStorage` per tabel.

---

## 🛠️ Teknologi yang Digunakan

| Layer | Teknologi / Library | Deskripsi |
| :--- | :--- | :--- |
| **Frontend UI** | **Vue 3** (Composition API `<script setup>`) | Framework UI reaktif |
| **Build Tool** | **Vite v6** | Dev server & bundler cepat |
| **Iconography** | **Lucide Vue Next** | Icon set modern & konsisten |
| **Styling** | **Custom CSS Design System** | Dark mode theme, glassmorphism, responsive grid |
| **Desktop Framework**| **Tauri v2** | Framework aplikasi desktop lintas platform |
| **Backend Engine** | **Rust (Edition 2021)** | Logic native berkinerja tinggi |
| **Database Driver** | **sqlx 0.8** (Tokio Async, Native TLS, MySQL) | Driver MySQL/MariaDB async di Rust |
| **HTTP & Export** | **reqwest / flate2 / urlencoding** | Native HTTP streaming & gzip decompressor di Rust |
| **Serialization** | **serde / serde_json** | Serialisasi & deserialisasi JSON cepat |

---

## 📋 Prasyarat Sistem

Sebelum menjalankan atau membangun aplikasi ini, pastikan sistem Anda memenuhi kebutuhan berikut:

1. **Node.js**: v18.0.0 atau lebih baru ([Download Node.js](https://nodejs.org/))
2. **Rust & Cargo**: Toolchain Rust versi stabil terbaru ([Install Rust](https://www.rust-lang.org/tools/install))
3. **Database Server Lokal**: MySQL Server atau MariaDB Server berjalan di port `3306` (atau port kustom Anda).
4. **PhpMyAdmin Remote**: Akses web ke PhpMyAdmin yang dapat dijangkau via jaringan HTTP/HTTPS.

---

## 🚀 Panduan Instalasi & Penggunaan

### 1. Clone Repository & Install Dependency

```bash
# Clone repository
git clone https://github.com/HeriEfendi/DB-Sync-Desktop-Client.git
cd DB-Sync-Desktop-Client

# Install dependency Node.js
npm install
```

### 2. Jalankan Mode Pengembangan (Development)

Untuk menjalankan aplikasi desktop berbasis Tauri (direkomendasikan):

```bash
npm run tauri dev
```

> **Catatan Mode Browser:**
> Jika Anda menjalankan `npm run dev`, aplikasi akan terbuka di Web Browser. Namun fitur koneksi native port 3306 MySQL lokal hanya dapat diakses saat dijalankan menggunakan perintah `npm run tauri dev`.

---

## 🛠️ Build & Rilis App

### Build Manual Lokal

Untuk melakukan build installer aplikasi secara lokal:

```bash
# Build binary & bundle paket bawaan Tauri
npm run tauri build
```

Hasil build lokal tersimpan di direktori: `src-tauri/target/release/bundle/`.

#### Build Khusus Arch Linux (`.pkg.tar.zst`)
```bash
# Menggunakan PKGBUILD bawaan
npm run build:pacman
```

### Rilis Otomatis (Windows & Linux via GitHub Actions)

Proyek ini telah dilengkapi skrip rilis otomatis yang menyinkronkan versi di `package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, dan `PKGBUILD`, lalu melakukan tag & push ke Git untuk memicu GitHub Actions:

```bash
# Contoh merilis versi 0.8.2
npm run release 0.8.2
```

Workflow [.github/workflows/release.yml](file:///home/lenovo/www/DB-Sync-Desktop-Client/.github/workflows/release.yml) akan otomatis:
- **Build Linux**: Membuat paket `.deb`, `.rpm`, dan `.pkg.tar.zst` (Arch Linux).
- **Build Windows**: Membuat installer `.msi` dan `.exe` (NSIS).
- **GitHub Release**: Membuat entri Release baru dan melampirkan seluruh installer secara otomatis.

---

## 📁 Struktur Proyek

```
DB-Sync-Desktop-Client/
├── .github/
│   └── workflows/
│       └── release.yml          # GitHub Actions CI/CD Workflow
├── scripts/
│   ├── create-arch-pkg.mjs      # Builder skrip paket Arch Linux (.pkg.tar.zst)
│   └── release.mjs              # Skrip otomasi rilis versi & Git tagging
├── src/                         # Frontend Layer (Vue 3)
│   ├── components/              # Komponen UI Modular
│   │   ├── ConnectionConfig.vue # Form Master Konfigurasi Remote & Local Database
│   │   ├── ConnectionConfigSection.vue # Komponen Sub-section Input Koneksi
│   │   ├── TableConfig.vue      # Pilihan Tabel Remote & Konfigurasi Kolom
│   │   ├── SyncControl.vue      # Panel Kontrol Sync, Progress, & State Table History
│   │   ├── LogConsole.vue       # Console Monitor Log Activity Real-Time
│   │   ├── DataPreview.vue      # Data Inspector & Table Preview / Truncate Modal
│   │   └── Navbar.vue           # Header Navigation & Indicator Status Badges
│   ├── services/                # Business Logic Services
│   │   ├── pmaClient.js         # HTTP Client untuk Authenticate & Query Remote PMA
│   │   ├── syncEngine.js        # Engine Koordinasi Incremental Sync & Scheduling
│   │   ├── syncStateStore.js    # Per-Table Sync State Persistence (LocalStorage)
│   │   └── tauriHelper.js       # Helper Bridge IPC (Tauri Invoke & Native Call)
│   ├── styles/                  # Styling Tokens & Custom Global CSS
│   ├── App.vue                  # Main Layout & State Aggregator
│   └── main.js                  # Vue Entry Point
├── src-tauri/                   # Backend Layer (Rust Engine)
│   ├── src/
│   │   ├── commands.rs          # Tauri IPC Commands (MySQL Pool, Bulk Upsert, Preview)
│   │   ├── pma_export.rs        # Native Rust Exporter (Gzip Stream & PMA Auth)
│   │   ├── lib.rs               # Registrasi Command Handler & Tauri Plugin
│   │   └── main.rs              # Entrypoint Binary Tauri
│   ├── Cargo.toml               # Dependensi & Manifest Crate Rust
│   └── tauri.conf.json          # Konfigurasi Window & Build Tauri App
├── PKGBUILD                     # Resep Paket Arch Linux
├── index.html                   # HTML Entrypoint
├── vite.config.js               # Konfigurasi Build Vite
└── package.json                 # Node.js Package Manifest & Scripts
```

---

## 📄 Pengaturan Parameter Aplikasi

Di dalam antarmuka aplikasi, Anda dapat mengonfigurasi parameter berikut:

| Parameter | Deskripsi | Contoh Nilai |
| :--- | :--- | :--- |
| **URL PMA Remote** | URL basis web PhpMyAdmin remote | `http://192.168.1.100/phpmyadmin` |
| **PMA Kredensial** | Username & Password login PhpMyAdmin | `root` / `******` |
| **Database Remote** | Nama database target di server remote | `db_store` |
| **Table List & PK** | Pilih tabel remote & atur *Primary Key* masing-masing | `orders` (`id`), `users` (`user_id`) |
| **Local MySQL Host** | Host database MySQL / MariaDB lokal | `127.0.0.1` |
| **Local MySQL Port** | Port database MySQL / MariaDB lokal | `3306` |
| **Local Kredensial** | Username & Password MySQL lokal | `root` / `******` |
| **Local Database** | Nama database lokal penerima data | `db_store_local` |
| **Mode Sinkronisasi** | `incremental` (New & Update) / `fresh` (Resync) | `incremental` |
| **Row Limit** | Batas maksimum baris per sesi sync (0 = Semua Row) | `0`, `10000`, `100000` |
| **Interval Auto-Sync**| Interval otomatisasi sync (0 = Manual) | `0` (Manual), `10` (10s), `60` (1m) |

---

## 📄 Lisensi & Hak Cipta

Hak Cipta © 2026 **DB-Sync Team**.
Proyek ini dirilis di bawah lisensi [MIT License](LICENSE). Dibuat untuk mendukung sinkronisasi data antar lingkungan database secara cepat, aman, dan efisien.
