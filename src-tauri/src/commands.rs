use serde::{Deserialize, Serialize};
use sqlx::{mysql::MySqlPoolOptions, Column, Row};
use std::collections::HashMap;

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

/// Sanitize SQL identifier (table names & column names) to avoid SQL injection
fn sanitize_identifier(ident: &str) -> Result<String, String> {
    let trimmed = ident.trim().trim_matches('`');
    if trimmed.is_empty() {
        return Err("Nama tabel atau kolom tidak boleh kosong".to_string());
    }
    if !trimmed.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("Karakter tidak valid pada identifier SQL: '{}'", ident));
    }
    Ok(format!("`{}`", trimmed))
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

/// Sync array of JSON objects to local MySQL via bulk ON DUPLICATE KEY UPDATE (Upsert)
#[tauri::command]
pub async fn sync_to_local_db(
    config: LocalDbConfig,
    table_name: String,
    primary_key: String,
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
    let safe_pk = sanitize_identifier(&primary_key)?;
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

    let mut safe_cols = Vec::new();
    for col in &raw_cols {
        safe_cols.push(sanitize_identifier(col)?);
    }

    let batch_size = 100;
    let mut total_affected: u64 = 0;
    let total_rows = rows.len();

    for batch in rows.chunks(batch_size) {
        let mut query = format!("INSERT INTO {} ({}) VALUES ", safe_table, safe_cols.join(", "));
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
            query.push_str(&format!(" ON DUPLICATE KEY UPDATE {}=VALUES({})", safe_pk, safe_pk));
        }

        let res = sqlx::query(&query)
            .execute(&pool)
            .await
            .map_err(|e| format!("Gagal memproses query bulk upsert: {}", e))?;

        total_affected += res.rows_affected();
    }

    pool.close().await;

    Ok(SyncResult {
        success: true,
        message: format!("Berhasil mensinkronkan {} baris ke database lokal.", total_rows),
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

/// Helper function to format JSON value safely into SQL literal
fn format_json_value_for_sql(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => if *b { "1".to_string() } else { "0".to_string() },
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
