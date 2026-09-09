use serde::{Deserialize, Serialize};
use sqlx::{
    mysql::{MySqlPool, MySqlPoolOptions},
    Row,
};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalDbConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    #[serde(default)]
    pub use_docker: bool,
    #[serde(default)]
    pub docker_container: String,
}

/// Helper function to build MySQL connection URL
pub fn build_connection_string(config: &LocalDbConfig) -> String {
    let host = if config.host.is_empty() {
        "127.0.0.1".to_string()
    } else {
        config.host.clone()
    };
    let port = if config.port == 0 { 3306 } else { config.port };

    format!(
        "mysql://{}:{}@{}:{}/{}",
        urlencoding::encode(&config.username),
        urlencoding::encode(&config.password),
        host,
        port,
        config.database
    )
}

/// Sanitize SQL identifier (table names & column names) by escaping with backticks.
pub fn sanitize_identifier(ident: &str) -> Result<String, String> {
    let trimmed = ident.trim().trim_matches('`');
    if trimmed.is_empty() {
        return Err("Nama tabel atau kolom tidak boleh kosong".to_string());
    }
    let escaped = trimmed.replace('`', "``");
    Ok(format!("`{}`", escaped))
}

pub async fn table_exists(pool: &MySqlPool, table_name: &str) -> Result<bool, String> {
    let escaped_name = table_name.replace('\'', "''");
    let query = format!(
        "SELECT COUNT(*) AS cnt FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{}'",
        escaped_name
    );

    let row = sqlx::query(&query)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            format!(
                "Gagal mengecek keberadaan tabel lokal '{}': {}",
                table_name, e
            )
        })?;

    let count = row.and_then(|r| r.try_get::<i64, _>(0).ok()).unwrap_or(0);
    Ok(count > 0)
}

pub async fn get_local_table_columns(
    pool: &MySqlPool,
    table_name: &str,
) -> Result<Vec<String>, String> {
    let escaped_name = table_name.replace('\'', "''");
    let query = format!(
        "SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{}' ORDER BY ORDINAL_POSITION ASC",
        escaped_name
    );

    let rows = sqlx::query(&query).fetch_all(pool).await.map_err(|e| {
        format!(
            "Gagal membaca struktur kolom tabel lokal '{}': {}",
            table_name, e
        )
    })?;

    let mut columns = Vec::new();
    for row in rows {
        if let Ok(col_name) = row.try_get::<String, _>(0) {
            columns.push(col_name);
        }
    }

    Ok(columns)
}

/// Test connection to local MySQL / MariaDB server
#[tauri::command]
pub async fn test_local_connection(config: LocalDbConfig) -> Result<String, String> {
    let conn_str = build_connection_string(&config);

    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Gagal terhubung ke MySQL lokal: {}", e))?;

    let row: (String,) = sqlx::query_as("SELECT VERSION()")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("Gagal mengambil versi MySQL: {}", e))?;

    pool.close().await;

    if config.use_docker {
        let container = config.docker_container.trim();
        if container.is_empty() {
            return Err("Opsi Docker diaktifkan, namun 'Nama / ID Kontainer Docker' masih kosong.".to_string());
        }

        let extract_output = |out: &std::process::Output| -> String {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("Kode status keluar: {:?}", out.status.code().unwrap_or(-1))
            }
        };

        // 1. Coba perintah 'mysql --version' di dalam kontainer
        let docker_check_mysql = std::process::Command::new("docker")
            .arg("exec")
            .arg(container)
            .arg("mysql")
            .arg("--version")
            .output();

        match docker_check_mysql {
            Ok(out) if out.status.success() => {
                let docker_ver = {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if s.is_empty() {
                        String::from_utf8_lossy(&out.stderr).trim().to_string()
                    } else {
                        s
                    }
                };
                return Ok(format!(
                    "Koneksi TCP berhasil (Server: {}) | Docker [{}] (MySQL CLI): {}",
                    row.0, container, docker_ver
                ));
            }
            Ok(out_mysql) => {
                // 2. Jika 'mysql' gagal, coba 'mariadb --version' (image MariaDB baru seperti Ubuntu 24.04 menggunakan binary 'mariadb')
                let docker_check_mariadb = std::process::Command::new("docker")
                    .arg("exec")
                    .arg(container)
                    .arg("mariadb")
                    .arg("--version")
                    .output();

                match docker_check_mariadb {
                    Ok(out_maria) if out_maria.status.success() => {
                        let docker_ver = {
                            let s = String::from_utf8_lossy(&out_maria.stdout).trim().to_string();
                            if s.is_empty() {
                                String::from_utf8_lossy(&out_maria.stderr).trim().to_string()
                            } else {
                                s
                            }
                        };
                        return Ok(format!(
                            "Koneksi TCP berhasil (Server: {}) | Docker [{}] (MariaDB CLI): {}",
                            row.0, container, docker_ver
                        ));
                    }
                    Ok(out_maria) => {
                        let err_mysql = extract_output(&out_mysql);
                        let err_maria = extract_output(&out_maria);
                        return Err(format!(
                            "Koneksi TCP MySQL berhasil ({}), tetapi 'docker exec' ke kontainer '{}' gagal.\n• mysql: {}\n• mariadb: {}",
                            row.0, container, err_mysql, err_maria
                        ));
                    }
                    Err(e) => {
                        let err_mysql = extract_output(&out_mysql);
                        return Err(format!(
                            "Koneksi TCP MySQL berhasil ({}), tetapi 'docker exec' ke kontainer '{}' gagal: {}\n(Error sistem: {})",
                            row.0, container, err_mysql, e
                        ));
                    }
                }
            }
            Err(e) => {
                return Err(format!(
                    "Koneksi TCP MySQL berhasil ({}), tetapi gagal menjalankan perintah 'docker' pada host: {}. Pastikan Docker terinstall dan ada di PATH sistem.",
                    row.0, e
                ));
            }
        }
    }

    Ok(format!("Koneksi berhasil! Versi Server: {}", row.0))
}

