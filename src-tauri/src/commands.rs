use serde::{Deserialize, Serialize};
use sqlx::{
    mysql::{MySqlPool, MySqlPoolOptions},
    Column, Row,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalDbConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResult {
    pub success: bool,
    pub message: String,
    pub rows_processed: usize,
    pub rows_inserted_or_updated: u64,
}

/// Helper function to build MySQL connection URL
fn build_connection_string(config: &LocalDbConfig) -> String {
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
/// Allows any character including spaces — backticks within the name are escaped.
fn sanitize_identifier(ident: &str) -> Result<String, String> {
    let trimmed = ident.trim().trim_matches('`');
    if trimmed.is_empty() {
        return Err("Nama tabel atau kolom tidak boleh kosong".to_string());
    }
    // Escape any backtick characters within the identifier itself
    let escaped = trimmed.replace('`', "``");
    Ok(format!("`{}`", escaped))
}

async fn table_exists(pool: &MySqlPool, table_name: &str) -> Result<bool, String> {
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

async fn get_local_table_columns(
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

fn infer_mysql_column_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "LONGTEXT",
        serde_json::Value::Bool(_) => "TINYINT(1)",
        serde_json::Value::Number(n) => {
            if n.is_i64() {
                "BIGINT"
            } else {
                "DOUBLE"
            }
        }
        serde_json::Value::String(_) => "LONGTEXT",
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => "JSON",
    }
}

async fn rebuild_local_table(
    pool: &MySqlPool,
    safe_table: &str,
    safe_pk: &str,
    table_name: &str,
    primary_key: Option<&String>,
    sample_obj: &serde_json::Map<String, serde_json::Value>,
    raw_cols: &[String],
) -> Result<(), String> {
    let mut definitions = Vec::new();
    for col_name in raw_cols {
        let safe_col = sanitize_identifier(col_name)?;
        let inferred_type =
            infer_mysql_column_type(sample_obj.get(col_name).unwrap_or(&serde_json::Value::Null));
        definitions.push(format!("{} {} NULL", safe_col, inferred_type));
    }

    let pk_def = if let Some(primary_key) = primary_key {
        if raw_cols.iter().any(|col| col == primary_key) {
            format!(", PRIMARY KEY ({})", safe_pk)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let create_query = format!(
        "CREATE TABLE {} ({}){}",
        safe_table,
        definitions.join(", "),
        pk_def
    );

    sqlx::query(&format!("DROP TABLE IF EXISTS {}", safe_table))
        .execute(pool)
        .await
        .map_err(|e| {
            format!(
                "Gagal menghapus tabel lokal '{}' sebelum rebuild: {}",
                table_name, e
            )
        })?;

    sqlx::query(&create_query)
        .execute(pool)
        .await
        .map_err(|e| {
            format!(
                "Gagal membuat ulang tabel lokal '{}' secara otomatis: {}",
                table_name, e
            )
        })?;

    Ok(())
}

async fn ensure_local_table_exists(
    pool: &MySqlPool,
    safe_table: &str,
    safe_pk: &str,
    table_name: &str,
    primary_key: Option<&String>,
    sample_obj: &serde_json::Map<String, serde_json::Value>,
    raw_cols: &[String],
) -> Result<(), String> {
    if table_exists(pool, table_name).await? {
        let local_cols = get_local_table_columns(pool, table_name).await?;
        let local_set: HashSet<String> = local_cols
            .iter()
            .map(|col| col.to_ascii_lowercase())
            .collect();
        let remote_set: HashSet<String> = raw_cols
            .iter()
            .map(|col| col.to_ascii_lowercase())
            .collect();

        if local_set != remote_set {
            rebuild_local_table(
                pool,
                safe_table,
                safe_pk,
                table_name,
                primary_key,
                sample_obj,
                raw_cols,
            )
            .await?;
        }

        return Ok(());
    }

    rebuild_local_table(
        pool,
        safe_table,
        safe_pk,
        table_name,
        primary_key,
        sample_obj,
        raw_cols,
    )
    .await
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::new();
        for c in s.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
                _ => {
                    for b in c.to_string().bytes() {
                        result.push_str(&format!("%{:02X}", b));
                    }
                }
            }
        }
        result
    }
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

    Ok(format!("Koneksi berhasil! Versi Server: {}", row.0))
}

/// Get latest ID or maximum value of primary key from local table
#[tauri::command]
pub async fn get_last_local_id(
    config: LocalDbConfig,
    table_name: String,
    primary_key: String,
) -> Result<Option<serde_json::Value>, String> {
    let safe_table = sanitize_identifier(&table_name)?;
    let safe_pk = sanitize_identifier(&primary_key)?;
    let conn_str = build_connection_string(&config);

    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    let query = format!("SELECT MAX({}) AS max_id FROM {}", safe_pk, safe_table);

    let row = sqlx::query(&query)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("Gagal query MAX({}): {}", primary_key, e))?;

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

/// Delete local rows newer than the last server-confirmed primary key.
#[tauri::command]
pub async fn delete_local_rows_after_id(
    config: LocalDbConfig,
    table_name: String,
    primary_key: String,
    last_synced_id: serde_json::Value,
) -> Result<u64, String> {
    let safe_table = sanitize_identifier(&table_name)?;
    let safe_pk = sanitize_identifier(&primary_key)?;
    let conn_str = build_connection_string(&config);
    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    let query = format!("DELETE FROM {} WHERE {} > ?", safe_table, safe_pk);
    let result = match last_synced_id {
        serde_json::Value::Number(value) => sqlx::query(&query).bind(value.to_string()).execute(&pool).await,
        serde_json::Value::String(value) => sqlx::query(&query).bind(value).execute(&pool).await,
        _ => return Err("Last synced ID harus angka atau teks".to_string()),
    }.map_err(|e| format!("Gagal menghapus data lokal setelah {}: {}", primary_key, e))?;

    pool.close().await;
    Ok(result.rows_affected())
}

/// Get the maximum updated_at timestamp from a local table.
/// Returns None if the table doesn't exist or has no updated_at column.
#[tauri::command]
pub async fn get_local_max_updated_at(
    config: LocalDbConfig,
    table_name: String,
) -> Result<Option<String>, String> {
    let safe_table = sanitize_identifier(&table_name)?;
    let conn_str = build_connection_string(&config);

    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    // Check whether updated_at column exists in this table
    let col_check_query = format!(
        "SELECT COUNT(*) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{}' AND COLUMN_NAME = 'updated_at'",
        table_name.replace('\'', "''")
    );
    let col_row = sqlx::query(&col_check_query)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("Gagal memeriksa kolom updated_at: {}", e))?;

    let has_updated_at = col_row
        .and_then(|r| r.try_get::<i64, _>(0).ok())
        .unwrap_or(0)
        > 0;

    if !has_updated_at {
        pool.close().await;
        return Ok(None);
    }

    let query = format!(
        "SELECT MAX(updated_at) AS max_updated_at FROM {}",
        safe_table
    );
    let row = sqlx::query(&query)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("Gagal query MAX(updated_at): {}", e))?;

    pool.close().await;

    if let Some(r) = row {
        if let Ok(val) = r.try_get::<String, _>(0) {
            return Ok(Some(val));
        }
    }

    Ok(None)
}

