SAYA INGIN MERUBAH STRATEGI AMBIL DATA PADA APLIKASI DESKTOP DB SYNC (TAURI V2 + VUE 3 + RUST).

[SITUASI TERKINI]
Saat ini aplikasi masih menggunakan mekanisme Scraping Tampilan View (HTML query/sql.php). Ini menyebabkan masalah timeout, parsing HTML yang sangat berat, dan ukuran payload membengkak pada database berukuran besar (~5 GB).

[TUGAS]
Ubah total mekanisme pengambilan data dari "Tampilan View HTML" menjadi "Direct SQL/GZIP Stream via Endpoint Export (`export.php`)" phpMyAdmin.

---

### SPECIFICATION & IMPLEMENTATION REQUIREMENTS (RUST BACKEND)

1. AUTHENTICATION & SESSION MANAGEMENT:
   - Gunakan `reqwest::Client` dengan `cookie_store(true)` agar Session Cookie (`phpMyAdmin`, `pma_lang`, dll.) dan CSRF Token tersimpan otomatis.
   - Saat Login (`POST /index.php`), dapatkan CSRF Token (biasanya bernama `token` atau `set_session`) jika versi PMA membutuhkannya.

2. EXPORT ENDPOINT TARGET:
   - Targetkan HTTP POST request ke `{PMA_BASE_URL}/export.php`.
   - BUKAN `sql.php` atau `import.php`.

3. MANDATORY POST PAYLOAD PARAMETERS:
   Kirimkan payload `application/x-www-form-urlencoded` dengan parameter wajib berikut:
   - `db`: {NAMA_DATABASE_REMOTE}
   - `table`: {NAMA_TABEL} (Iterasi per-tabel dari list `SHOW TABLES` untuk mencegah server timeout!)
   - `what`: "sql"
   - `export_type`: "table" (atau "database" jika tabelnya kecil)
   - `output_format`: "sendit" <-- MANDATORY: Memaksa PMA bertindak sebagai File Downloader/Streamer, BUKAN HTML render.
   - `compression`: "gzip" <-- MANDATORY: Mengompresi SQL mentah hingga 80-90% saat transit.
   - `asfile`: "sendit"
   - `token`: {CSRF_TOKEN_IF_ANY}

4. RESPONSE VALIDATION & ERROR HANDLING (SANGAT PENTING):
   - Sebelum mengalirkan data ke MySQL lokal, CEK `Content-Type` dari HTTP Response:
     - Jika `Content-Type` mengandung `text/html`, BERARTI PROSES GAGAL (Session expired / Error PHP). BACA response sebagai String, LOG error-nya ke UI Vue, dan STOP eksekusi. JANGAN biarkan HTML masuk ke MySQL!
     - Jika `Content-Type` adalah `application/x-gzip`, `application/octet-stream`, atau `text/plain`, BERARTI DATA VALID.

5. STREAMING & DECOMPRESSION PIPE (ZERO MEMORY BLOAT):
   - Baca HTTP Response secara `chunk` / `stream`.
   - Gunakan `flate2::read::GzDecoder` di Rust untuk mendekompresi stream GZIP secara real-time.
   - Pipe / Write langsung hasil dekompresi ke STDIN dari Child Process `mysql` CLI lokal (`mysql -u root -p{PASS} {LOCAL_DB}`).
   - Jangan simpan file dump 5 GB ke harddisk local/remote! Langsung stream:
     [PMA GZIP Stream] -> [Rust GzDecoder] -> [MySQL Local STDIN]

6. LOOPING & THROTTLING LOGIC:
   - Dapatkan daftar tabel terlebih dahulu.
   - Eksekusi `export.php` berurutan (Sequential Table Loop).
   - Berikan jeda/sleep kecil (misal: 300ms - 500ms) di antara tiap request tabel (`tokio::time::sleep`) agar CPU server remote tidak overloaded.
   - Kirimkan Event Progress (`pma-log`, `pma-progress`) ke Frontend Vue 3 untuk menampilkan status tabel mana yang sedang di-sync.

---

Tolong refactor seluruh logic sync di Rust backend dan panggilannya di Vue 3 sesuai instruksi di atas sekarang.
