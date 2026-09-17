use serde::{Deserialize, Serialize};
use sqlx::{
    mysql::{MySqlPool, MySqlPoolOptions},
    Row,
};
use std::collections::HashMap;
use tauri::Emitter;


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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CleanupWatermark {
    pub primary_key: String,
    pub last_synced_id: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CleanupLogPayload {
    r#type: String,
    message: String,
    timestamp: String,
}

fn cleanup_timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn cleanup_emit_log(app: &tauri::AppHandle, log_type: &str, message: impl Into<String>) {
    let msg = message.into();
    println!("[CLEANUP] [{}]: {}", log_type, msg);
    let _ = app.emit(
        "pma-log",
        CleanupLogPayload {
            r#type: log_type.to_string(),
            message: msg,
            timestamp: cleanup_timestamp(),
        },
    );
}

/// Batch cleanup: delete local rows beyond server-confirmed ID for multiple tables.
/// Uses a single pool, batched DELETE with LIMIT, parallel processing, and progress events.
#[tauri::command]
pub async fn batch_cleanup_incremental(
    app: tauri::AppHandle,
    config: LocalDbConfig,
    watermarks: HashMap<String, CleanupWatermark>,
    batch_size: Option<u64>,
    concurrency: Option<usize>,
) -> Result<HashMap<String, u64>, String> {
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let effective_batch = batch_size.unwrap_or(50_000).max(1_000);
    let effective_concurrency = concurrency.unwrap_or(3).max(1).min(8);

    if watermarks.is_empty() {
        return Ok(HashMap::new());
    }

    let conn_str = build_connection_string(&config);
    let pool = Arc::new(
        MySqlPoolOptions::new()
            .max_connections((effective_concurrency as u32 + 2).min(10))
            .connect(&conn_str)
            .await
            .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?,
    );

    cleanup_emit_log(
        &app,
        "info",
        format!(
            "🧹 Memulai batch cleanup {} tabel (batch={}, paralel={})",
            watermarks.len(),
            effective_batch,
            effective_concurrency
        ),
    );

    let semaphore = Arc::new(Semaphore::new(effective_concurrency));
    let app_arc = Arc::new(app);
    let mut handles = Vec::new();

    for (table_name, wm) in watermarks.into_iter() {
        // Validate last_synced_id
        let is_valid = match &wm.last_synced_id {
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
        if !is_valid {
            continue;
        }

        let pool = Arc::clone(&pool);
        let sem = Arc::clone(&semaphore);
        let app_handle = Arc::clone(&app_arc);
        let pk = wm.primary_key.clone();
        let last_id = wm.last_synced_id.clone();
        let batch_sz = effective_batch;

        let handle = tokio::spawn(async move {
            let _permit = sem
                .acquire()
                .await
                .map_err(|e| format!("Semaphore error: {}", e))?;

            // Check table & column existence
            if !table_exists(&pool, &table_name).await.unwrap_or(false) {
                return Ok::<(String, u64), String>((table_name, 0));
            }
            let cols = get_local_table_columns(&pool, &table_name)
                .await
                .unwrap_or_default();
            if !cols.iter().any(|c| c.eq_ignore_ascii_case(&pk)) {
                return Ok((table_name, 0));
            }

            let safe_table = sanitize_identifier(&table_name)?;
            let safe_pk = sanitize_identifier(&pk)?;

            let mut conn = pool
                .acquire()
                .await
                .map_err(|e| format!("Pool acquire gagal untuk '{}': {}", table_name, e))?;

            // Disable FK checks
            let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=0")
                .execute(&mut *conn)
                .await;

            // Count rows to delete
            let count_query =
                format!("SELECT COUNT(*) FROM {} WHERE {} > ?", safe_table, safe_pk);
            let total_to_delete: u64 = match &last_id {
                serde_json::Value::Number(num) => {
                    let n = num.as_i64().unwrap_or(0);
                    sqlx::query(&count_query)
                        .bind(n)
                        .fetch_one(&mut *conn)
                        .await
                        .and_then(|r| r.try_get::<i64, _>(0).map(|v| v as u64))
                        .unwrap_or(0)
                }
                serde_json::Value::String(s) => sqlx::query(&count_query)
                    .bind(s.clone())
                    .fetch_one(&mut *conn)
                    .await
                    .and_then(|r| r.try_get::<i64, _>(0).map(|v| v as u64))
                    .unwrap_or(0),
                _ => 0,
            };

            if total_to_delete == 0 {
                let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1")
                    .execute(&mut *conn)
                    .await;
                return Ok((table_name, 0));
            }

            let total_batches =
                ((total_to_delete as f64) / (batch_sz as f64)).ceil() as u64;

            cleanup_emit_log(
                &app_handle,
                "info",
                format!(
                    "🗑️ [Cleanup '{}'] {} row akan dihapus dalam ~{} batch",
                    table_name,
                    format_number_u64(total_to_delete),
                    total_batches
                ),
            );

            // Batched delete loop
            let delete_query = format!(
                "DELETE FROM {} WHERE {} > ? ORDER BY {} ASC LIMIT {}",
                safe_table, safe_pk, safe_pk, batch_sz
            );
            let mut total_deleted: u64 = 0;
            let mut batch_idx: u64 = 0;

            loop {
                let affected = match &last_id {
                    serde_json::Value::Number(num) => {
                        let n = num.as_i64().unwrap_or(0);
                        sqlx::query(&delete_query)
                            .bind(n)
                            .execute(&mut *conn)
                            .await
                            .map(|r| r.rows_affected())
                            .unwrap_or(0)
                    }
                    serde_json::Value::String(s) => sqlx::query(&delete_query)
                        .bind(s.clone())
                        .execute(&mut *conn)
                        .await
                        .map(|r| r.rows_affected())
                        .unwrap_or(0),
                    _ => 0,
                };

                if affected == 0 {
                    break;
                }

                total_deleted += affected;
                batch_idx += 1;

                let pct = if total_to_delete > 0 {
                    ((total_deleted as f64 / total_to_delete as f64) * 100.0).min(100.0)
                } else {
                    100.0
                };

                cleanup_emit_log(
                    &app_handle,
                    "info",
                    format!(
                        "🗑️ [Cleanup '{}'] Batch {}/{}: {} / {} row dihapus ({:.0}%)",
                        table_name,
                        batch_idx,
                        total_batches,
                        format_number_u64(total_deleted),
                        format_number_u64(total_to_delete),
                        pct
                    ),
                );

                // Small yield between batches to prevent CPU hogging
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }

            // Re-enable FK checks
            let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1")
                .execute(&mut *conn)
                .await;

            if total_deleted > 0 {
                cleanup_emit_log(
                    &app_handle,
                    "warning",
                    format!(
                        "✅ [Cleanup '{}'] Selesai: {} row dihapus untuk sinkron dengan server",
                        table_name,
                        format_number_u64(total_deleted)
                    ),
                );
            }

            Ok((table_name, total_deleted))
        });

        handles.push(handle);
    }

    // Collect results
    let mut result_map = HashMap::new();
    for handle in handles {
        match handle.await {
            Ok(Ok((table, count))) => {
                result_map.insert(table, count);
            }
            Ok(Err(e)) => {
                cleanup_emit_log(&app_arc, "error", format!("Cleanup error: {}", e));
            }
            Err(e) => {
                cleanup_emit_log(
                    &app_arc,
                    "error",
                    format!("Cleanup task panic: {}", e),
                );
            }
        }
    }

    let total_all: u64 = result_map.values().sum();
    cleanup_emit_log(
        &app_arc,
        "success",
        format!(
            "🧹 Batch cleanup selesai: {} row dihapus dari {} tabel",
            format_number_u64(total_all),
            result_map.len()
        ),
    );

    pool.close().await;
    Ok(result_map)
}

fn format_number_u64(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            result.push('.');
        }
        result.push(b as char);
    }
    result
}

