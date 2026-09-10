# 🗄️ DB-Sync Desktop Client

![Version](https://img.shields.io/badge/version-0.22.1-emerald)
![Tauri](https://img.shields.io/badge/Tauri-v2-blue?logo=tauri)
![Vue](https://img.shields.io/badge/Vue.js-3.5-brightgreen?logo=vuedotjs)
![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![License](https://img.shields.io/badge/license-Proprietary-red)

**DB-Sync Desktop Client** adalah aplikasi desktop **Tauri v2 (Rust)** + **Vue 3** berkinerja tinggi untuk menyalin data dari **PhpMyAdmin (PMA) Remote** ke **MySQL/MariaDB lokal**. Remote PMA diperlakukan sebagai sumber data **read-only**: aplikasi login lalu mengekspor data SQL/GZIP terkompresi; seluruh operasi penulisan, `DROP`, `DELETE`, dan penyesuaian skema hanya dieksekusi di database lokal.

Aplikasi dirancang untuk menangani database skala kecil hingga besar (~5+ GB) dengan strategi _Zero-Memory-Bloat Direct GZIP Stream_ tanpa membebani memori sistem ataupun menyebabkan server timeout.

---

## ✨ Fitur Utama

- 🔒 **Remote PMA 100% Read-Only**: Tidak ada query modifikasi data (`INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `TRUNCATE`) yang dikirim ke server remote. PMA remote hanya menerima autentikasi login dan query `SELECT` untuk proses export.
- 🚀 **Direct GZIP Stream (Zero-Memory-Bloat)**: Mengalirkan data chunked stream langsung dari endpoint native `export.php` PMA remote, didekompresi secara real-time di Rust (`flate2`), dan langsung di-pipe ke STDIN `mysql` lokal tanpa menyimpan file dump raksasa di disk.
- 🐳 **Dukungan Native Docker Container**: MySQL lokal berjalan di dalam kontainer Docker? DB-Sync mendukung eksekusi direct stream via `docker exec -i <container> mysql` tanpa mewajibkan Anda menginstall CLI `mysql`/`mariadb` di OS host.
- ⚡ **Bounded Pre-fetching Pipeline (Producer-Consumer)**: Download export tabel dari remote dijalankan secara pre-fetch paralel sesuai jumlah core CPU, sementara proses dekompresi dan import ke MySQL lokal dijalankan berurutan dengan throttling terkontrol agar CPU server remote tidak overload.
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
                     |  (Web / Apache / Nginx / PHP Engine)  |
                     +-------------------+-------------------+
                                         |
                                         | POST export.php (GZIP Stream)
                                         v
                     +---------------------------------------+
                     |     Rust Exporter (Bounded Queue)     |
                     |  Reqwest (Cookie + CSRF) -> Decoder   |
                     +-------------------+-------------------+
                                         |
                       Direct STDIN Pipe | (Tanpa simpan file dump ke disk)
                                         |
               +-------------------------+-------------------------+
               | (Mode Standar Host)                               | (Mode Docker Container)
               v                                                   v
+-----------------------------+                     +-----------------------------+
|    Local MySQL / MariaDB    |                     |   Docker Container Engine   |
|      (Host CLI Client)      |                     | `docker exec -i <c> mysql`  |
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
4. **Producer (Pre-fetch)**: Mengunduh export SQL GZIP terkompresi per tabel secara paralel melalui antrean berbatas (_bounded queue_) dengan jeda throttling aman (300ms–500ms).
5. **Consumer (Decompress & Pipe)**:
   - Jika tabel 0 baris dan tidak butuh rebuild skema: **Fast-Skip**.
   - Jika tabel berisi data: Membuka STDIN child process `mysql` (atau `docker exec -i`) dan menyalurkan chunk data dekompresi secara real-time.
   - Jika terjadi error skema (kolom tidak cocok): Menjalankan _Auto-Fallback ke Fresh Sync_.
6. **Commit Watermark**: Membaca `MAX(primary_key)` lokal baru dan memperbarui timestamp di riwayat lokal.

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
| **Database Client**  | **sqlx 0.8** (Tokio Async, Native TLS, MySQL) | Driver MySQL TCP async untuk inspeksi tabel & query lokal       |
| **HTTP & Streaming** | **reqwest / flate2 / urlencoding**            | Client HTTP cookie-store, streaming dekompresi GZIP real-time   |
| **IPC & Serde**      | **serde / serde_json / tauri-macros**         | Komunikasi data asinkron antara UI Vue dan backend Rust         |

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