/// Get latest ID or maximum value of primary key from local table
#[tauri::command]
pub async fn get_last_local_id(
    config: LocalDbConfig,
    table_name: String,
    primary_key: String,
) -> Result<Option<serde_json::Value>, String> {
    let conn_str = build_connection_string(&config);
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    if !table_exists(&pool, &table_name).await.unwrap_or(false) {
        pool.close().await;
        return Ok(None);
    }

    let cols = get_local_table_columns(&pool, &table_name).await.unwrap_or_default();
    if cols.is_empty() {
        pool.close().await;
        return Ok(None);
    }

    // Auto-detect kolom ID jika primary_key yang dikirim tidak ditemukan di tabel
    let effective_pk = if cols.iter().any(|c| c.eq_ignore_ascii_case(&primary_key)) {
        primary_key.clone()
    } else if let Some(found_pk) = cols.iter().find(|c| c.eq_ignore_ascii_case("id") || c.to_ascii_lowercase().ends_with("_id") || c.to_ascii_lowercase().starts_with("id_")).cloned() {
        found_pk
    } else if let Some(first_col) = cols.first().cloned() {
        first_col
    } else {
        pool.close().await;
        return Ok(None);
    };

    let safe_table = sanitize_identifier(&table_name)?;
    let safe_pk = sanitize_identifier(&effective_pk)?;
    let query = format!("SELECT MAX({}) AS max_id FROM {}", safe_pk, safe_table);

    let row = sqlx::query(&query)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("Gagal query MAX({}): {}", effective_pk, e))?;

    pool.close().await;

    if let Some(r) = row {
        if let Ok(val) = r.try_get::<u64, _>(0) {
            return Ok(Some(serde_json::Value::Number(val.into())));
        }
        if let Ok(val) = r.try_get::<i64, _>(0) {
            return Ok(Some(serde_json::Value::Number(val.into())));
        }
        if let Ok(val) = r.try_get::<f64, _>(0) {
            if let Some(num) = serde_json::Number::from_f64(val) {
                return Ok(Some(serde_json::Value::Number(num)));
            }
        }
        if let Ok(val) = r.try_get::<String, _>(0) {
            return Ok(Some(serde_json::Value::String(val)));
        }
    }

    Ok(None)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableLastIdInfo {
    pub table: String,
    pub primary_key: String,
    pub last_id: Option<serde_json::Value>,
}

