# Antigravity Workspace Instructions & Architecture Guide

# Project: DB Sync Desktop Client (Tauri v2 + Vue 3 + Rust)

> [!IMPORTANT]
> Seluruh agen AI yang bekerja di workspace ini **WAJIB** mematuhi strategi sinkronisasi data, arsitektur backend Rust, dan komunikasi frontend Vue 3 yang dijelaskan di bawah ini secara otomatis.

---

## 1. CORE SYNC STRATEGY: Direct GZIP Stream via `export.php`

Aplikasi **TIDAK BOLEH** menggunakan scraping tampilan view HTML (`sql.php` / table view). Seluruh proses dump & sinkronisasi data harus menggunakan endpoint ekspor native phpMyAdmin (`export.php`) dengan kompresi GZIP untuk menangani database skala besar (~5+ GB) tanpa memori bloat atau timeout.

---

## 2. BACKEND SPECIFICATION & RULES (Rust)

### A. Authentication & Session Management

- Gunakan `reqwest::Client` dengan `cookie_store(true)` agar Session Cookie (`phpMyAdmin`, `pma_lang`, dll.) dan CSRF token tersimpan otomatis.
- Saat login (`POST /index.php`), ekstrak dan simpan CSRF Token (`token` / `set_session`) jika versi phpMyAdmin membutuhkannya.

### B. Export Endpoint & Mandatory Payload

- **Target Endpoint**: `POST {PMA_BASE_URL}/export.php` (bukan `sql.php` atau `import.php`).
- **Payload (`application/x-www-form-urlencoded`)**:
  - `db`: `{NAMA_DATABASE_REMOTE}`
  - `table`: `{NAMA_TABEL}` (Lakukan iterasi berurutan per-tabel dari hasil `SHOW TABLES` untuk mencegah server timeout).
  - `what`: `"sql"`
  - `export_type`: `"table"` (atau `"database"` untuk database kecil)
  - `output_format`: `"sendit"` _(MANDATORY: Memaksa PMA bertindak sebagai File Downloader/Streamer, bukan HTML render)_.
  - `compression`: `"gzip"` _(MANDATORY: Mengompresi SQL mentah 80-90% saat transit)_.
  - `asfile`: `"sendit"`
  - `token`: `{CSRF_TOKEN}` (jika tersedia).

### C. Response Validation & Error Handling

Sebelum mengalirkan data ke MySQL lokal, periksa `Content-Type` dari HTTP Response:

- **Jika `Content-Type` mengandung `text/html`**: Proses gagal (Session expired / Error PHP). Baca response sebagai string, log error ke frontend, dan **STOP eksekusi**. JANGAN alirkan HTML ke MySQL.
- **Jika `Content-Type` adalah `application/x-gzip` / `application/octet-stream` / `text/plain`**: Data valid, lanjutkan streaming.

### D. Zero-Memory-Bloat Streaming Pipeline

- Baca HTTP Response secara chunked stream (`reqwest::Response::bytes_stream`).
- Gunakan `flate2::read::GzDecoder` (atau async decoder) untuk mendekompresi GZIP stream secara real-time.
- Pipe / tulis langsung hasil dekompresi ke `STDIN` dari child process `mysql` CLI lokal (`mysql -u {USER} -p{PASS} {LOCAL_DB}`).
- **DILARANG** menyimpan file dump 5 GB ke disk local/remote. Alur data harus berupa direct pipe:
  $$\text{PMA GZIP Stream} \longrightarrow \text{Rust GzDecoder} \longrightarrow \text{MySQL Local STDIN}$$

### E. Sequential Loop & Throttling

- Ambil daftar tabel terlebih dahulu (`SHOW TABLES`).
- Eksekusi request `export.php` secara sequential (per tabel).
- Berikan jeda throttling (300ms – 500ms via `tokio::time::sleep`) di antara tabel agar CPU server remote tidak overloaded.
- Pancarkan event progress Tauri (`pma-log`, `pma-progress`) ke Frontend Vue 3 untuk setiap tabel.

---

## 3. FRONTEND SPECIFICATION (Vue 3 + Tailwind/CSS)

- Dengarkan event `pma-log` dan `pma-progress` dari Tauri backend untuk menampilkan progress bar dan live terminal logs.
- Tampilkan nama tabel yang sedang di-sync, ukuran data yang ditransfer, dan estimasi waktu.
- Sediakan kontrol cancel / abort sync yang aman melalui Tauri command.

"Gunakan repo-graph orient untuk memahami alur project ini."
"Gunakan repo-graph find untuk mencari fungsi sinkronisasi database."
"Gunakan repo-graph trace untuk melihat alur dari klik tombol sampai ke backend."
