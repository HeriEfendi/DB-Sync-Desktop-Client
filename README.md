# 🗄️ DB-Sync Desktop Client

**DB-Sync Desktop Client** adalah aplikasi desktop modern berbasis **Tauri v2 (Rust)** dan **Vue 3** yang dirancang untuk melakukan **sinkronisasi data incremental** secara efisien dari web interface **PhpMyAdmin (PMA) Remote** langsung ke **Database MySQL / MariaDB Lokal** (Port 3306).

Aplikasi ini sangat berguna untuk sinkronisasi data transaksi, log, atau tabel database dari server produksi/staging ke lingkungan pengujian/pengembangan lokal secara real-time atau terjadwal tanpa harus melakukan export-import database manual secara berulang.

---

## ✨ Fitur Utama

- 🔄 **Incremental Data Synchronization**: Menarik baris data baru secara bertahap berdasarkan *Primary Key* (`WHERE primary_key > last_local_id`) sehingga proses transfer efisien dan cepat tanpa menduplikasi data yang sudah ada.
- ⚡ **High-Performance Bulk Upsert (`ON DUPLICATE KEY UPDATE`)**: Eksekusi pengolahan batch data berkecepatan tinggi ke database lokal yang ditangani langsung oleh *native engine* Rust menggunakan `sqlx`.
- ⏱️ **Auto-Sync Scheduler**: Dilengkapi fitur penjadwalan otomatis dengan interval yang dapat disesuaikan (misal: setiap 10s, 30s, 60s, dsb.).
- 🧪 **Connection Tester**: Fitur pengujian independen untuk memvalidasi akses koneksi ke **Remote PhpMyAdmin** (HTTP/HTTPS) dan **MySQL Lokal** (Port 3306).
- 📊 **Data Inspector & Preview**: Melihat sampel data hasil sinkronisasi terkini langsung di dalam aplikasi desktop.
- 📋 **Live Log Console**: Log aktivitas sinkronisasi real-time dengan status indikator (Success, Info, Warning, Error) serta opsi pembersihan log.
- 🛡️ **SQL Injection Protection & Sanitization**: Sanitasi *SQL Identifiers* (nama tabel dan kolom) dilakukan di layer Rust untuk menjamin keamanan query.
- 💾 **Persistent Settings**: Menyimpan otomatis alamat endpoint, kredensial, dan opsi tabel ke `LocalStorage` agar siap digunakan kembali saat aplikasi dibuka.

---

## 🏗️ Arsitektur & Alur Kerja Sinkronisasi

