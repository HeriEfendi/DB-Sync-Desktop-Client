# 🗄️ DB-Sync Desktop Client

![Version](https://img.shields.io/badge/version-0.24.3-emerald)
![Tauri](https://img.shields.io/badge/Tauri-v2-blue?logo=tauri)
![Vue](https://img.shields.io/badge/Vue.js-3.5-brightgreen?logo=vuedotjs)
![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![License](https://img.shields.io/badge/license-Proprietary-red)

**DB-Sync Desktop Client** adalah aplikasi desktop **Tauri v2 (Rust)** + **Vue 3** berkinerja tinggi untuk menyalin data dari **PhpMyAdmin (PMA) Remote** ke **MySQL/MariaDB lokal**. Remote PMA diperlakukan sebagai sumber data **read-only**: aplikasi login lalu mengekspor data terkompresi; seluruh operasi penulisan, `DROP`, `DELETE`, dan penyesuaian skema hanya dieksekusi di database lokal.

Aplikasi dirancang untuk menangani database skala kecil hingga besar (~5+ GB) dengan strategi _Zero-Memory-Bloat Direct CSV/GZIP Stream Pipeline_ tanpa membebani memori sistem ataupun menyebabkan server hosting remote overload / timeout.

---

## ✨ Fitur Utama

- 🔒 **Remote PMA 100% Read-Only**: Tidak ada query modifikasi data (`INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `TRUNCATE`) yang dikirim ke server remote. PMA remote hanya menerima autentikasi login dan query `SELECT` untuk proses export.
- ⚡ **Native CSV + GZIP Stream (Ultra-Ringan di Server)**:
  - Mengambil data dalam format **CSV terkompresi GZIP** (`what=csv`, `compression=gzip`) dari endpoint native `export.php` PMA.
  - Membebaskan PHP server remote dari beban formatting string SQL yang memeras CPU hingga 100%. Beban CPU PHP-FPM di server remote turun drastis (hanya ~20%–35%) dan penggunaan RAM server tetap dingin (< 20 MB).
- 🦀 **Local Rust Batch Engine & Fail-Safe Fallback**:
  - Rust client mendekompresi GZIP on-the-fly (`flate2`) dan mem-parse baris CSV secara instan di CPU lokal pengguna (`csv` crate).
  - Mengalirkan batch SQL `REPLACE INTO / INSERT INTO` yang aman langsung ke STDIN child process `mysql` lokal dengan transaksi InnoDB (`COMMIT`).
  - Dilengkapi **Auto-Fallback**: Jika modul CSV tidak aktif di server remote tertentu, sistem otomatis beralih ke mode SQL stream standar.
- 🐳 **Dukungan Native Docker Container**: MySQL lokal berjalan di dalam kontainer Docker? DB-Sync mendukung eksekusi direct stream via `docker exec -i <container> mysql` dengan dukungan `--local-infile=1` tanpa mewajibkan Anda menginstall CLI `mysql`/`mariadb` di OS host.
- 🎯 **Keyset Pagination (Seek Method) Berbasis Index O(1)**:
  - Chunking tabel skala besar tidak menggunakan `OFFSET` besar yang membebani I/O disk server.
  - Nilai Primary Key terakhir diambil langsung dari baris CSV, dan chunk berikutnya mengambil data secara instan via B-Tree index: `WHERE pk > last_id ORDER BY pk ASC LIMIT 100000`.
- ⏱️ **Adaptive Throttling & Micro-Pause**:
  - Jeda napas mikro (250ms) antar-chunk dan jeda inter-tabel (300ms–500ms) memberikan waktu bagi CPU server remote untuk kembali idle, menjaga *Load Average* server tetap rendah dan mencegah throttle CloudLinux LVE / alert admin server.
- 🏗️ **Skema DDL Pre-fetch**: Struktur tabel (`CREATE TABLE IF NOT EXISTS`) diambil sekali secara terpisah di awal, menjamin definisi kolom dan tipe data di MySQL lokal 100% presisi sebelum data dialirkan.
- 🔄 **Dua Mode Sinkronisasi**:
  - **Sync Server (New & Update)**: Menggunakan watermark `Last ID` dan `Last Sync Time`. Data lokal dengan `id > Last ID` dihapus, lalu mengimpor data baru (`id > Last ID`) dan data yang berubah (`updated_at > Last Sync Time`).
  - **Fresh Sync**: Menjalankan `DROP TABLE IF EXISTS` **hanya di MySQL lokal**, lalu mengimpor ulang struktur skema (`CREATE TABLE`) dan seluruh data dari remote.
- 🛡️ **Auto-Fallback on Schema Mismatch**: Jika sinkronisasi incremental gagal akibat perbedaan struktur kolom/skema antara remote dan lokal (misal: kolom baru ditambahkan di server), sistem secara otomatis melakukan fallback ke **Fresh Sync** khusus untuk tabel tersebut.
- 🔍 **Automatic Discovery**: Deteksi otomatis Primary Key per tabel dan ketersediaan kolom `updated_at` dari `INFORMATION_SCHEMA` server remote.
- ⏩ **Zero-Row Fast Skip**: Tabel kosong atau tabel tanpa perubahan data baru dilewati secara instan (_fast-skip_) tanpa memicu proses import lokal.
- 📁 **Manajemen Profil Preset**: Simpan, muat, perbarui, dan beralih antar profil koneksi (kombinasi PMA remote + MySQL lokal) dengan satu klik.
- 📋 **Multi-Table Selection & Templates**: Cari tabel, centang massal, input tabel manual, serta simpan _template centangan tabel_ untuk digunakan kembali.
- 🎚️ **Row Limit Terbaru**: Opsi membatasi jumlah baris per tabel (berguna untuk sampling data dev/staging) dengan pengurutan `ORDER BY primary_key DESC LIMIT N`.
- ⏱️ **Auto-Sync & Live Statistics**: Pengulangan otomatis berkala (5 detik hingga 5 menit), penghitung baris masuk live, throughput kecepatan transfer (MB/s), serta tombol pembatalan aman (_Stop Sync_).
- 🧾 **Per-Table Sync State**: Riwayat watermark tersimpan per tabel di `localStorage`. Dilengkapi modal untuk meninjau dan mereset watermark per tabel atau seluruhnya.
- 🧪 **Dual Connection Tester**: Validasi koneksi PMA remote dan MySQL lokal (TCP socket + uji kontainer Docker) sebelum sinkronisasi dimulai.

> [!WARNING]
> **Mode Sync Server (New & Update) menghapus baris lokal dengan ID di atas `Last ID`.** Mode ini dirancang jika server remote adalah sumber kebenaran (_single source of truth_). Jangan gunakan pada tabel yang memiliki data lokal-only yang ingin dipertahankan.

> [!NOTE]
> Deteksi pembaruan data lama membutuhkan kolom `updated_at` (atau sejenisnya) di tabel remote. Tabel tanpa kolom timestamp pembaruan tetap dapat menerima data baru berdasarkan Primary Key.

---

## 🏗️ Arsitektur & Alur Data

```
                     +---------------------------------------+
                     |         PhpMyAdmin Remote             |
                     |  (Web / Apache / Nginx / PHP-FPM)     |
                     +-------------------+-------------------+
                                         |
                                         | POST export.php (CSV + GZIP Stream)
                                         | [Sangat ringan: No SQL formatting in PHP]
                                         v
                     +---------------------------------------+
                     |         Rust Engine (Client)          |
                     |  1. flate2 (GzDecoder Stream)         |
                     |  2. csv parser -> Instant SQL Batch   |
                     |  3. Keyset Pagination (Seek O(1))     |
                     +-------------------+-------------------+
                                         |
                        Direct STDIN Pipe | (Zero-Memory-Bloat, Tanpa temp file)
                                         |
                +-------------------------+-------------------------+
                | (Mode Standar Host)                               | (Mode Docker Container)
                v                                                   v
+-----------------------------+                     +-----------------------------+
|    Local MySQL / MariaDB    |                     |   Docker Container Engine   |
|      (Host CLI Client)      |                     | `docker exec -i <c> mysql`  |
|      `--local-infile=1`     |                     |     `--local-infile=1`      |
+-----------------------------+                     +-----------------------------+
               |                                                   |
               +-------------------------+-------------------------+
                                         v
                            +--------------------------+
                            | Local Database Instance  |
                            |     (127.0.0.1:3306)     |
                            +--------------------------+
```

### Tahapan Eksekusi Sinkronisasi:

1. **Verifikasi Database Lokal**: Backend Rust memastikan database lokal tujuan sudah ada (`CREATE DATABASE IF NOT EXISTS`) melalui MySQL CLI host atau Docker container.
2. **Autentikasi & Handshake**: Mengambil sesi cookie dan CSRF token dari login PMA remote.
3. **Auto-Discovery Skema**: Mengambil daftar Primary Key dan tabel dengan kolom `updated_at` langsung dari `INFORMATION_SCHEMA`.
4. **Prefetch Struktur (DDL)**: Mengambil definisi skema tabel (`CREATE TABLE`) sekali di awal untuk menyiapkan struktur lokal.
5. **Producer (Pre-fetch)**: Mengunduh data per tabel dalam potongan CSV + GZIP terkompresi melalui antrean terkelola (*bounded queue*) dengan 2 download worker paralel dan jeda micro-pause (250ms).
6. **Consumer (Decompress, Convert & Pipe)**:
   - Jika tabel 0 baris dan tidak butuh rebuild skema: **Fast-Skip**.
   - Jika tabel berisi data: Rust mendekompresi GZIP dan mem-parse CSV menjadi batch SQL `REPLACE/INSERT INTO` secara lokal, lalu menyalurkan langsung ke STDIN child process `mysql` lokal secara real-time.
   - Jika terjadi error skema (kolom tidak cocok): Menjalankan _Auto-Fallback ke Fresh Sync_.
7. **Commit Watermark**: Membaca `MAX(primary_key)` lokal baru dan memperbarui timestamp di riwayat lokal.

---

## 🛠️ Teknologi yang Digunakan

| Layer                | Komponen / Library                            | Deskripsi                                                       |
| :------------------- | :-------------------------------------------- | :-------------------------------------------------------------- |
| **Frontend UI**      | **Vue 3** (Composition API `<script setup>`)  | Antarmuka interaktif dan responsif                              |
| **Build Tool**       | **Vite v6**                                   | Kompilasi frontend ultra-cepat                                  |
| **Styling**          | **Vanilla CSS + Glassmorphism Tokens**        | Tema gelap modern, responsif, dan ringan tanpa dependensi berat |
| **Iconography**      | **Lucide Vue Next**                           | Icon set modern dan konsisten                                   |
| **Desktop Core**     | **Tauri v2**                                  | Framework aplikasi desktop native lintas platform               |
| **Backend Engine**   | **Rust (Edition 2021)**                       | Logika sinkronisasi native memori aman berkecepatan tinggi      |
| **CSV Processing**   | **csv 1.4 / csv-core**                        | Parsing data CSV tabular streaming ultra-cepat di sisi client   |
| **Database Client**  | **sqlx 0.8** (Tokio Async, Native TLS, MySQL) | Driver MySQL TCP async untuk inspeksi tabel & query lokal       |
| **HTTP & Streaming** | **reqwest / flate2 / urlencoding**            | Client HTTP cookie-store, streaming dekompresi GZIP real-time   |
| **IPC & Serde**      | **serde / serde_json / tauri-macros**         | Komunikasi data asinkron antara UI Vue dan backend Rust         |omunikasi data asinkron antara UI Vue dan backend Rust         |

---

## 📋 Prasyarat Sistem

1. **Node.js**: v18.0.0 atau lebih baru ([Download Node.js](https://nodejs.org/)).
2. **Rust & Cargo**: Toolchain Rust versi stabil terbaru ([Install Rust](https://www.rust-lang.org/tools/install)).
3. **Database Target Lokal**:
   - **Opsi A (Host)**: MySQL Server atau MariaDB Server berjalan di port `3306` serta client CLI `mysql`/`mariadb` ada di PATH.
   - **Opsi B (Docker)**: Kontainer Docker MySQL/MariaDB aktif dengan port yang di-mapping ke host.
4. **PhpMyAdmin Remote**: Akses web PhpMyAdmin remote yang dapat dijangkau via HTTP/HTTPS.

---

## 🚀 Panduan Menjalankan Aplikasi

### 1. Clone & Install Dependency

```bash
# Clone repository
git clone https://github.com/HeriEfendi/DB-Sync-Desktop-Client.git
cd DB-Sync-Desktop-Client

# Install dependency Node.js
npm install
```

### 2. Mode Pengembangan (Development)

Jalankan aplikasi desktop berbasis Tauri:

```bash
npm run tauri dev
```

> [!NOTE]
> Jika Anda hanya menjalankan `npm run dev`, aplikasi akan terbuka di web browser standar tanpa akses IPC ke backend Rust dan soket MySQL lokal. Selalu gunakan `npm run tauri dev`.

---

## ⚙️ Panduan Konfigurasi & Pengaturan

### 1. Remote PMA

- **URL PMA Remote**: URL lengkap ke antarmuka PhpMyAdmin (contoh: `https://pma.perusahaan.com`).
- **PMA Username & Password**: Kredensial akun database remote Anda.
- **Database Remote**: Nama database sumber di server remote.
- **Default Primary Key**: Nama kolom acuan watermark urutan data (default: `id`).

### 2. MySQL Lokal & Pilihan Eksekusi

- **Host Server MySQL**: Host MySQL lokal (default: `127.0.0.1`).
- **Port TCP**: Port MySQL lokal (default: `3306`).
- **Username & Password**: Kredensial database lokal (default user: `root`).
- **Database Target Lokal**: Nama database penampung di lokal (dibuat otomatis jika belum ada).
- **Gunakan Docker Container (docker exec)**:
  - Centang opsi ini jika MySQL lokal berjalan di dalam kontainer Docker dan OS host tidak memiliki client `mysql` terinstall.
  - Masukkan nama atau ID kontainer pada kolom **Nama / ID Kontainer Docker** (contoh: `mysql-local` atau `db-app`).

### 3. Profil Preset Database

Anda dapat menyimpan konfigurasi lengkap (PMA Remote + MySQL Lokal) ke dalam preset:

- **Buat**: Simpan konfigurasi form saat ini sebagai preset baru.
- **Simpan**: Perbarui preset yang sedang dipilih.
- **Hapus**: Hapus preset dari daftar.

### 4. Mode Sinkronisasi

| Mode                           | Aksi di MySQL Lokal                                                                    | Aksi di PMA Remote     | Skenario Penggunaan                                                                        |
| :----------------------------- | :------------------------------------------------------------------------------------- | :--------------------- | :----------------------------------------------------------------------------------------- |
| **Sync Server (New & Update)** | Hapus baris lokal di atas `Last ID`, import baris baru dan baris yang mengalami update | `SELECT` / export saja | Menjaga data lokal tetap sinkron dengan server tanpa perlu download ulang seluruh database |
| **Fresh Sync**                 | `DROP TABLE IF EXISTS`, lalu impor ulang struktur dan seluruh isi data                 | `SELECT` / export saja | Inisialisasi awal, reset skema tabel lokal, atau tabel tanpa Primary Key                   |

---

## ❓ Troubleshooting & Pertanyaan Umum (FAQ)

### 1. Error: `No such file or directory (os error 2)` saat sinkronisasi

- **Penyebab**: Aplikasi mencoba mengeksekusi binary `mysql` CLI pada OS host, namun tool client MySQL belum terinstall di PATH sistem.
- **Solusi**:
  - **Jika memakai Docker**: Di tab _MySQL Lokal_, centang **Gunakan Docker Container (docker exec)** dan isi nama kontainer MySQL Anda.
  - **Jika tanpa Docker**: Install client CLI ringan di OS Anda:
    - Ubuntu/Debian: `sudo apt install default-mysql-client`
    - Arch Linux: `sudo pacman -S mariadb-clients`
    - macOS: `brew install mysql-client`

### 2. Terjadi `Schema Mismatch / Column doesn't match`

- **Solusi**: Sistem memiliki fitur **Auto-Fallback**. Jika terjadi perubahan kolom di server remote, DB-Sync akan otomatis mengubah mode tabel tersebut menjadi Fresh Sync sehingga struktur lokal disesuaikan kembali secara otomatis.

### 3. Error `Session Expired` atau `CSRF Token Invalid`

- **Penyebab**: Sesi login phpMyAdmin remote telah habis masa berlakunya atau server menerapkan proteksi IP/Cookie yang ketat.
- **Solusi**: Jalankan ulang tombol _"Tes Koneksi PMA"_ untuk memperbarui sesi cookie dan CSRF token baru.

---

## 🛠️ Build & Rilis Installer

### Build Manual Lokal

```bash
npm run tauri build
```

Hasil installer executable dapat ditemukan pada folder: `src-tauri/target/release/bundle/`.

#### Build Khusus Arch Linux (`.pkg.tar.zst`)

```bash
npm run build:pacman
```

### Rilis Otomatis Lintas Platform (GitHub Actions)

Gunakan skrip rilis untuk memperbarui seluruh file versi manifest secara serentak dan memicu build multi-platform otomatis:

```bash
# Contoh merilis versi 0.22.1
npm run release 0.22.1
```

---

## 📄 Lisensi & Hak Cipta

Hak Cipta © 2026 **Heri Efendi**.
Proyek ini dirilis di bawah **Proprietary License** — bebas diunduh dan digunakan untuk keperluan pribadi & non-komersial, namun **dilarang dimodifikasi untuk didistribusikan** atau digunakan untuk keperluan komersial tanpa izin tertulis. Lihat [LICENSE](LICENSE) untuk detail lengkap.