/// Batch query to fetch latest ID and Primary Key for multiple tables in a single connection pool
#[tauri::command]
pub async fn get_all_tables_last_local_ids(
    config: LocalDbConfig,
    tables: Vec<String>,
) -> Result<HashMap<String, TableLastIdInfo>, String> {
    let conn_str = build_connection_string(&config);
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    // Ambil semua primary key untuk seluruh tabel di schema lokal sekaligus
    let pk_query = "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND COLUMN_KEY = 'PRI' ORDER BY ORDINAL_POSITION ASC";
    let pk_rows = sqlx::query(pk_query).fetch_all(&pool).await.unwrap_or_default();
    let mut table_pk_map: HashMap<String, String> = HashMap::new();
    for r in pk_rows {
        if let (Ok(tbl), Ok(col)) = (r.try_get::<String, _>(0), r.try_get::<String, _>(1)) {
            table_pk_map.entry(tbl).or_insert(col);
        }
    }

    let mut result = HashMap::new();

    for table_name in tables {
        if !table_exists(&pool, &table_name).await.unwrap_or(false) {
            continue;
        }

        let effective_pk = if let Some(pk) = table_pk_map.get(&table_name) {
            pk.clone()
        } else {
            let cols = get_local_table_columns(&pool, &table_name).await.unwrap_or_default();
            if cols.is_empty() {
                continue;
            }
            if let Some(found_pk) = cols.iter().find(|c| c.eq_ignore_ascii_case("id") || c.to_ascii_lowercase().ends_with("_id") || c.to_ascii_lowercase().starts_with("id_")).cloned() {
                found_pk
            } else if let Some(first_col) = cols.first().cloned() {
                first_col
            } else {
                continue;
            }
        };

        let safe_table = match sanitize_identifier(&table_name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let safe_pk = match sanitize_identifier(&effective_pk) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let query = format!("SELECT MAX({}) AS max_id FROM {}", safe_pk, safe_table);
        let row_opt = sqlx::query(&query).fetch_optional(&pool).await.ok().flatten();

        let mut last_id_val = None;
        if let Some(r) = row_opt {
            if let Ok(val) = r.try_get::<u64, _>(0) {
                last_id_val = Some(serde_json::Value::Number(val.into()));
            } else if let Ok(val) = r.try_get::<i64, _>(0) {
                last_id_val = Some(serde_json::Value::Number(val.into()));
            } else if let Ok(val) = r.try_get::<f64, _>(0) {
                if let Some(num) = serde_json::Number::from_f64(val) {
                    last_id_val = Some(serde_json::Value::Number(num));
                }
            } else if let Ok(val) = r.try_get::<String, _>(0) {
                last_id_val = Some(serde_json::Value::String(val));
            }
        }

        result.insert(table_name.clone(), TableLastIdInfo {
            table: table_name,
            primary_key: effective_pk,
            last_id: last_id_val,
        });
    }

    pool.close().await;
    Ok(result)
}

/// Delete local rows newer than the last server-confirmed primary key.
#[tauri::command]
pub async fn delete_local_rows_after_id(
    config: LocalDbConfig,
    table_name: String,
    primary_key: String,
    last_synced_id: serde_json::Value,
) -> Result<u64, String> {
    // Proteksi ketat: Jangan hapus jika last_synced_id bernilai 0, "0", kosong, atau non-positif
    let is_valid_id = match &last_synced_id {
        serde_json::Value::Number(num) => {
            num.as_i64().map(|n| n > 0).unwrap_or(false)
                || num.as_f64().map(|f| f > 0.0).unwrap_or(false)
        }
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            !trimmed.is_empty() && trimmed != "0"
        }
        _ => false,
    };

    if !is_valid_id {
        return Ok(0);
    }

    let conn_str = build_connection_string(&config);
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    if !table_exists(&pool, &table_name).await.unwrap_or(false) {
        pool.close().await;
        return Ok(0);
    }

    let cols = get_local_table_columns(&pool, &table_name).await.unwrap_or_default();
    let pk_exists = cols.iter().any(|c| c.eq_ignore_ascii_case(&primary_key));
    if !pk_exists {
        pool.close().await;
        return Ok(0);
    }

    let safe_table = sanitize_identifier(&table_name)?;
    let safe_pk = sanitize_identifier(&primary_key)?;

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    // Nonaktifkan Foreign Key Checks agar tidak terjadi Foreign Key Constraint error saat DELETE pada relasi tabel
    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=0").execute(&mut *conn).await;

    let query = format!("DELETE FROM {} WHERE {} > ?", safe_table, safe_pk);
    let result = match last_synced_id {
        serde_json::Value::Number(num) => {
            if let Some(n) = num.as_i64() {
                sqlx::query(&query).bind(n).execute(&mut *conn).await
            } else if let Some(n) = num.as_u64() {
                sqlx::query(&query).bind(n as i64).execute(&mut *conn).await
            } else if let Some(f) = num.as_f64() {
                sqlx::query(&query).bind(f).execute(&mut *conn).await
            } else {
                sqlx::query(&query).bind(num.to_string()).execute(&mut *conn).await
            }
        }
        serde_json::Value::String(value) => sqlx::query(&query).bind(value).execute(&mut *conn).await,
        _ => {
            let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1").execute(&mut *conn).await;
            drop(conn);
            pool.close().await;
            return Err("Last synced ID harus angka atau teks".to_string());
        }
    };

    // Aktifkan kembali Foreign Key Checks
    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1").execute(&mut *conn).await;

    let result = result.map_err(|e| format!("Gagal menghapus data lokal setelah {}: {}", primary_key, e))?;

    drop(conn);
    pool.close().await;
    Ok(result.rows_affected())
}