/// Sync array of JSON objects to local MySQL via bulk ON DUPLICATE KEY UPDATE (Upsert)
/// Automatically disables FOREIGN_KEY_CHECKS for the duration of the insert to prevent
/// constraint errors when syncing tables with foreign key references.
#[tauri::command]
pub async fn sync_to_local_db(
    config: LocalDbConfig,
    table_name: String,
    primary_key: Option<String>,
    rows: Vec<serde_json::Value>,
) -> Result<SyncResult, String> {
    if rows.is_empty() {
        return Ok(SyncResult {
            success: true,
            message: "Tidak ada data baru untuk disinkronkan.".to_string(),
            rows_processed: 0,
            rows_inserted_or_updated: 0,
        });
    }

    let safe_table = sanitize_identifier(&table_name)?;
    let safe_pk = match primary_key.as_deref() {
        Some(pk) => sanitize_identifier(pk)?,
        None => String::new(),
    };
    let conn_str = build_connection_string(&config);

    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    let sample_obj = match rows[0].as_object() {
        Some(obj) => obj,
        None => {
            pool.close().await;
            return Err("Format baris data tidak valid (bukan JSON Object)".to_string());
        }
    };

    let mut raw_cols: Vec<String> = sample_obj.keys().cloned().collect();
    raw_cols.sort();

    if raw_cols.is_empty() {
        pool.close().await;
        return Err("Data baris tidak memiliki kolom/field".to_string());
    }

    let primary_key_ref = primary_key.as_ref();
    ensure_local_table_exists(
        &pool,
        &safe_table,
        &safe_pk,
        &table_name,
        primary_key_ref,
        sample_obj,
        &raw_cols,
    )
    .await?;

    let mut safe_cols = Vec::new();
    for col in &raw_cols {
        safe_cols.push(sanitize_identifier(col)?);
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Gagal memulai transaksi MySQL lokal: {}", e))?;

    // Nonaktifkan FK checks dan strict SQL mode pada transaksi ini
    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=0")
        .execute(&mut *tx)
        .await;
    let _ = sqlx::query("SET SESSION sql_mode=''")
        .execute(&mut *tx)
        .await;

    let batch_size = 500;
    let mut total_affected: u64 = 0;
    let total_rows = rows.len();
    let mut upsert_error: Option<String> = None;

    'batch_loop: for batch in rows.chunks(batch_size) {
        let mut query = format!(
            "INSERT INTO {} ({}) VALUES ",
            safe_table,
            safe_cols.join(", ")
        );
        let mut value_clauses = Vec::new();

        for row_val in batch {
            let obj = match row_val.as_object() {
                Some(o) => o,
                None => continue,
            };

            let mut row_values = Vec::new();
            for col_name in &raw_cols {
                let val = obj.get(col_name).unwrap_or(&serde_json::Value::Null);
                row_values.push(format_json_value_for_sql(val));
            }
            value_clauses.push(format!("({})", row_values.join(", ")));
        }

        if value_clauses.is_empty() {
            continue;
        }

        query.push_str(&value_clauses.join(", "));

        if !safe_pk.trim().is_empty() {
            let mut update_clauses = Vec::new();
            for col_name in &raw_cols {
                let safe_col = sanitize_identifier(col_name)?;
                if safe_col != safe_pk {
                    update_clauses.push(format!("{}=VALUES({})", safe_col, safe_col));
                }
            }

            if !update_clauses.is_empty() {
                query.push_str(" ON DUPLICATE KEY UPDATE ");
                query.push_str(&update_clauses.join(", "));
            } else {
                query.push_str(&format!(
                    " ON DUPLICATE KEY UPDATE {}=VALUES({})",
                    safe_pk, safe_pk
                ));
            }
        }

        match sqlx::query(&query).execute(&mut *tx).await {
            Ok(res) => total_affected += res.rows_affected(),
            Err(e) => {
                upsert_error = Some(format!("Gagal memproses query bulk upsert: {}", e));
                break 'batch_loop;
            }
        }
    }

    if let Some(err) = upsert_error {
        let _ = tx.rollback().await;
        pool.close().await;
        return Err(err);
    }

    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1")
        .execute(&mut *tx)
        .await;
    let _ = sqlx::query("SET SESSION sql_mode=DEFAULT")
        .execute(&mut *tx)
        .await;

    tx.commit()
        .await
        .map_err(|e| format!("Gagal meng-commit data ke MySQL lokal: {}", e))?;

    pool.close().await;

    if let Some(err) = upsert_error {
        return Err(err);
    }

    Ok(SyncResult {
        success: true,
        message: format!(
            "Berhasil mensinkronkan {} baris ke database lokal.",
            total_rows
        ),
        rows_processed: total_rows,
        rows_inserted_or_updated: total_affected,
    })
}