/// Get list of table names currently in local MySQL database
#[tauri::command]
pub async fn get_local_tables(config: LocalDbConfig) -> Result<Vec<String>, String> {
    let conn_str = build_connection_string(&config);
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    let rows = sqlx::query("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME ASC")
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Gagal membaca daftar tabel lokal: {}", e))?;

    pool.close().await;

    let mut tables = Vec::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String, _>(0) {
            tables.push(name);
        }
    }
    Ok(tables)
}

/// Drop specified tables from local MySQL database
#[tauri::command]
pub async fn drop_local_tables(config: LocalDbConfig, tables: Vec<String>) -> Result<Vec<String>, String> {
    if tables.is_empty() {
        return Ok(Vec::new());
    }

    let conn_str = build_connection_string(&config);
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=0").execute(&mut *conn).await;

    let mut dropped = Vec::new();
    for table_name in tables {
        let safe_table = match sanitize_identifier(&table_name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let query = format!("DROP TABLE IF EXISTS {}", safe_table);
        if let Err(e) = sqlx::query(&query).execute(&mut *conn).await {
            eprintln!("Gagal drop tabel lokal {}: {}", table_name, e);
        } else {
            dropped.push(table_name);
        }
    }

    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1").execute(&mut *conn).await;
    drop(conn);
    pool.close().await;

    Ok(dropped)
}