```
  +-----------------------+              +-----------------------------+
  |  PhpMyAdmin Remote    |              | DB-Sync Desktop Client UI   |
  |  (Web / HTTP Server)  |              | Vue 3 + Lucide Icons        |
  +-----------+-----------+              +--------------+--------------+
              ^                                         |
              | HTTP Query                              | IPC Invocation
              v                                         v
  +-----------+-----------+              +--------------+--------------+
  | PMA Client Parser     | ------------>| Sync Engine Manager         |
  | (CSRF Token & JSON)   |              | (Auto-Sync & State Control) |
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
1. **Check Local State**: Engine mengambil nilai *Primary Key* maksimum (`MAX(primary_key)`) dari tabel MySQL lokal via Rust `sqlx`.
2. **Authenticate PMA Remote**: Menginisialisasi sesi HTTP ke PhpMyAdmin remote, mengekstrak cookie sesi dan CSRF token.
3. **Fetch Incremental Rows**: Mengeksekusi query incremental (`SELECT * FROM table WHERE pk > last_id ORDER BY pk ASC LIMIT N`) dari PMA remote.
4. **Bulk Upsert Local**: Mengirimkan batch baris data ke backend Rust untuk disisipkan/diperbarui (*Upsert*) ke MySQL lokal.

---

## 🛠️ Teknologi yang Digunakan

### **Frontend & UI Layer**
- **Vue 3** (Composition API with `<script setup>`)
- **Vite** (Build Tool & Dev Server)
- **Lucide Vue Next** (Iconography Set)
- **Custom CSS Design System** (Dark mode theme, glassmorphism, responsive grid layout)

### **Desktop Engine & Backend Layer**
- **Tauri v2** (Cross-platform desktop application framework)
- **Rust** (High-performance native system logic)
- **sqlx** (Asynchronous MySQL/MariaDB driver powered by Tokio)
- **serde / serde_json** (Fast JSON serialization & deserialization)

---

## 📋 Prasyarat Sistem

Sebelum menjalankan atau membagikan aplikasi ini, pastikan sistem Anda memenuhi kebutuhan berikut:

1. **Node.js**: v18.0.0 atau lebih baru ([Download Node.js](https://nodejs.org/))
2. **Rust & Cargo**: Toolchain Rust terbaru ([Install Rust](https://www.rust-lang.org/tools/install))
3. **Database Server Lokal**: MySQL Server atau MariaDB Server berjalan di port `3306` (atau port pilihan Anda).
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

### 4. Rilis Otomatis Windows & Linux

Push tag versi untuk memicu GitHub Actions:

```bash
npm run release -- 1.0.1
```

Workflow [release.yml](file:///home/lenovo/www/DB-Sync-Desktop-Client/.github/workflows/release.yml) otomatis:

- Build Linux (`.deb`, `.AppImage` bila didukung runner)
- Build Windows (`.msi`, `.exe` sesuai konfigurasi Tauri)
- Membuat GitHub Release
- Melampirkan installer ke release

GitHub Actions membutuhkan permission repository `Contents: write`.

## Build Lokal

```bash
npm run tauri build
```

Hasil build lokal ada di `src-tauri/target/release/bundle/`.


## 📁 Struktur Proyek

```
DB-Sync Desktop Client/
├── src/                         # Frontend Layer (Vue 3)
│   ├── components/              # Komponen UI Modular
│   │   ├── ConnectionConfig.vue # Form Konfigurasi Koneksi Remote & Lokal
│   │   ├── SyncControl.vue      # Kontrol Tombol Sync & Penjadwalan Auto-Sync
│   │   ├── LogConsole.vue       # Console Monitor Log Real-Time
│   │   ├── DataPreview.vue      # Data Inspector & Tabel Preview Data Lokal
│   │   └── Navbar.vue           # Header Navigation & Status Indicator Badges
│   ├── services/                # Business Logic Services
│   │   ├── pmaClient.js         # HTTP Client untuk Authenticate & Query PhpMyAdmin
│   │   ├── syncEngine.js        # Engine Koordinasi Sinkronisasi Incremental
│   │   └── tauriHelper.js       # Helper Bridge IPC (Tauri Invoke & Fetch)
│   ├── styles/                  # Styling Token & Global CSS Rules
│   ├── App.vue                  # Main Layout & State Aggregator
│   └── main.js                  # Vue Entry Point
├── src-tauri/                   # Backend Layer (Rust Engine)
│   ├── src/
│   │   ├── commands.rs          # Tauri IPC Commands (sqlx MySQL queries & bulk upsert)
│   │   ├── lib.rs               # Registrasi Command Handler & Plugin Tauri
│   │   └── main.rs              # Entrypoint Binary Tauri
│   ├── Cargo.toml               # Dependensi & Manifest Crate Rust
│   └── tauri.conf.json          # Konfigurasi Window & Build Tauri App
├── index.html                   # HTML Entrypoint
├── vite.config.js               # Konfigurasi Build Vite
└── package.json                 # Node.js Package Manifest & Scripts
```

---

## 📄 Pengaturan Parameter Sinkronisasi

Di dalam antarmuka aplikasi, Anda dapat mengonfigurasi parameter berikut:

| Parameter | Deskripsi | Example Value |
| :--- | :--- | :--- |
| **URL PMA Remote** | URL akses basis PhpMyAdmin | `http://192.168.1.100/phpmyadmin` |
| **PMA Username / Password** | Kredensial login ke PhpMyAdmin | `root` / `******` |
| **Database & Tabel Remote** | Database dan tabel target yang ada di PMA | `db_store` / `orders` |
| **Primary Key** | Kolom kunci utama bertipe auto-increment / timestamp / ID sekuensial | `id` |
| **Local MySQL Config** | Host, Port, Username, Password, Database & Tabel Lokal | `127.0.0.1:3306` |

---

## 📄 Lisensi & Hak Cipta

Hak Cipta © 2026 **DB-Sync Team**.
Proyek ini dibuat untuk mendukung sinkronisasi data antar lingkungan database secara cepat, aman, dan efisien.