/// Fetch sample preview rows from local table
#[tauri::command]
pub async fn get_local_table_preview(
    config: LocalDbConfig,
    table_name: String,
    limit: u32,
) -> Result<Vec<HashMap<String, serde_json::Value>>, String> {
    let safe_table = sanitize_identifier(&table_name)?;
    let conn_str = build_connection_string(&config);

    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    let effective_limit = if limit == 0 || limit > 100 { 20 } else { limit };
    let query = format!("SELECT * FROM {} LIMIT {}", safe_table, effective_limit);

    let rows = sqlx::query(&query)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("Gagal mengambil data preview: {}", e))?;

    pool.close().await;

    let mut result = Vec::new();

    for row in rows {
        let mut map = HashMap::new();
        for col in row.columns() {
            let col_name = col.name().to_string();
            let val: serde_json::Value = if let Ok(v) = row.try_get::<i64, _>(col.name()) {
                serde_json::Value::Number(v.into())
            } else if let Ok(v) = row.try_get::<f64, _>(col.name()) {
                serde_json::Number::from_f64(v)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else if let Ok(v) = row.try_get::<String, _>(col.name()) {
                serde_json::Value::String(v)
            } else if let Ok(v) = row.try_get::<bool, _>(col.name()) {
                serde_json::Value::Bool(v)
            } else {
                serde_json::Value::Null
            };
            map.insert(col_name, val);
        }
        result.push(map);
    }

    Ok(result)
}

