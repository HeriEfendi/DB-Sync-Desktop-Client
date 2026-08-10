# 🗄️ DB-Sync Desktop Client

![Version](https://img.shields.io/badge/version-0.8.2-emerald)
![Tauri](https://img.shields.io/badge/Tauri-v2-blue?logo=tauri)
![Vue](https://img.shields.io/badge/Vue.js-3.5-brightgreen?logo=vuedotjs)
![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![License](https://img.shields.io/badge/license-MIT-blue)

**DB-Sync Desktop Client** adalah aplikasi desktop **Tauri v2 (Rust)** + **Vue 3** untuk menyalin data dari **PhpMyAdmin (PMA) Remote** ke **MySQL/MariaDB lokal**. Remote PMA dipakai sebagai sumber data **read-only**: aplikasi login lalu menjalankan export `SELECT`; seluruh `DROP`, `DELETE`, pembuatan tabel, dan import berjalan hanya di database lokal.

Aplikasi cocok untuk membuat salinan data produksi/staging ke lingkungan pengembangan atau pengujian lokal tanpa export-import dump manual. Transfer memakai export SQL/GZIP phpMyAdmin lalu dipipe langsung ke MySQL CLI lokal.

---

## ✨ Fitur Utama

- 🔒 **Remote PMA Read-Only**: Tidak ada `INSERT`, `UPDATE`, `DELETE`, `DROP`, atau `TRUNCATE` yang dikirim ke PMA remote. PMA hanya menerima login dan query `SELECT` untuk export.
- 🚀 **Direct SQL/GZIP Export**: Rust autentikasi PMA dengan cookie + CSRF token, meminta export SQL lewat endpoint PMA, mendekompresi GZIP, lalu mengirim SQL ke stdin `mysql` lokal. Tidak menyimpan dump besar di disk.
- 🔄 **Dua Mode Sync Server**:
  - **Sync Server (New & Update)**: Menggunakan `Last ID` dan waktu sync tersimpan sebagai watermark. Sebelum export, baris **lokal** dengan `id > Last ID` dihapus. Lalu aplikasi mengambil data remote `id > Last ID` atau `updated_at > Last Sync`, sehingga data lokal kembali mengikuti server.
  - **Fresh Sync**: Menjalankan `DROP TABLE IF EXISTS` **hanya di MySQL lokal**, lalu import ulang struktur dan data export dari remote. Cocok untuk penggantian penuh tabel lokal.
- 🧾 **Per-Table Sync State**: Menyimpan `Last ID`, waktu sync terakhir, primary key, dan metadata tabel di `localStorage`. Riwayat dapat di-reset per tabel atau seluruhnya.
- 📋 **Multi-Table Selection & Templates**: Pilih banyak tabel, cari tabel, centang massal, dan simpan template pilihan tabel.
- 🎚️ **Row Limit Terbaru**: Saat limit aktif, Fresh Sync mengambil baris terbaru dengan `ORDER BY primary_key DESC LIMIT N`. Default primary key adalah `id`, dapat diubah dari konfigurasi.
- ⏱️ **Auto Sync, Progress, dan Stop**: Interval otomatis, progres tabel/baris, penghentian aman, serta log ringkas untuk success/warning/error.
- 🧪 **Dual Connection Tester**: Uji koneksi PMA remote dan MySQL lokal terpisah.
- 🛡️ **Identifier Safety**: Nama tabel/kolom dikutip dan disanitasi di Rust; nilai watermark delete lokal memakai parameter query.

> [!WARNING]
> **Sync Server (New & Update) menghapus data lokal dengan ID di atas `Last ID`.** Mode ini dibuat saat server adalah sumber data utama. Jangan gunakan untuk tabel yang menyimpan data lokal-only yang ingin dipertahankan.

> [!NOTE]
> Deteksi update pada ID lama membutuhkan kolom `updated_at` di tabel remote. Tabel tanpa `updated_at` tetap bisa mengambil ID baru, tetapi perubahan server pada ID lama tidak dapat dideteksi otomatis.

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

### Alur Kerja Sync Engine

1. **Baca State Lokal**: UI membaca `Last ID` dan `Last Sync Time` per tabel dari `localStorage`.
2. **Login PMA Read-Only**: Rust membuat sesi HTTP PMA, menyimpan cookie, mengambil token CSRF, dan mengakses endpoint export.
3. **Pilih Mode**:
   - **Fresh Sync**: tabel lokal dihapus dengan `DROP TABLE IF EXISTS`, kemudian struktur + data hasil export diimport kembali ke lokal.
   - **Sync Server**: bila state ada, lokal menjalankan `DELETE ... WHERE primary_key > Last ID`. Data remote lalu difilter dengan `primary_key > Last ID OR updated_at > Last Sync Time`.
4. **Stream Export ke Lokal**: PMA mengirim dump SQL/GZIP; Rust mendekompresi dan pipe SQL langsung ke `mysql` CLI database lokal.
5. **Simpan Watermark Baru**: setelah export sukses, aplikasi mengambil `MAX(primary_key)` dari lokal dan menyimpan ID/waktu baru ke `localStorage`.

### Contoh Sync Server

State tersimpan `Last ID = 100`. Lokal memiliki data uji `101..105`, sementara server hanya memiliki `101..103`.

1. Aplikasi menghapus lokal `101..105`.
2. Aplikasi mengexport remote `101..103` dan baris lama yang `updated_at`-nya berubah.
3. Lokal berakhir di ID `103`, kembali konsisten dengan remote.

Tidak ada perubahan data pada PMA remote selama proses ini.

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
│   │   ├── ConnectionConfigSection.vue # Form konfigurasi PMA remote dan MySQL lokal
│   │   ├── TableConfig.vue      # Pilihan tabel, pencarian, dan template tabel
│   │   ├── SyncControl.vue      # Mode Fresh/Sync Server, limit, progress, dan riwayat state
│   │   ├── LogConsole.vue       # Console aktivitas sync
│   │   └── Navbar.vue           # Header dan indikator koneksi
│   ├── services/
│   │   ├── pmaClient.js         # Client browser fallback
│   │   ├── syncEngine.js        # Orkestrasi state, prune lokal, dan command Tauri
│   │   ├── syncStateStore.js    # Watermark per tabel di LocalStorage
│   │   └── tauriHelper.js       # Bridge IPC Tauri
│   ├── App.vue                  # Layout dan state aplikasi
│   └── main.js                  # Vue entry point
├── src-tauri/
│   ├── src/
│   │   ├── commands.rs          # Command MySQL lokal, MAX ID, delete watermark
│   │   ├── pma_export.rs        # Login PMA dan direct SQL/GZIP stream
│   │   ├── lib.rs               # Registrasi command Tauri
│   │   └── main.rs              # Entrypoint binary
│   ├── Cargo.toml
│   └── tauri.conf.json          # Konfigurasi window dan build Tauri
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
| **URL PMA Remote** | URL basis web PhpMyAdmin remote | `https://server.example.com/phpmyadmin` |
| **PMA Kredensial** | Username dan password login PhpMyAdmin | `root` / `******` |
| **Database Remote** | Nama database sumber di server remote | `db_store` |
| **Primary Key Default** | Kolom urutan/watermark default untuk export | `id` |
| **Tabel Dipilih** | Tabel remote yang akan disalin | `orders`, `users` |
| **MySQL Lokal** | Host, port, kredensial, dan database penerima data | `127.0.0.1:3306`, `db_store_local` |
| **Mode Sinkronisasi** | `Sync Server` atau `Fresh Sync` | `incremental` |
| **Row Limit** | Jumlah maksimum row per tabel. Saat limit aktif, Fresh memakai row terbaru berdasarkan primary key | `0`, `1000`, `100000` |
| **Interval Auto-Sync** | Interval otomatisasi | `0` (Manual), `10` (10s), `60` (1m) |

### Pilih Mode dengan Aman

| Mode | Aksi di MySQL Lokal | Aksi di PMA Remote | Gunakan Saat |
| :--- | :--- | :--- | :--- |
| **Sync Server (New & Update)** | Hapus row lokal di atas `Last ID`, import row baru dan row yang berubah | `SELECT` / export saja | Server sumber data utama; ingin menjaga tabel lokal konsisten tanpa full refresh |
| **Fresh Sync** | `DROP TABLE IF EXISTS`, lalu import ulang struktur dan data | `SELECT` / export saja | Perlu mengganti penuh salinan tabel lokal |

> [!CAUTION]
> Kedua mode dapat menghapus atau mengganti data **lokal**. Tidak satu pun mode menulis ke PMA remote.

---

## 📄 Lisensi & Hak Cipta

Hak Cipta © 2026 **DB-Sync Team**.
Proyek ini dirilis di bawah lisensi [MIT License](LICENSE). Dibuat untuk mendukung sinkronisasi data antar lingkungan database secara cepat, aman, dan efisien.