/// Truncate local table for Fresh Sync mode
/// Automatically disables FOREIGN_KEY_CHECKS before TRUNCATE and re-enables after.
#[tauri::command]
pub async fn truncate_local_table(
    config: LocalDbConfig,
    table_name: String,
) -> Result<String, String> {
    let safe_table = sanitize_identifier(&table_name)?;
    let conn_str = build_connection_string(&config);

    let pool = MySqlPoolOptions::new()
        .max_connections(2)
        .connect(&conn_str)
        .await
        .map_err(|e| format!("Koneksi ke MySQL lokal gagal: {}", e))?;

    if !table_exists(&pool, &table_name).await? {
        pool.close().await;
        return Ok(format!(
            "Tabel lokal '{}' tidak ada, skip TRUNCATE.",
            table_name
        ));
    }

    // Disable FK checks agar TRUNCATE tidak gagal karena relasi foreign key
    sqlx::query("SET FOREIGN_KEY_CHECKS=0")
        .execute(&pool)
        .await
        .map_err(|e| format!("Gagal menonaktifkan FOREIGN_KEY_CHECKS: {}", e))?;

    let query = format!("TRUNCATE TABLE {}", safe_table);

    let truncate_result = sqlx::query(&query).execute(&pool).await.map_err(|e| {
        format!(
            "Gagal mengosongkan (TRUNCATE) tabel lokal {}: {}",
            table_name, e
        )
    });

    // Selalu re-enable FK checks, bahkan jika TRUNCATE gagal
    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1").execute(&pool).await;

    pool.close().await;

    // Propagate error setelah FK checks di-restore
    truncate_result?;

    Ok(format!(
        "Tabel lokal '{}' berhasil dikosongkan (TRUNCATE).",
        table_name
    ))
}

/// Helper function to format JSON value safely into SQL literal
fn format_json_value_for_sql(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('\0', "\\0")
                .replace('\n', "\\n")
                .replace('\r', "\\r");
            format!("'{}'", escaped)
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            let s = val.to_string();
            let escaped = s
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('\0', "\\0")
                .replace('\n', "\\n")
                .replace('\r', "\\r");
            format!("'{}'", escaped)
        }
    }
}
