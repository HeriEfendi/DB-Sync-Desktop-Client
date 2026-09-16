use crate::commands::LocalDbConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::Emitter;
use tokio::process::Command;

pub static USER_CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static WORKER_ABORT_REQUESTED: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn cancel_pma_export() {
    USER_CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    WORKER_ABORT_REQUESTED.store(true, Ordering::SeqCst);
}

pub fn is_user_cancelled() -> bool {
    USER_CANCEL_REQUESTED.load(Ordering::SeqCst)
}

pub fn is_aborted() -> bool {
    USER_CANCEL_REQUESTED.load(Ordering::SeqCst) || WORKER_ABORT_REQUESTED.load(Ordering::SeqCst)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PmaExportConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub database: String,
    pub tables: Vec<String>,
    pub sync_mode: Option<String>,
    pub row_limit: Option<usize>,
    pub throttle_ms: Option<u64>,
    /// Map nama_tabel -> nama_kolom_primary_key (opsional, per tabel)
    pub table_primary_keys: Option<HashMap<String, String>>,
    /// Fallback primary key global (dipakai kalau tabel tidak ada di table_primary_keys)
    pub primary_key: Option<String>,
    /// Riwayat per tabel: ID dan waktu terakhir yang sudah dikonfirmasi dari server.
    pub incremental_watermarks: Option<HashMap<String, IncrementalWatermark>>,
    /// Set nama tabel yang memiliki kolom updated_at di server.
    pub tables_with_updated_at: Option<std::collections::HashSet<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IncrementalWatermark {
    pub last_synced_id: serde_json::Value,
    pub last_sync_time: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogPayload {
    pub r#type: String, // "info" | "success" | "warning" | "error"
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProgressPayload {
    pub current_table_index: usize,
    pub total_tables: usize,
    pub current_table_name: String,
    pub rows_synced_current_table: usize,
    pub total_synced_all_tables: usize,
    pub status: String,
}

fn current_timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn emit_log(app: &tauri::AppHandle, log_type: &str, message: impl Into<String>) {
    let msg = message.into();
    let norm_type = if log_type == "warn" { "warning" } else { log_type };

    let noisy_http = norm_type == "info"
        && [
            "Inisialisasi HTTP Session",
            "PMA final URL setelah redirect",
            "HTML index.php",
            "Redirect HTML ditemukan",
            "Resolved redirect URL",
            "effective_base_url diperbarui",
            "Re-fetch dari",
            "CSRF token diekstrak",
            "Melakukan otentikasi login PMA",
            "POST login ke",
            "Login response:",
            "Token baru dari login response",
            "POST request ke export",
            "Endpoint PMA valid ditemukan",
            "Export menerima",
        ]
        .iter()
        .any(|prefix| msg.starts_with(prefix) || msg.contains(prefix));

    let noisy_fallback =
        norm_type == "warning" && (msg.contains("mengembalikan HTML response: 404 Not Found") || msg.contains("tidak menghasilkan PK"));

    if noisy_http || noisy_fallback {
        println!("[PMA-LOG] [{}]: {}", norm_type, msg);
        return;
    }

    println!("[PMA-LOG] [{}]: {}", norm_type, msg);
    let _ = app.emit(
        "pma-log",
        LogPayload {
            r#type: norm_type.to_string(),
            message: msg,
            timestamp: current_timestamp(),
        },
    );
}

fn emit_progress(
    app: &tauri::AppHandle,
    current_index: usize,
    total: usize,
    table_name: &str,
    current_rows: usize,
    total_rows: usize,
    status: &str,
) {
    let _ = app.emit(
        "pma-progress",
        ProgressPayload {
            current_table_index: current_index,
            total_tables: total,
            current_table_name: table_name.to_string(),
            rows_synced_current_table: current_rows,
            total_synced_all_tables: total_rows,
            status: status.to_string(),
        },
    );
}

/// Extract CSRF token from PMA HTML content
fn extract_csrf_token(html: &str) -> Option<String> {
    if let Some(caps) = regex_find_token(html) {
        return Some(caps);
    }

    let token_keys = [
        "name=\"token\" value=\"",
        "\"token\":\"",
        "token=",
        "set_session=",
    ];

    for key in token_keys {
        for line in html.lines() {
            if line.contains("token") || line.contains("set_session") {
                if let Some(idx) = line.find(key) {
                    let rest = &line[idx + key.len()..];
                    let val: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                        .collect();
                    if val.len() >= 16 {
                        return Some(val);
                    }
                }
            }
        }
    }

    None
}

fn regex_find_token(html: &str) -> Option<String> {
    for line in html.lines() {
        if line.contains("token") || line.contains("set_session") {
            if line.contains("value=\"") {
                for part in line.split("value=\"").skip(1) {
                    if let Some(end) = part.find('"') {
                        let val = part[..end].trim();
                        if val.len() >= 16
                            && val
                                .chars()
                                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                        {
                            return Some(val.to_string());
                        }
                    }
                }
            }
            if line.contains("value='") {
                for part in line.split("value='").skip(1) {
                    if let Some(end) = part.find('\'') {
                        let val = part[..end].trim();
                        if val.len() >= 16
                            && val
                                .chars()
                                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                        {
                            return Some(val.to_string());
                        }
                    }
                }
            }
            if line.contains("\"token\":\"") {
                for part in line.split("\"token\":\"").skip(1) {
                    if let Some(end) = part.find('"') {
                        let val = part[..end].trim();
                        if val.len() >= 16 {
                            return Some(val.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Authenticate with remote phpMyAdmin server and get cookie-store client & CSRF token
async fn authenticate_pma(
    pma_config: &PmaExportConfig,
    app: &tauri::AppHandle,
) -> Result<(reqwest::Client, String, String), String> {
    let base_url = pma_config.url.trim().trim_end_matches('/').to_string();
    emit_log(
        app,
        "info",
        format!("Inisialisasi HTTP Session ke PMA: {}", base_url),
    );

    let client = reqwest::Client::builder()
        .cookie_store(true)
        .gzip(false)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) DB-Sync-Client/1.0")
        .timeout(Duration::from_secs(300))
        .tcp_keepalive(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| format!("Gagal membina Client HTTP: {}", e))?;

    let index_url = format!("{}/index.php", base_url);
    let resp = client
        .get(&index_url)
        .send()
        .await
        .map_err(|e| format!("Gagal menghubungi PMA ({}): {}", index_url, e))?;

    let final_url = resp.url().to_string();
    emit_log(
        app,
        "info",
        format!("PMA final URL setelah redirect: {}", final_url),
    );

    let mut effective_base_url = if final_url.contains("/index.php") {
        final_url
            .split("/index.php")
            .next()
            .unwrap_or(&base_url)
            .to_string()
    } else {
        base_url.clone()
    };

    let mut html = resp
        .text()
        .await
        .map_err(|e| format!("Gagal membaca response HTML dari PMA: {}", e))?;
    emit_log(
        app,
        "info",
        format!(
            "HTML index.php len={}, preview={}",
            html.len(),
            &html[..html.len().min(200)]
        ),
    );

    // Check for HTML meta refresh or JS redirect (e.g. redirecting to ./public/ or cPanel subpath)
    if let Some(redirect_path) = find_html_redirect(&html) {
        emit_log(
            app,
            "info",
            format!("Redirect HTML ditemukan: {}", redirect_path),
        );
        let resolved_url = resolve_relative_url(&index_url, &redirect_path);
        emit_log(
            app,
            "info",
            format!("Resolved redirect URL: {}", resolved_url),
        );

        // Update effective_base_url to the new subpath (e.g. https://host/public)
        effective_base_url = if resolved_url.contains("/index.php") {
            resolved_url
                .split("/index.php")
                .next()
                .unwrap_or(&resolved_url)
                .to_string()
        } else {
            resolved_url.trim_end_matches('/').to_string()
        };
        emit_log(
            app,
            "info",
            format!("effective_base_url diperbarui: {}", effective_base_url),
        );

        // Re-fetch from the actual PMA location
        let new_index_url = format!("{}/index.php", effective_base_url);
        if let Ok(r) = client.get(&new_index_url).send().await {
            if let Ok(new_html) = r.text().await {
                emit_log(
                    app,
                    "info",
                    format!("Re-fetch dari {}: len={}", new_index_url, new_html.len()),
                );
                html = new_html;
            }
        }
    }

    let mut csrf_token = extract_csrf_token(&html).unwrap_or_default();
    emit_log(
        app,
        "info",
        format!(
            "CSRF token diekstrak: '{}' (len={})",
            &csrf_token[..csrf_token.len().min(16)],
            csrf_token.len()
        ),
    );

    if !pma_config.username.is_empty() {
        emit_log(
            app,
            "info",
            format!(
                "Melakukan otentikasi login PMA untuk user '{}'...",
                pma_config.username
            ),
        );
        let login_url = format!("{}/index.php?route=/login", effective_base_url);
        let mut form = vec![
            ("pma_username", pma_config.username.as_str()),
            ("pma_password", pma_config.password.as_str()),
            ("server", "1"),
            ("target", "index.php"),
        ];
        if !pma_config.database.is_empty() {
            form.push(("db", pma_config.database.as_str()));
        }
        if !csrf_token.is_empty() {
            form.push(("token", csrf_token.as_str()));
        }

        emit_log(app, "info", format!("POST login ke: {}", login_url));

        let login_resp = client.post(&login_url).form(&form).send().await;

        let login_resp = match login_resp {
            Ok(r) => r,
            Err(_) => {
                let alt_login_url = format!("{}/index.php", effective_base_url);
                emit_log(
                    app,
                    "warn",
                    format!("Login route=/login gagal, mencoba: {}", alt_login_url),
                );
                client
                    .post(&alt_login_url)
                    .form(&form)
                    .send()
                    .await
                    .map_err(|e| format!("Gagal login ke PMA: {}", e))?
            }
        };

        let login_status = login_resp.status();
        let login_final_url = login_resp.url().to_string();
        let login_html = login_resp
            .text()
            .await
            .map_err(|e| format!("Gagal membaca response login: {}", e))?;

        emit_log(
            app,
            "info",
            format!(
                "Login response: status={}, final_url={}, len={}, preview={}",
                login_status.as_u16(),
                login_final_url,
                login_html.len(),
                &login_html[..login_html.len().min(300)]
            ),
        );

        if let Some(new_token) = extract_csrf_token(&login_html) {
            emit_log(
                app,
                "info",
                format!(
                    "Token baru dari login response: '{}' (len={})",
                    &new_token[..new_token.len().min(16)],
                    new_token.len()
                ),
            );
            csrf_token = new_token;
        } else {
            emit_log(
                app,
                "warn",
                "Tidak ada token baru di login response, menggunakan token lama",
            );
        }

        if login_html.contains("Access denied")
            || login_html.contains("Cannot log in to the MySQL server")
        {
            return Err(
                "Login PMA Gagal: Username atau Password salah atau akses ditolak.".to_string(),
            );
        }

        // Check if login was actually successful by looking for authenticated page indicators
        let login_ok = !login_html.contains("pma_username")
            && !login_html.contains("\"name\":\"pma_username\"")
            && (login_html.contains("main_pane_left")
                || login_html.contains("navigation_tree")
                || login_html.contains("pma-core")
                || login_html.contains("\"success\":true")
                || login_html.contains("db=")
                || !csrf_token.is_empty());

        if login_ok {
            emit_log(app, "success", "Otentikasi login PMA berhasil.");
        } else {
            emit_log(app, "warn", "Login mungkin gagal — halaman login masih tampil. Melanjutkan dengan cookies yang ada...");
        }
    }

    Ok((client, effective_base_url, csrf_token))
}

fn find_html_redirect(html: &str) -> Option<String> {
    for line in html.lines() {
        if line.to_lowercase().contains("refresh") || line.contains("window.location") {
            if let Some(start) = line.find("url=") {
                let rest = &line[start + 4..];
                let end = rest
                    .find('"')
                    .or_else(|| rest.find('\''))
                    .unwrap_or(rest.len());
                return Some(rest[..end].trim().to_string());
            }
            let start_opt = line
                .find("window.location=")
                .or_else(|| line.find("window.location ="));
            if let Some(start) = start_opt {
                if let Some(quote_start) =
                    line[start..].find('"').or_else(|| line[start..].find('\''))
                {
                    let rest = &line[start + quote_start + 1..];
                    if let Some(quote_end) = rest.find('"').or_else(|| rest.find('\'')) {
                        return Some(rest[..quote_end].trim().to_string());
                    }
                }
            }
        }
    }
    None
}

fn resolve_relative_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.trim_end_matches('/').to_string();
    }

    // Get directory of base URL (strip the filename part, e.g. /index.php)
    let base_dir = if base.ends_with('/') {
        base.trim_end_matches('/').to_string()
    } else {
        // Find the last '/' after the protocol (avoid stripping https://)
        let protocol_end = base.find("://").map(|i| i + 3).unwrap_or(0);
        match base[protocol_end..].rfind('/') {
            Some(offset) => base[..protocol_end + offset].to_string(),
            None => base.to_string(),
        }
    };

    if relative.starts_with('/') {
        // Absolute path — keep only the origin (scheme + host)
        let protocol_end = base_dir.find("://").map(|i| i + 3).unwrap_or(0);
        let origin_end = base_dir[protocol_end..]
            .find('/')
            .map(|i| i + protocol_end)
            .unwrap_or(base_dir.len());
        return format!(
            "{}{}",
            &base_dir[..origin_end],
            relative.trim_end_matches('/')
        );
    }

    // Relative path (may start with ./ or just a name)
    let clean = relative.trim_start_matches("./").trim_end_matches('/');
    format!("{}/{}", base_dir, clean)
}

/// Fetch tables list from remote PMA
pub async fn fetch_pma_tables(
    pma_config: &PmaExportConfig,
    app: &tauri::AppHandle,
) -> Result<Vec<String>, String> {
    if !pma_config.tables.is_empty() {
        return Ok(pma_config.tables.clone());
    }

    let (client, base_url, csrf_token) = authenticate_pma(pma_config, app).await?;
    let db = &pma_config.database;

    emit_log(
        app,
        "info",
        format!("Mengambil daftar tabel dari database '{}'...", db),
    );

    let mut found_tables: Vec<String> = Vec::new();

    // === PRIMARY: AJAX SQL via /index.php?route=/sql ===
    let sql_queries = vec![
        format!("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{}' ORDER BY TABLE_NAME LIMIT 100000", db.replace('\'', "''")),
        format!("SHOW TABLES FROM `{}`", db.replace('`', "``")),
        "SHOW TABLES".to_string(),
    ];

    let sql_endpoints = vec![
        format!("{}/index.php?route=/sql", base_url),
        format!("{}/sql.php", base_url),
    ];

    'outer: for sql_url in &sql_endpoints {
        for query in &sql_queries {
            let mut form_data: Vec<(&str, String)> = vec![
                ("db", db.clone()),
                ("table", String::new()),
                ("server", "1".to_string()),
                ("sql_query", query.clone()),
                ("sql_delimiter", ";".to_string()),
                ("ajax_request", "true".to_string()),
                ("ajax_page_request", "true".to_string()),
                ("submit_query", "Go".to_string()),
                ("session_max_rows", "all".to_string()),
                ("max_rows", "100000".to_string()),
                ("limit", "100000".to_string()),
            ];
            if !csrf_token.is_empty() {
                form_data.push(("token", csrf_token.clone()));
            }

            emit_log(
                app,
                "info",
                format!(
                    "Mencoba AJAX SQL ke '{}': {}",
                    sql_url,
                    &query[..query.len().min(60)]
                ),
            );

            let resp_result = client
                .post(sql_url)
                .header("X-Requested-With", "XMLHttpRequest")
                .header("Content-Type", "application/x-www-form-urlencoded")
                .form(&form_data)
                .send()
                .await;

            let resp = match resp_result {
                Ok(r) => r,
                Err(e) => {
                    emit_log(
                        app,
                        "warn",
                        format!("AJAX SQL request gagal ke '{}': {}", sql_url, e),
                    );
                    continue;
                }
            };

            let status = resp.status();
            let raw_text = resp.text().await.unwrap_or_default();
            emit_log(
                app,
                "info",
                format!(
                    "AJAX SQL response status={}, len={}, preview={}",
                    status.as_u16(),
                    raw_text.len(),
                    &raw_text[..raw_text.len().min(300)]
                ),
            );

            // Try parse JSON
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&raw_text) {
                let success = json_val
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !success {
                    let err_msg = json_val
                        .get("error")
                        .or_else(|| json_val.get("message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    emit_log(app, "warn", format!("AJAX SQL error dari PMA: {}", err_msg));
                    continue;
                }

                let mut tables = extract_tables_from_json(&json_val, db);
                if !tables.is_empty() {
                    emit_log(
                        app,
                        "success",
                        format!(
                            "AJAX SQL berhasil, {} tabel ditemukan pada batch awal",
                            tables.len()
                        ),
                    );

                    // PAGINATION LOOP: If PMA hard-capped the result (e.g. 250 rows limit per response), fetch next pages via OFFSET
                    let mut offset = tables.len();
                    while tables.len() % 250 == 0 {
                        let paged_query = format!(
                            "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{}' ORDER BY TABLE_NAME LIMIT 1000 OFFSET {}",
                            db.replace('\'', "''"),
                            offset
                        );
                        let mut paged_form = vec![
                            ("db", db.clone()),
                            ("table", String::new()),
                            ("server", "1".to_string()),
                            ("sql_query", paged_query),
                            ("sql_delimiter", ";".to_string()),
                            ("ajax_request", "true".to_string()),
                            ("ajax_page_request", "true".to_string()),
                            ("submit_query", "Go".to_string()),
                            ("session_max_rows", "all".to_string()),
                            ("max_rows", "100000".to_string()),
                            ("limit", "100000".to_string()),
                        ];
                        if !csrf_token.is_empty() {
                            paged_form.push(("token", csrf_token.clone()));
                        }

                        emit_log(
                            app,
                            "info",
                            format!("Mengekstrak halaman lanjutan offset {}...", offset),
                        );
                        if let Ok(paged_resp) = client
                            .post(sql_url)
                            .header("X-Requested-With", "XMLHttpRequest")
                            .header("Content-Type", "application/x-www-form-urlencoded")
                            .form(&paged_form)
                            .send()
                            .await
                        {
                            if let Ok(paged_text) = paged_resp.text().await {
                                if let Ok(paged_json) =
                                    serde_json::from_str::<serde_json::Value>(&paged_text)
                                {
                                    let new_page_tables = extract_tables_from_json(&paged_json, db);
                                    if new_page_tables.is_empty() {
                                        break;
                                    }
                                    let count_before = tables.len();
                                    for t in new_page_tables {
                                        if !tables.contains(&t) {
                                            tables.push(t);
                                        }
                                    }
                                    if tables.len() == count_before {
                                        break;
                                    }
                                    offset = tables.len();
                                    continue;
                                }
                            }
                        }
                        break;
                    }

                    found_tables = tables;
                    break 'outer;
                }

                emit_log(
                    app,
                    "warn",
                    "AJAX SQL success=true tapi tidak ada tabel di response JSON",
                );
            } else {
                emit_log(
                    app,
                    "warn",
                    format!(
                        "AJAX SQL response bukan JSON valid (len={})",
                        raw_text.len()
                    ),
                );
            }
        }
    }

    // === FALLBACK: HTML scraping dari database structure & export pages ===
    if found_tables.is_empty() {
        emit_log(
            app,
            "warn",
            "AJAX SQL tidak menghasilkan tabel, mencoba HTML scraping...",
        );

        let db_encoded = urlencoding::encode(db);
        let token_param = if !csrf_token.is_empty() {
            format!("&token={}", urlencoding::encode(&csrf_token))
        } else {
            String::new()
        };

        // Prioritize export page because export pages list ALL tables without pagination
        let candidate_urls = vec![
            format!(
                "{}/index.php?route=/database/export&db={}{}",
                base_url, db_encoded, token_param
            ),
            format!("{}/export.php?db={}{}", base_url, db_encoded, token_param),
            format!(
                "{}/index.php?route=/database/structure&db={}{}",
                base_url, db_encoded, token_param
            ),
            format!("{}/db_structure.php?db={}{}", base_url, db_encoded, token_param),
            format!("{}/index.php?db={}{}", base_url, db_encoded, token_param),
        ];

        for url in &candidate_urls {
            if let Ok(resp) = client.get(url).send().await {
                let html = resp.text().await.unwrap_or_default();
                let mut tables = extract_tables_from_html(&html);

                // If structure page returns 250 tables, perform HTML pagination (pos=250, pos=500...)
                if tables.len() >= 250 && url.contains("structure") {
                    let mut pos = 250;
                    loop {
                        let pos_url = format!("{}&pos={}", url, pos);
                        if let Ok(p_resp) = client.get(&pos_url).send().await {
                            let p_html = p_resp.text().await.unwrap_or_default();
                            let p_tables = extract_tables_from_html(&p_html);
                            if p_tables.is_empty() {
                                break;
                            }
                            let count_before = tables.len();
                            for t in p_tables {
                                if !tables.contains(&t) {
                                    tables.push(t);
                                }
                            }
                            if tables.len() == count_before {
                                break;
                            }
                            pos += 250;
                        } else {
                            break;
                        }
                    }
                }

                if !tables.is_empty() {
                    emit_log(
                        app,
                        "info",
                        format!(
                            "HTML scraping dari '{}' menemukan {} tabel",
                            url,
                            tables.len()
                        ),
                    );
                    found_tables = tables;
                    break;
                } else {
                    emit_log(
                        app,
                        "warn",
                        format!(
                            "HTML scraping dari '{}': tidak ada tabel (len={})",
                            url,
                            html.len()
                        ),
                    );
                }
            }
        }
    }

    if !found_tables.is_empty() {
        found_tables.sort();
        found_tables.dedup();
        emit_log(
            app,
            "success",
            format!(
                "Ditemukan {} tabel dari database '{}'.",
                found_tables.len(),
                db
            ),
        );
        return Ok(found_tables);
    }

    Err(format!(
        "Tidak ada tabel yang ditemukan di database '{}'. Pastikan nama database benar, credentials memiliki akses, dan coba lakukan 'Tes Koneksi PMA' terlebih dahulu.",
        db
    ))
}

/// Fetch primary key column names for all tables in the database in a single INFORMATION_SCHEMA query.
/// Returns a map of table_name -> primary_key_column. Tables without a PK are omitted.
async fn fetch_all_primary_keys(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    db: &str,
    _app: &tauri::AppHandle,
) -> HashMap<String, String> {
    let mut pk_map: HashMap<String, String> = HashMap::new();

    let safe_db = db.replace('\'', "''");
    let query = format!(
        "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE \
         WHERE TABLE_SCHEMA = '{}' AND CONSTRAINT_NAME = 'PRIMARY' \
         ORDER BY TABLE_NAME, ORDINAL_POSITION LIMIT 10000",
        safe_db
    );

    let sql_endpoints = vec![
        format!("{}/index.php?route=/sql", base_url),
        format!("{}/sql.php", base_url),
    ];

    for sql_url in &sql_endpoints {
        let mut form_data: Vec<(&str, String)> = vec![
            ("db", db.to_string()),
            ("table", String::new()),
            ("server", "1".to_string()),
            ("sql_query", query.clone()),
            ("sql_delimiter", ";".to_string()),
            ("ajax_request", "true".to_string()),
            ("ajax_page_request", "true".to_string()),
            ("submit_query", "Go".to_string()),
            ("session_max_rows", "all".to_string()),
            ("max_rows", "100000".to_string()),
        ];
        if !csrf_token.is_empty() {
            form_data.push(("token", csrf_token.to_string()));
        }

        let resp = match client
            .post(sql_url)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form_data)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => {
                continue;
            }
        };

        let raw_text = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let json_val = match serde_json::from_str::<serde_json::Value>(&raw_text) {
            Ok(v) => v,
            Err(_) => {
                continue;
            }
        };

        let success = json_val
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            continue;
        }

        // --- Format 1: fields + rows (paling umum di PMA modern) ---
        // fields: [{name: "TABLE_NAME"}, {name: "COLUMN_NAME"}]
        // rows: [["users", "id"], ["orders", "order_id"], ...]  -- array atau object
        if let (Some(fields), Some(rows)) = (
            json_val.get("fields").and_then(|v| v.as_array()),
            json_val.get("rows").and_then(|v| v.as_array()),
        ) {
            // Cari index kolom TABLE_NAME dan COLUMN_NAME dari fields
            let col_names: Vec<String> = fields.iter().map(|f| {
                f.get("name").or_else(|| f.get("Name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("").to_uppercase()
            }).collect();

            let tbl_idx = col_names.iter().position(|c| c == "TABLE_NAME");
            let col_idx = col_names.iter().position(|c| c == "COLUMN_NAME");

            if let (Some(ti), Some(ci)) = (tbl_idx, col_idx) {
                for row in rows {
                    let (tbl, col) = if let Some(arr) = row.as_array() {
                        let t = arr.get(ti).and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let c = arr.get(ci).and_then(|v| v.as_str()).unwrap_or("").to_string();
                        (t, c)
                    } else if let Some(obj) = row.as_object() {
                        // row bisa juga object keyed by field name
                        let t = obj.get("TABLE_NAME").or_else(|| obj.get("table_name"))
                            .and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let c = obj.get("COLUMN_NAME").or_else(|| obj.get("column_name"))
                            .and_then(|v| v.as_str()).unwrap_or("").to_string();
                        (t, c)
                    } else {
                        continue;
                    };
                    if !tbl.is_empty() && !col.is_empty() {
                        pk_map.entry(tbl).or_insert(col);
                    }
                }
            }
        }

        // --- Format 2: data array (PMA lama) ---
        if pk_map.is_empty() {
            let try_keys = ["data", "dataset", "results", "query_data"];
            'fmt2: for key in &try_keys {
                if let Some(rows) = json_val.get(key).and_then(|v| v.as_array()) {
                    for row in rows {
                        let (tbl, col) = if let Some(arr) = row.as_array() {
                            let t = arr.first().and_then(|v| v.as_str()
                                .or_else(|| v.get("v").and_then(|x| x.as_str()))).unwrap_or("").to_string();
                            let c = arr.get(1).and_then(|v| v.as_str()
                                .or_else(|| v.get("v").and_then(|x| x.as_str()))).unwrap_or("").to_string();
                            (t, c)
                        } else if let Some(obj) = row.as_object() {
                            let t = obj.get("TABLE_NAME").or_else(|| obj.get("table_name"))
                                .and_then(|v| v.as_str()
                                    .or_else(|| v.get("v").and_then(|x| x.as_str()))).unwrap_or("").to_string();
                            let c = obj.get("COLUMN_NAME").or_else(|| obj.get("column_name"))
                                .and_then(|v| v.as_str()
                                    .or_else(|| v.get("v").and_then(|x| x.as_str()))).unwrap_or("").to_string();
                            (t, c)
                        } else {
                            continue;
                        };
                        if !tbl.is_empty() && !col.is_empty() {
                            pk_map.entry(tbl).or_insert(col);
                        }
                    }
                    if !pk_map.is_empty() {
                        break 'fmt2;
                    }
                }
            }
        }

        if !pk_map.is_empty() {
            break;
        }
    }

    pk_map
}

async fn fetch_tables_with_updated_at(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    db: &str,
    _app: &tauri::AppHandle,
) -> std::collections::HashSet<String> {
    let mut updated_at_set: std::collections::HashSet<String> = std::collections::HashSet::new();

    let safe_db = db.replace('\'', "''");
    let query = format!(
        "SELECT TABLE_NAME FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = '{}' AND COLUMN_NAME = 'updated_at' LIMIT 10000",
        safe_db
    );

    let sql_endpoints = vec![
        format!("{}/index.php?route=/sql", base_url),
        format!("{}/sql.php", base_url),
    ];

    for sql_url in &sql_endpoints {
        let mut form_data: Vec<(&str, String)> = vec![
            ("db", db.to_string()),
            ("table", String::new()),
            ("server", "1".to_string()),
            ("sql_query", query.clone()),
            ("sql_delimiter", ";".to_string()),
            ("ajax_request", "true".to_string()),
            ("ajax_page_request", "true".to_string()),
            ("submit_query", "Go".to_string()),
            ("session_max_rows", "all".to_string()),
            ("max_rows", "100000".to_string()),
        ];
        if !csrf_token.is_empty() {
            form_data.push(("token", csrf_token.to_string()));
        }

        let resp = match client
            .post(sql_url)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form_data)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let raw_text = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let json_val = match serde_json::from_str::<serde_json::Value>(&raw_text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let success = json_val
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            continue;
        }

        if let (Some(fields), Some(rows)) = (
            json_val.get("fields").and_then(|v| v.as_array()),
            json_val.get("rows").and_then(|v| v.as_array()),
        ) {
            let col_names: Vec<String> = fields.iter().map(|f| {
                f.get("name").or_else(|| f.get("Name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("").to_uppercase()
            }).collect();

            if let Some(tbl_idx) = col_names.iter().position(|c| c == "TABLE_NAME") {
                for row in rows {
                    let tbl = if let Some(arr) = row.as_array() {
                        arr.get(tbl_idx).and_then(|v| v.as_str()).unwrap_or("").to_string()
                    } else if let Some(obj) = row.as_object() {
                        obj.get("TABLE_NAME").or_else(|| obj.get("table_name"))
                            .and_then(|v| v.as_str()).unwrap_or("").to_string()
                    } else {
                        String::new()
                    };
                    if !tbl.is_empty() {
                        updated_at_set.insert(tbl);
                    }
                }
            }
        }

        if !updated_at_set.is_empty() {
            break;
        }
    }

    updated_at_set
}

/// Fetch estimated row counts for all tables in the database from INFORMATION_SCHEMA.TABLES
async fn fetch_table_row_counts(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    db: &str,
    _app: &tauri::AppHandle,
) -> HashMap<String, usize> {
    let mut row_counts: HashMap<String, usize> = HashMap::new();

    let safe_db = db.replace('\'', "''");
    let query = format!(
        "SELECT TABLE_NAME, TABLE_ROWS FROM information_schema.TABLES \
         WHERE LOWER(TABLE_SCHEMA) = LOWER('{}') LIMIT 10000",
        safe_db
    );

    let sql_endpoints = vec![
        format!("{}/index.php?route=/sql", base_url),
        format!("{}/sql.php", base_url),
    ];

    for sql_url in &sql_endpoints {
        let mut form_data: Vec<(&str, String)> = vec![
            ("db", db.to_string()),
            ("table", String::new()),
            ("server", "1".to_string()),
            ("sql_query", query.clone()),
            ("sql_delimiter", ";".to_string()),
            ("ajax_request", "true".to_string()),
            ("ajax_page_request", "true".to_string()),
            ("submit_query", "Go".to_string()),
            ("session_max_rows", "all".to_string()),
            ("max_rows", "100000".to_string()),
        ];
        if !csrf_token.is_empty() {
            form_data.push(("token", csrf_token.to_string()));
        }

        let resp = match client
            .post(sql_url)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form_data)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let raw_text = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let json_val = match serde_json::from_str::<serde_json::Value>(&raw_text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let success = json_val
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            continue;
        }

        if let (Some(fields), Some(rows)) = (
            json_val.get("fields").and_then(|v| v.as_array()),
            json_val.get("rows").and_then(|v| v.as_array()),
        ) {
            let col_names: Vec<String> = fields
                .iter()
                .map(|f| {
                    f.get("name")
                        .or_else(|| f.get("Name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_uppercase()
                })
                .collect();

            let tbl_idx = col_names.iter().position(|c| c == "TABLE_NAME");
            let rows_idx = col_names.iter().position(|c| c == "TABLE_ROWS");

            if let (Some(ti), Some(ri)) = (tbl_idx, rows_idx) {
                for row in rows {
                    if let Some(arr) = row.as_array() {
                        let tbl = arr.get(ti).and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let r_cnt = arr.get(ri).and_then(|v| {
                            if let Some(n) = v.as_u64() {
                                Some(n as usize)
                            } else if let Some(s) = v.as_str() {
                                s.parse::<usize>().ok()
                            } else {
                                None
                            }
                        }).unwrap_or(0);
                        if !tbl.is_empty() {
                            row_counts.insert(tbl, r_cnt);
                        }
                    } else if let Some(obj) = row.as_object() {
                        let tbl = obj.get("TABLE_NAME").or_else(|| obj.get("table_name"))
                            .and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let r_cnt = obj.get("TABLE_ROWS").or_else(|| obj.get("table_rows"))
                            .and_then(|v| {
                                if let Some(n) = v.as_u64() {
                                    Some(n as usize)
                                } else if let Some(s) = v.as_str() {
                                    s.parse::<usize>().ok()
                                } else {
                                    None
                                }
                            }).unwrap_or(0);
                        if !tbl.is_empty() {
                            row_counts.insert(tbl, r_cnt);
                        }
                    }
                }
            }
        }

        if !row_counts.is_empty() {
            break;
        }
    }

    row_counts
}

/// Extract table names from PMA AJAX SQL JSON response
fn extract_tables_from_json(json_val: &serde_json::Value, db: &str) -> Vec<String> {
    let mut tables = Vec::new();

    // Try top-level fields: dataset, data, rows, results
    let try_keys = ["dataset", "data", "rows", "results", "query_data"];
    for key in &try_keys {
        if let Some(arr) = json_val.get(key).and_then(|v| v.as_array()) {
            for row in arr {
                if let Some(name) = extract_table_name_from_row(row, db) {
                    if !tables.contains(&name) {
                        tables.push(name);
                    }
                }
            }
            if !tables.is_empty() {
                return tables;
            }
        }
    }

    // Try with fields+rows style (PMA often returns this)
    if let (Some(fields), Some(rows)) = (
        json_val.get("fields").and_then(|v| v.as_array()),
        json_val.get("rows").and_then(|v| v.as_array()),
    ) {
        let col_names: Vec<String> = fields
            .iter()
            .map(|f| {
                f.get("name")
                    .or_else(|| f.get("Name"))
                    .or_else(|| f.get("Field"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase()
            })
            .collect();

        for row in rows {
            if let Some(arr) = row.as_array() {
                for (i, val) in arr.iter().enumerate() {
                    let col = col_names.get(i).map(|s| s.as_str()).unwrap_or("");
                    if col.contains("table") || col.contains("name") {
                        if let Some(s) = val.as_str() {
                            let name = s.trim().to_string();
                            if name.len() >= 1 && !tables.contains(&name) {
                                tables.push(name);
                            }
                        }
                    }
                }
            } else if let Some(obj) = row.as_object() {
                for (k, v) in obj {
                    let key_lower = k.to_lowercase();
                    if key_lower.contains("table") || key_lower == "name" {
                        if let Some(s) = v.as_str() {
                            let name = s.trim().to_string();
                            if name.len() >= 1 && !tables.contains(&name) {
                                tables.push(name);
                            }
                        }
                    }
                }
            }
        }
        if !tables.is_empty() {
            return tables;
        }
    }

    // Recursive walk in case PMA wraps result in nested structure
    if let Some(obj) = json_val.as_object() {
        for (_k, v) in obj {
            if v.is_object() || v.is_array() {
                let nested = extract_tables_from_json(v, db);
                for t in nested {
                    if !tables.contains(&t) {
                        tables.push(t);
                    }
                }
            }
        }
    }

    tables
}

fn extract_table_name_from_row(row: &serde_json::Value, _db: &str) -> Option<String> {
    let try_keys = [
        "TABLE_NAME",
        "table_name",
        "Tables_in_db",
        "Name",
        "name",
        "Table",
        "table",
    ];
    if let Some(obj) = row.as_object() {
        // try well-known keys first
        for key in &try_keys {
            if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
                let s = val.trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        // try any key containing "table" or "name"
        for (k, v) in obj {
            let kl = k.to_lowercase();
            if kl.contains("table") || kl == "name" {
                if let Some(s) = v.as_str() {
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
    }
    // check if row is an array, first element might be table name
    if let Some(arr) = row.as_array() {
        if let Some(first) = arr.first().and_then(|v| v.as_str()) {
            let s = first.trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

#[tauri::command]
pub async fn get_pma_tables(
    pma_config: PmaExportConfig,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    fetch_pma_tables(&pma_config, &app).await
}

fn extract_tables_from_html(html: &str) -> Vec<String> {
    let mut tables = Vec::new();

    // Valid MySQL table name: starts with letter or underscore, only [a-zA-Z0-9_], length 2-64
    let is_valid_table_name = |name: &str| -> bool {
        let n = name.trim();
        if n.len() < 2 || n.len() > 64 {
            return false;
        }
        let mut chars = n.chars();
        let first = match chars.next() {
            Some(c) => c,
            None => return false,
        };
        // Must start with letter or underscore
        if !first.is_ascii_alphabetic() && first != '_' {
            return false;
        }
        // Rest must be alphanumeric or underscore
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        // Exclude known PMA UI / system values / export options / compatibility modes
        let nl = n.to_lowercase();
        !matches!(
            nl.as_str(),
            "phpmyadmin"
                | "index"
                | "true"
                | "false"
                | "select"
                | "structure"
                | "sql"
                | "export"
                | "import"
                | "checkall"
                | "select_all"
                | "uncheck_all"
                | "none"
                | "null"
                | "yes"
                | "no"
                | "ok"
                | "on"
                | "off"
                | "go"
                | "db"
                | "server"
                | "action"
                | "table"
                | "view"
                | "all"
                | "new"
                | "ansi"
                | "db2"
                | "maxdb"
                | "mssql"
                | "mysql323"
                | "mysql40"
                | "oracle"
                | "traditional"
                | "insert"
                | "replace"
                | "update"
                | "structure_and_data"
                | "texytext"
                | "textext"
                | "toon"
                | "win"
                | "xml"
                | "yaml"
                | "zip"
                | "gzip"
                | "bzip2"
                | "codegen"
                | "csv"
                | "excel"
                | "htmldir"
                | "htmlword"
                | "json"
                | "latex"
                | "mediawiki"
                | "ods"
                | "odt"
                | "pdf"
                | "phparray"
                | "shift_jis"
                | "sjis"
                | "utf8"
                | "utf8mb4"
                | "latin1"
                | "ascii"
                | "quick"
                | "custom"
                | "quick_export"
                | "sendit"
                | "asfile"
        )
    };

    // Pattern 1: data-table="table_name"  ← most reliable, PMA adds this on structure page
    for line in html.lines() {
        if line.contains("data-table=\"") {
            for part in line.split("data-table=\"").skip(1) {
                if let Some(end) = part.find('"') {
                    let name = part[..end].trim();
                    if is_valid_table_name(name) && !tables.contains(&name.to_string()) {
                        tables.push(name.to_string());
                    }
                }
            }
        }
    }

    // Pattern 2: value="..." inside table selection checkboxes / options (export.php / database/export / database/structure)
    let mut inside_table_select = false;
    for line in html.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.contains("<select")
            && (line_lower.contains("table_select")
                || line_lower.contains("table_structure")
                || line_lower.contains("selected_tbl"))
        {
            inside_table_select = true;
        }

        let is_table_line = inside_table_select
            || line_lower.contains("table_select")
            || line_lower.contains("table_structure")
            || line_lower.contains("table_data")
            || line_lower.contains("selected_tbl");

        if is_table_line {
            if line.contains("value=\"") {
                for part in line.split("value=\"").skip(1) {
                    if let Some(end) = part.find('"') {
                        let name = part[..end].trim();
                        if is_valid_table_name(name) && !tables.contains(&name.to_string()) {
                            tables.push(name.to_string());
                        }
                    }
                }
            }
            if line.contains("value='") {
                for part in line.split("value='").skip(1) {
                    if let Some(end) = part.find('\'') {
                        let name = part[..end].trim();
                        if is_valid_table_name(name) && !tables.contains(&name.to_string()) {
                            tables.push(name.to_string());
                        }
                    }
                }
            }
        }

        if inside_table_select && line_lower.contains("</select>") {
            inside_table_select = false;
        }
    }

    // Pattern 3: table=table_name or dbtable=table_name in URLs (query strings)
    for line in html.lines() {
        for key in &["table=", "dbtable="] {
            if line.contains(key) {
                for part in line.split(key).skip(1) {
                    let name: String = part
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if is_valid_table_name(&name) && !tables.contains(&name) {
                        tables.push(name);
                    }
                }
            }
        }
    }

    tables.sort();
    tables.dedup();
    tables
}

fn count_imported_sql_rows(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    let len = bytes.len();
    let mut i = 0;
    let mut in_string = false;
    let mut in_escape = false;

    while i < len {
        if in_escape {
            in_escape = false;
            i += 1;
            continue;
        }
        if bytes[i] == b'\\' {
            in_escape = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'\'' {
            in_string = !in_string;
            i += 1;
            continue;
        }

        if !in_string {
            // Check for VALUES keyword
            if i + 6 <= len && bytes[i..i + 6].eq_ignore_ascii_case(b"VALUES") {
                let mut p = i + 6;
                let mut in_str2 = false;
                let mut in_esc2 = false;
                let mut depth = 0usize;

                while p < len {
                    if in_esc2 {
                        in_esc2 = false;
                        p += 1;
                        continue;
                    }
                    if bytes[p] == b'\\' {
                        in_esc2 = true;
                        p += 1;
                        continue;
                    }
                    if bytes[p] == b'\'' {
                        in_str2 = !in_str2;
                        p += 1;
                        continue;
                    }

                    if !in_str2 {
                        if bytes[p] == b'(' {
                            if depth == 0 {
                                count += 1;
                            }
                            depth += 1;
                        } else if bytes[p] == b')' {
                            if depth > 0 {
                                depth -= 1;
                            }
                        } else if bytes[p] == b';' && depth == 0 {
                            p += 1;
                            break;
                        }
                    }
                    p += 1;
                }
                i = p;
                continue;
            }
        }
        i += 1;
    }

    if count == 0 && (bytes.windows(11).any(|w| w.eq_ignore_ascii_case(b"INSERT INTO")) || bytes.windows(12).any(|w| w.eq_ignore_ascii_case(b"REPLACE INTO"))) {
        count = 1;
    }
    count
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

fn decompress_gzip_bytes(bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    use flate2::read::MultiGzDecoder;
    use std::io::Read;
    let mut decoder = MultiGzDecoder::new(bytes);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

fn format_byte_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

#[derive(Clone)]
struct DownloadedTableData {
    table_name: String,
    response_bytes: Vec<u8>,
    is_gzip: bool,
    table_start: std::time::Instant,
}

/// Helper download low-level payload dari phpMyAdmin server
async fn download_export_payload(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    pma_config: &PmaExportConfig,
    table_name: &str,
    struct_or_data: &str,
    sql_structure: &str,
    sql_create: &str,
    sql_type_val: &str,
    custom_query_opt: Option<String>,
    cached_endpoint: &std::sync::Arc<tokio::sync::Mutex<Option<(String, String)>>>,
    app: &tauri::AppHandle,
    chunk_label: Option<&str>,
) -> Result<Option<DownloadedTableData>, String> {
    let table_start = std::time::Instant::now();
    let (cached_url, cached_comp) = {
        let guard = cached_endpoint.lock().await;
        guard.clone().unzip()
    };

    let default_urls = vec![
        format!("{}/export.php", base_url),
        format!("{}/index.php?route=/export", base_url),
        format!("{}/index.php?route=/database/export", base_url),
        format!("{}/index.php?route=/table/export", base_url),
    ];
    let default_compressions = vec!["gzip".to_string(), "none".to_string()];

    let mut candidates = Vec::new();
    if let (Some(u), Some(c)) = (cached_url, cached_comp) {
        candidates.push((u.clone(), c.clone()));
        if c == "gzip" {
            candidates.push((u.clone(), "none".to_string()));
        }
    }
    for u in &default_urls {
        for c in &default_compressions {
            if !candidates.iter().any(|(cand_u, cand_c)| cand_u == u && cand_c == c) {
                candidates.push((u.clone(), c.clone()));
            }
        }
    }

    let mut last_html_err = String::new();
    let mut last_network_err = String::new();
    let max_retries_per_candidate = 3;

    'candidate_loop: for (export_url, compression_mode) in candidates {
        let is_gz = compression_mode == "gzip";

        let is_structure_only = struct_or_data == "structure";
        let sql_data_val = if is_structure_only { "0" } else { "1" };

        let mut form: Vec<(String, String)> = vec![
            ("db".to_string(), pma_config.database.clone()),
            ("table".to_string(), table_name.to_string()),
            ("table_select[]".to_string(), table_name.to_string()),
            ("table_structure[]".to_string(), table_name.to_string()),
            ("single_table".to_string(), "true".to_string()),
            ("what".to_string(), "sql".to_string()),
            ("export_type".to_string(), "table".to_string()),
            ("export_method".to_string(), "custom".to_string()),
            ("quick_or_custom".to_string(), "custom".to_string()),
            ("output_format".to_string(), "sendit".to_string()),
            ("compression".to_string(), compression_mode.clone()),
            ("asfile".to_string(), "sendit".to_string()),
            (
                "sql_structure_or_data".to_string(),
                struct_or_data.to_string(),
            ),
            ("sql_structure".to_string(), sql_structure.to_string()),
            ("sql_data".to_string(), sql_data_val.to_string()),
            ("sql_create_table".to_string(), sql_create.to_string()),
            ("sql_drop_table".to_string(), "false".to_string()),
            ("sql_if_not_exists".to_string(), "true".to_string()),
            ("sql_auto_increment".to_string(), "1".to_string()),
            ("sql_backquotes".to_string(), "1".to_string()),
            ("sql_type".to_string(), sql_type_val.to_string()),
            ("sql_extended_inserts".to_string(), "true".to_string()),
            ("sql_max_query_size".to_string(), "4194304".to_string()),
            ("sql_disable_fk".to_string(), "true".to_string()),
            ("sql_use_transaction".to_string(), "true".to_string()),
        ];

        if !is_structure_only {
            form.push(("table_data[]".to_string(), table_name.to_string()));
        }

        if let Some(ref limited_query) = custom_query_opt {
            form.push(("sql_query".to_string(), limited_query.clone()));
            form.push(("query_type".to_string(), "table".to_string()));
            form.push(("sql_query_input".to_string(), "true".to_string()));
        }

        if !csrf_token.is_empty() {
            form.push(("token".to_string(), csrf_token.to_string()));
        }

        let mut attempt = 0;
        while attempt < max_retries_per_candidate {
            attempt += 1;
            if is_user_cancelled() {
                return Err("__USER_CANCELLED__".to_string());
            }
            if is_aborted() {
                return Err("__WORKER_ABORTED__".to_string());
            }

            let post_future = client.post(&export_url).form(&form).send();
            tokio::pin!(post_future);

            let send_start = std::time::Instant::now();
            let mut last_wait_log = std::time::Instant::now();
            let mut post_timed_out = false;
            let send_result = loop {
                if is_user_cancelled() {
                    return Err("__USER_CANCELLED__".to_string());
                }
                if is_aborted() {
                    return Err("__WORKER_ABORTED__".to_string());
                }

                tokio::select! {
                    res = &mut post_future => {
                        break Some(res);
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                        let elapsed_wait = send_start.elapsed().as_secs();
                        if elapsed_wait >= 60 {
                            post_timed_out = true;
                            break None;
                        }
                        if elapsed_wait >= 5 && last_wait_log.elapsed().as_secs() >= 5 {
                            let label_str = chunk_label.map(|lbl| format!(" ({})", lbl)).unwrap_or_default();
                            emit_log(
                                app,
                                "info",
                                format!(
                                    "[Tabel '{}'] Menunggu server remote PMA memproses & menyiapkan export SQL{}... ({}s)",
                                    table_name, label_str, elapsed_wait
                                ),
                            );
                            last_wait_log = std::time::Instant::now();
                        }
                    }
                }
            };

            let response = if post_timed_out || send_result.is_none() {
                last_network_err = format!("TIMEOUT: Server remote tidak merespons dalam 60 detik (url: {})", export_url);
                if attempt < max_retries_per_candidate {
                    emit_log(
                        app,
                        "warn",
                        format!(
                            "[Tabel '{}'] TIMEOUT menunggu response dari {} ({}/{}). Mencoba kembali...",
                            table_name, export_url, attempt, max_retries_per_candidate
                        ),
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                } else {
                    return Err(format!("TIMEOUT: Server remote PMA tidak merespons request export dalam 60 detik (url: {})", export_url));
                }
            } else {
                match send_result.unwrap() {
                    Ok(r) => r,
                    Err(e) => {
                        last_network_err = e.to_string();
                        if attempt < max_retries_per_candidate {
                            emit_log(
                                app,
                                "warn",
                                format!(
                                    "[Tabel '{}'] Koneksi ke {} terputus ({}/{}): {}. Mencoba kembali...",
                                    table_name, export_url, attempt, max_retries_per_candidate, e
                                ),
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        } else {
                            continue 'candidate_loop;
                        }
                    }
                }
            };

            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_lowercase();

            if content_type.contains("text/html") {
                let err_html = response.text().await.unwrap_or_default();
                last_html_err = sanitize_html_error(&err_html);
                println!(
                    "[PMA-DEBUG] Endpoint {} (comp={}) mengembalikan HTML response: {}",
                    export_url, compression_mode, last_html_err
                );
                let mut guard = cached_endpoint.lock().await;
                if guard.as_ref().map(|(u, _)| u == &export_url).unwrap_or(false) {
                    *guard = None;
                }
                continue 'candidate_loop;
            }

            use futures_util::StreamExt;
            let mut stream = response.bytes_stream();
            let mut response_bytes = Vec::new();
            let mut last_progress_log = std::time::Instant::now();
            let mut last_logged_bytes = 0usize;
            let mut read_failed = false;
            let mut stream_timed_out = false;

            while let Some(chunk_res) = {
                if is_user_cancelled() {
                    return Err("__USER_CANCELLED__".to_string());
                }
                if is_aborted() {
                    return Err("__WORKER_ABORTED__".to_string());
                }
                match tokio::time::timeout(std::time::Duration::from_secs(60), stream.next()).await {
                    Ok(item) => item,
                    Err(_elapsed) => {
                        stream_timed_out = true;
                        None
                    }
                }
            } {
                match chunk_res {
                    Ok(chunk) => {
                        response_bytes.extend_from_slice(&chunk);
                        if response_bytes.len() >= 3 * 1024 * 1024 && last_progress_log.elapsed().as_secs() >= 3 {
                            let delta_bytes = response_bytes.len() - last_logged_bytes;
                            let delta_secs = last_progress_log.elapsed().as_secs_f64();
                            let speed_mb = (delta_bytes as f64 / (1024.0 * 1024.0)) / delta_secs.max(0.1);
                            let label_str = chunk_label.map(|lbl| format!(" ({})", lbl)).unwrap_or_default();
                            emit_log(
                                app,
                                "info",
                                format!(
                                    "[Tabel '{}'] Mengunduh stream{}: {} diterima (~{:.2} MB/s)...",
                                    table_name,
                                    label_str,
                                    format_byte_size(response_bytes.len()),
                                    speed_mb
                                ),
                            );
                            last_progress_log = std::time::Instant::now();
                            last_logged_bytes = response_bytes.len();
                        }
                    }
                    Err(e) => {
                        last_network_err = format!("error decoding response body: {}", e);
                        emit_log(
                            app,
                            "warn",
                            format!(
                                "[Tabel '{}'] Gagal membaca body dari {} (comp={}, percobaan {}/{}): {}. Mencoba kembali...",
                                table_name, export_url, compression_mode, attempt, max_retries_per_candidate, e
                            ),
                        );
                        read_failed = true;
                        break;
                    }
                }
            }

            if stream_timed_out {
                last_network_err = format!("TIMEOUT: Aliran stream data terhenti lebih dari 60 detik (url: {})", export_url);
                if attempt < max_retries_per_candidate {
                    emit_log(
                        app,
                        "warn",
                        format!(
                            "[Tabel '{}'] TIMEOUT membaca stream dari {} ({}/{}). Mencoba kembali...",
                            table_name, export_url, attempt, max_retries_per_candidate
                        ),
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                } else {
                    return Err(format!("TIMEOUT: Aliran stream data terhenti lebih dari 60 detik (url: {})", export_url));
                }
            }

            if read_failed {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }

            let total_bytes = response_bytes.len();
            if total_bytes == 0 {
                emit_log(
                    app,
                    "warn",
                    format!(
                        "[Tabel '{}'] Response kosong dari {} (comp={}, percobaan {}/{}). Mencoba kembali...",
                        table_name, export_url, compression_mode, attempt, max_retries_per_candidate
                    ),
                );
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }

            let mut guard = cached_endpoint.lock().await;
            if guard.is_none() {
                *guard = Some((export_url.clone(), compression_mode.clone()));
            }

            return Ok(Some(DownloadedTableData {
                table_name: table_name.to_string(),
                response_bytes: response_bytes.to_vec(),
                is_gzip: is_gz,
                table_start,
            }));
        }
    }

    let err_msg = if !last_html_err.is_empty() {
        format!("Export GAGAL untuk tabel '{}'. Semua endpoint PMA mengembalikan HTML Error: {}", table_name, last_html_err)
    } else if !last_network_err.is_empty() {
        format!("Export GAGAL untuk tabel '{}'. Gagal membaca data dari server PMA: {}", table_name, last_network_err)
    } else {
        format!("Export GAGAL untuk tabel '{}'. Tidak ada data yang berhasil diunduh dari server PMA.", table_name)
    };
    Err(err_msg)
}

/// Tahap 1 (Producer): Mengunduh export SQL/GZIP dari phpMyAdmin server ke dalam memory buffer terkompresi
async fn fetch_table_export_stream(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    pma_config: &PmaExportConfig,
    table_name: &str,
    cached_endpoint: &std::sync::Arc<tokio::sync::Mutex<Option<(String, String)>>>,
    table_row_counts: &std::sync::Arc<HashMap<String, usize>>,
    app: &tauri::AppHandle,
    forced_chunk_size: Option<usize>,
) -> Result<Option<DownloadedTableData>, String> {
    if is_user_cancelled() {
        return Err("__USER_CANCELLED__".to_string());
    }
    if is_aborted() {
        return Err("__WORKER_ABORTED__".to_string());
    }

    let table_start = std::time::Instant::now();

    emit_log(
        app,
        "info",
        format!("[Tabel '{}'] Mengunduh export SQL dari server PMA...", table_name),
    );

    let pk_col_opt = pma_config
        .table_primary_keys
        .as_ref()
        .and_then(|m| m.get(table_name))
        .map(|s| s.trim())
        .filter(|pk| !pk.is_empty())
        .or_else(|| pma_config.primary_key.as_deref().map(str::trim).filter(|pk| !pk.is_empty()));

    let has_updated_at = pma_config
        .tables_with_updated_at
        .as_ref()
        .map(|set| set.contains(table_name))
        .unwrap_or(false);

    let has_incremental_watermark = pma_config.sync_mode.as_deref() == Some("incremental")
        && pma_config.incremental_watermarks.as_ref()
            .is_some_and(|watermarks| watermarks.contains_key(table_name));

    // Optimasi: Jika mode incremental dan watermark sudah ada, tetapi tabel TIDAK punya PK dan TIDAK punya updated_at
    if pma_config.sync_mode.as_deref() == Some("incremental") && has_incremental_watermark && pk_col_opt.is_none() && !has_updated_at {
        emit_log(
            app,
            "info",
            format!("[Tabel '{}'] Tidak memiliki kolom Primary Key (ID) maupun 'updated_at'. Di-skip pada sinkronisasi incremental untuk mencegah error.", table_name),
        );
        return Ok(None);
    }

    let is_structure_only = pma_config.sync_mode.as_deref() == Some("structure_only")
        || pma_config.sync_mode.as_deref() == Some("structure");

    let sql_type_val = if pma_config.sync_mode.as_deref() == Some("fresh") || is_structure_only {
        "INSERT"
    } else {
        "REPLACE"
    };

    let is_incremental = pma_config.sync_mode.as_deref() == Some("incremental");
    let est_rows = table_row_counts.get(table_name).copied().unwrap_or(0);
    let effective_row_limit = pma_config.row_limit.filter(|limit| *limit > 0);

    let is_forced_chunk = forced_chunk_size.is_some();
    let should_chunk_initially = !is_structure_only
        && !has_incremental_watermark
        && (is_forced_chunk
            || est_rows >= 100_000
            || effective_row_limit.map(|l| l >= 100_000).unwrap_or(false));

    // Jika tidak perlu chunking di awal, coba unduh langsung (direct export)
    if !should_chunk_initially {
        let struct_or_data = if is_structure_only {
            "structure"
        } else if is_incremental {
            "data"
        } else {
            "structure_and_data"
        };
        let sql_structure = if is_incremental { "0" } else { "1" };
        let sql_create = if is_incremental { "false" } else { "true" };

        let custom_query_opt = if is_structure_only {
            None
        } else if effective_row_limit.is_some() || has_incremental_watermark {
            let limit = effective_row_limit.unwrap_or(usize::MAX);
            let safe_table = table_name.replace('`', "``");

            let limited_query = if let Some(watermark) = pma_config
                .incremental_watermarks
                .as_ref()
                .and_then(|watermarks| watermarks.get(table_name))
                .filter(|_| pma_config.sync_mode.as_deref() == Some("incremental"))
            {
                let last_id_str = match &watermark.last_synced_id {
                    serde_json::Value::Number(value) => Some(value.to_string()),
                    serde_json::Value::String(value) => Some(format!("'{}'", value.replace('\'', "''"))),
                    _ => None,
                };
                let last_sync = watermark.last_sync_time.replace('\'', "''");

                let mut conditions = Vec::new();

                if let (Some(pk_col), Some(last_id)) = (pk_col_opt, last_id_str) {
                    let safe_pk = pk_col.replace('`', "``");
                    conditions.push(format!("`{}` > {}", safe_pk, last_id));
                }

                if has_updated_at && !last_sync.is_empty() {
                    conditions.push(format!("`updated_at` > '{}'", last_sync));
                }

                let where_clause = if !conditions.is_empty() {
                    format!("WHERE {}", conditions.join(" OR "))
                } else {
                    String::new()
                };

                let order_clause = if let Some(pk_col) = pk_col_opt {
                    format!("ORDER BY `{}` ASC", pk_col.replace('`', "``"))
                } else if has_updated_at {
                    "ORDER BY `updated_at` ASC".to_string()
                } else {
                    String::new()
                };

                let limit_clause = if limit != usize::MAX {
                    format!("LIMIT {}", limit)
                } else {
                    String::new()
                };

                format!(
                    "SELECT * FROM `{}` {} {} {}",
                    safe_table, where_clause, order_clause, limit_clause
                ).split_whitespace().collect::<Vec<_>>().join(" ")
            } else {
                let order_clause = if let Some(pk_col) = pk_col_opt {
                    format!("ORDER BY `{}` DESC", pk_col.replace('`', "``"))
                } else {
                    String::new()
                };
                let limit_clause = if limit != usize::MAX {
                    format!("LIMIT {}", limit)
                } else {
                    String::new()
                };
                format!(
                    "SELECT * FROM `{}` {} {}",
                    safe_table, order_clause, limit_clause
                ).split_whitespace().collect::<Vec<_>>().join(" ")
            };
            Some(limited_query)
        } else {
            None
        };

        let direct_res = download_export_payload(
            client,
            base_url,
            csrf_token,
            pma_config,
            table_name,
            struct_or_data,
            sql_structure,
            sql_create,
            sql_type_val,
            custom_query_opt,
            cached_endpoint,
            app,
            None,
        ).await;

        match direct_res {
            Ok(data) => return Ok(data),
            Err(e) if e.contains("TIMEOUT") && !is_structure_only && !has_incremental_watermark => {
                emit_log(
                    app,
                    "warn",
                    format!(
                        "⏱️ [Tabel '{}'] Unduhan langsung TIMEOUT (> 1 menit): Server remote overload atau tabel sangat besar. Beralih otomatis ke MODE CICILAN (100.000 row per chunk) agar SELURUH data tetap terambil lengkap...",
                        table_name
                    ),
                );
                // Lanjut eksekusi mode chunking di bawah
            }
            Err(e) => return Err(e),
        }
    }

    // === MODE CICILAN (CHUNKING) PER-CHUNK TIMEOUT (1 MENIT / 60s) ===
    // Percobaan pertama: 100.000 row, percobaan kedua: 50.000 row, percobaan ketiga: 25.000 row
    let mut chunk_size = forced_chunk_size.unwrap_or(100_000usize);
    let effective_est = effective_row_limit
        .map(|l| if est_rows > 0 { l.min(est_rows) } else { l })
        .unwrap_or(est_rows);
    let total_chunks = if effective_est > 0 {
        (effective_est + chunk_size - 1) / chunk_size
    } else {
        0
    };
    let safe_table = table_name.replace('`', "``");

    let order_clause = if let Some(pk_col) = pk_col_opt {
        format!("ORDER BY `{}` ASC", pk_col.replace('`', "``"))
    } else {
        String::new()
    };

    if total_chunks > 0 {
        emit_log(
            app,
            "info",
            format!(
                "[Tabel '{}'] Mengunduh dalam cicilan {} chunk ({} row per chunk, target ~{} row)...",
                table_name,
                total_chunks,
                format_number(chunk_size),
                format_number(effective_est)
            ),
        );
    } else {
        emit_log(
            app,
            "info",
            format!(
                "[Tabel '{}'] Mengunduh dalam cicilan chunk ({} row per chunk) hingga seluruh data selesai...",
                table_name,
                format_number(chunk_size)
            ),
        );
    }

    let mut combined_sql = Vec::with_capacity(if total_chunks > 0 { (total_chunks.min(50)) * 2 * 1024 * 1024 } else { 4 * 1024 * 1024 });
    let mut chunk_idx = 0usize;
    let mut current_offset = 0usize;
    let mut last_seen_pk: Option<String> = None;
    let max_chunks_by_limit = effective_row_limit.map(|l| (l + chunk_size - 1) / chunk_size).unwrap_or(5000);
    let max_safe_chunks = max_chunks_by_limit.min(5000); // Mendukung hingga 250 juta baris

    while chunk_idx < max_safe_chunks {
        if is_user_cancelled() {
            return Err("__USER_CANCELLED__".to_string());
        }
        if is_aborted() {
            return Err("__WORKER_ABORTED__".to_string());
        }

        let chunk_struct = if chunk_idx == 0 && !is_incremental { "structure_and_data" } else { "data" };
        let chunk_sql_struct = if chunk_idx == 0 && !is_incremental { "1" } else { "0" };
        let chunk_sql_create = if chunk_idx == 0 && !is_incremental { "true" } else { "false" };

        let chunk_label = if total_chunks > 0 {
            let display_total = total_chunks.max(chunk_idx + 1);
            format!("chunk {}/{}", chunk_idx + 1, display_total)
        } else {
            format!("chunk {}", chunk_idx + 1)
        };

        // Percobaan per-chunk (Percobaan 1: 100k, Percobaan 2: 50k, Percobaan 3: 25k jika timeout)
        let mut chunk_attempt = 0;
        let mut chunk_res = None;

        while chunk_attempt < 3 {
            chunk_attempt += 1;
            if is_user_cancelled() {
                return Err("__USER_CANCELLED__".to_string());
            }
            if is_aborted() {
                return Err("__WORKER_ABORTED__".to_string());
            }

            // Langkah 1: Keyset Pagination (Seek Method) jika PK terdeteksi & chunk > 0 untuk kecepatan O(1) instan
            let chunk_query = if let (Some(pk_col), Some(ref last_pk)) = (pk_col_opt, &last_seen_pk) {
                format!(
                    "SELECT * FROM `{}` WHERE `{}` > {} ORDER BY `{}` ASC LIMIT {}",
                    safe_table,
                    pk_col.replace('`', "``"),
                    last_pk,
                    pk_col.replace('`', "``"),
                    chunk_size
                )
            } else {
                format!(
                    "SELECT * FROM `{}` {} LIMIT {} OFFSET {}",
                    safe_table, order_clause, chunk_size, current_offset
                )
            }
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

            match download_export_payload(
                client,
                base_url,
                csrf_token,
                pma_config,
                table_name,
                chunk_struct,
                chunk_sql_struct,
                chunk_sql_create,
                sql_type_val,
                Some(chunk_query),
                cached_endpoint,
                app,
                Some(&chunk_label),
            ).await {
                Ok(res) => {
                    chunk_res = res;
                    break;
                }
                Err(e) if e.contains("TIMEOUT") && chunk_attempt < 3 => {
                    chunk_size = match chunk_attempt {
                        1 => 50_000,
                        2 => 25_000,
                        _ => 25_000,
                    };
                    emit_log(
                        app,
                        "warn",
                        format!(
                            "⏱️ [Tabel '{}'] {} TIMEOUT (> 1 menit) pada percobaan {}/3. Mencoba ulang dengan ukuran chunk lebih kecil ({} row)...",
                            table_name, chunk_label, chunk_attempt, format_number(chunk_size)
                        ),
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        if let Some(chunk_data) = chunk_res {
            let raw_chunk_sql = if chunk_data.is_gzip {
                decompress_gzip_bytes(&chunk_data.response_bytes)
                    .unwrap_or(chunk_data.response_bytes)
            } else {
                chunk_data.response_bytes
            };

            let chunk_len = raw_chunk_sql.len();
            let row_count_in_chunk = count_imported_sql_rows(&raw_chunk_sql);
            let has_insert = raw_chunk_sql.windows(11).any(|w| w == b"INSERT INTO")
                || raw_chunk_sql.windows(12).any(|w| w == b"REPLACE INTO");

            // Update last_seen_pk dari chunk yang baru selesai untuk Keyset Pagination chunk berikutnya
            if let Some(pk_col) = pk_col_opt {
                if let Some(new_pk) = extract_last_pk_value(&raw_chunk_sql, pk_col) {
                    last_seen_pk = Some(new_pk);
                }
            }

            if !combined_sql.is_empty() {
                combined_sql.push(b'\n');
            }
            combined_sql.extend_from_slice(&raw_chunk_sql);

            let display_progress = if total_chunks > 0 {
                format!("Chunk {}/{}", chunk_idx + 1, total_chunks.max(chunk_idx + 1))
            } else {
                format!("Chunk {}", chunk_idx + 1)
            };

            emit_log(
                app,
                "info",
                format!(
                    "[Tabel '{}'] {} selesai diunduh: ~{} row ({})",
                    table_name,
                    display_progress,
                    format_number(row_count_in_chunk),
                    format_byte_size(chunk_len)
                ),
            );

            current_offset += chunk_size;
            chunk_idx += 1;

            // Jika chunk tidak memiliki data INSERT atau row count kosong, berarti seluruh isi tabel telah selesai
            if !has_insert || row_count_in_chunk == 0 {
                break;
            }

            // Jeda throttling singkat antar chunk agar memory & I/O remote server PMA tidak overload
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        } else {
            break;
        }
    }

    Ok(Some(DownloadedTableData {
        table_name: table_name.to_string(),
        response_bytes: combined_sql,
        is_gzip: false,
        table_start,
    }))
}

fn get_mysql_cli_binary() -> &'static str {
    static BINARY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BINARY.get_or_init(|| {
        // 1. Coba perintah di PATH
        if std::process::Command::new("mariadb").arg("--version").output().is_ok() {
            return "mariadb".to_string();
        }
        if std::process::Command::new("mysql").arg("--version").output().is_ok() {
            return "mysql".to_string();
        }

        // 2. Cek lokasi instalasi umum di macOS, Linux, dan Windows
        let candidates = [
            // macOS Homebrew (Apple Silicon)
            "/opt/homebrew/bin/mariadb",
            "/opt/homebrew/bin/mysql",
            "/opt/homebrew/opt/mysql-client/bin/mysql",
            "/opt/homebrew/opt/mariadb/bin/mariadb",
            // macOS Homebrew (Intel) & Standard Unix
            "/usr/local/bin/mariadb",
            "/usr/local/bin/mysql",
            "/usr/local/opt/mysql-client/bin/mysql",
            // macOS Official MySQL Package
            "/usr/local/mysql/bin/mysql",
            // macOS MAMP / XAMPP
            "/Applications/MAMP/Library/bin/mysql",
            "/Applications/XAMPP/xamppfiles/bin/mysql",
            // Linux standard & XAMPP
            "/usr/bin/mariadb",
            "/usr/bin/mysql",
            "/opt/lampp/bin/mysql",
            // Windows XAMPP / Laragon / Official Installer
            r"C:\xampp\mysql\bin\mysql.exe",
            r"C:\xampp\mysql\bin\mariadb.exe",
            r"C:\laragon\bin\mysql\current\bin\mysql.exe",
            r"C:\Program Files\MySQL\MySQL Server 8.4\bin\mysql.exe",
            r"C:\Program Files\MySQL\MySQL Server 8.0\bin\mysql.exe",
            r"C:\Program Files\MariaDB 11.4\bin\mariadb.exe",
            r"C:\Program Files\MariaDB 10.11\bin\mariadb.exe",
        ];

        for path in candidates {
            if std::path::Path::new(path).exists() {
                return path.to_string();
            }
        }

        "mysql".to_string()
    })
}

pub fn get_docker_mysql_cli(container: &str) -> String {
    static DOCKER_CLI_CACHE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    if let Ok(cache) = DOCKER_CLI_CACHE.lock() {
        if let Some(cli) = cache.get(container) {
            return cli.clone();
        }
    }

    let detected = if let Ok(out) = std::process::Command::new("docker")
        .arg("exec")
        .arg(container)
        .arg("mysql")
        .arg("--version")
        .output()
    {
        if out.status.success() {
            "mysql".to_string()
        } else if let Ok(out_m) = std::process::Command::new("docker")
            .arg("exec")
            .arg(container)
            .arg("mariadb")
            .arg("--version")
            .output()
        {
            if out_m.status.success() {
                "mariadb".to_string()
            } else {
                "mysql".to_string()
            }
        } else {
            "mysql".to_string()
        }
    } else {
        "mysql".to_string()
    };

    if let Ok(mut cache) = DOCKER_CLI_CACHE.lock() {
        cache.insert(container.to_string(), detected.clone());
    }

    detected
}


/// Mengekstrak nilai Primary Key terbesar/terakhir dari chunk SQL dump phpMyAdmin
/// untuk mendukung Keyset Pagination (Seek Method) O(1) yang super cepat
fn extract_last_pk_value(sql_bytes: &[u8], pk_column_name: &str) -> Option<String> {
    if sql_bytes.is_empty() || pk_column_name.is_empty() {
        return None;
    }

    let sql_str = match std::str::from_utf8(sql_bytes) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let target_pk = pk_column_name.trim().trim_matches('`').to_lowercase();
    if target_pk.is_empty() {
        return None;
    }

    // Cari posisi statement INSERT INTO atau REPLACE INTO terakhir
    let insert_pos = sql_str
        .rfind("INSERT INTO ")
        .or_else(|| sql_str.rfind("insert into "))
        .or_else(|| sql_str.rfind("REPLACE INTO "))
        .or_else(|| sql_str.rfind("replace into "))?;

    let raw_statement = &sql_str[insert_pos..];
    let semicolon_pos = raw_statement.find(';').unwrap_or(raw_statement.len());
    let statement = &raw_statement[..semicolon_pos];

    // Ekstrak daftar kolom di dalam tanda kurung sebelum VALUES
    let values_keyword_pos = statement.find("VALUES").or_else(|| statement.find("values"))?;
    let header_part = &statement[..values_keyword_pos];

    // Cari daftar kolom di header, contoh: `table` (`id`, `name`, `status`)
    let pk_index = if let (Some(open_p), Some(close_p)) = (header_part.find('('), header_part.rfind(')')) {
        if open_p < close_p {
            let cols_str = &header_part[open_p + 1..close_p];
            let cols: Vec<String> = cols_str
                .split(',')
                .map(|c| c.trim().trim_matches('`').trim_matches('\'').trim_matches('"').to_lowercase())
                .collect();
            cols.iter().position(|c| c == &target_pk).unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };

    // Cari tuple baris terakhir di bagian VALUES (...)
    let values_part = &statement[values_keyword_pos..];
    let last_close_paren = values_part.rfind(')')?;
    let last_open_paren = values_part[..last_close_paren].rfind('(')?;

    let row_str = &values_part[last_open_paren + 1..last_close_paren];

    // Parse nilai-nilai dalam tuple baris dengan memperhatikan petik string (') dan escaping (\)
    let mut values = Vec::new();
    let mut current_val = String::new();
    let mut in_quote = false;
    let mut is_escaped = false;

    for ch in row_str.chars() {
        if is_escaped {
            current_val.push(ch);
            is_escaped = false;
            continue;
        }
        if ch == '\\' {
            current_val.push(ch);
            is_escaped = true;
            continue;
        }
        if ch == '\'' {
            in_quote = !in_quote;
            current_val.push(ch);
            continue;
        }
        if ch == ',' && !in_quote {
            values.push(current_val.trim().to_string());
            current_val.clear();
            continue;
        }
        current_val.push(ch);
    }
    if !current_val.is_empty() {
        values.push(current_val.trim().to_string());
    }

    if let Some(val) = values.get(pk_index) {
        let clean = val.trim();
        if !clean.is_empty() && clean.to_uppercase() != "NULL" {
            return Some(clean.to_string());
        }
    }

    None
}

/// Menentukan apakah error MySQL disebabkan oleh struktur skema yang tidak cocok
fn is_schema_mismatch_error(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("unknown column")
        || lower.contains("column count doesn't match")
        || lower.contains("table") && lower.contains("doesn't exist")
        || lower.contains("doesn't have a default value")
        || lower.contains("data truncated")
        || lower.contains("cannot be null")
        || lower.contains("error 1054")
        || lower.contains("error 1136")
        || lower.contains("error 1146")
        || lower.contains("error 1364")
        || lower.contains("error 1265")
        || lower.contains("error 1048")
        || lower.contains("incorrect integer value")
        || lower.contains("incorrect decimal value")
        || lower.contains("incorrect date")
        || lower.contains("error 1366")
        || lower.contains("error 1292")
}

/// Ekstrak ringkasan baris error MySQL untuk log yang bersih
fn extract_short_schema_error(err: &str) -> String {
    for line in err.lines() {
        if line.contains("ERROR ") || line.contains("Unknown column") || line.contains("doesn't exist") {
            return line.trim().to_string();
        }
    }
    err.lines().next().unwrap_or(err).trim().to_string()
}

/// Tahap 2 (Consumer Internal): Mendekompresi GZIP secara real-time dan mengalirkan langsung ke STDIN MySQL Lokal
async fn import_table_to_local_internal(
    pma_config: &PmaExportConfig,
    local_config: &LocalDbConfig,
    data: DownloadedTableData,
    force_fresh: bool,
    app: &tauri::AppHandle,
) -> Result<usize, String> {
    if is_user_cancelled() {
        return Err("__USER_CANCELLED__".to_string());
    }
    if is_aborted() {
        return Err("__WORKER_ABORTED__".to_string());
    }

    let table_name = &data.table_name;
    let table_start = data.table_start;
    let response_bytes = data.response_bytes;
    let _is_gzip = data.is_gzip;
    let total_bytes = response_bytes.len();

    let host = if local_config.host.is_empty() {
        "127.0.0.1"
    } else {
        &local_config.host
    };
    let port_str = local_config.port.to_string();
    let db_name = &local_config.database;
    let is_fresh = force_fresh || pma_config.sync_mode.as_deref() == Some("fresh");

    let is_structure_only = pma_config.sync_mode.as_deref() == Some("structure_only")
        || pma_config.sync_mode.as_deref() == Some("structure");

    if total_bytes == 0 {
        emit_log(
            app,
            "info",
            format!(
                "[Tabel '{}'] 0 baris (kosong / tidak ada perubahan data). Melewati proses import lokal.",
                table_name
            ),
        );
        return Ok(0);
    }

    let is_actual_gzip = response_bytes.len() >= 2 && response_bytes[0] == 0x1f && response_bytes[1] == 0x8b;
    let sql_bytes = if is_actual_gzip {
        decompress_gzip_bytes(&response_bytes).unwrap_or(response_bytes)
    } else {
        response_bytes
    };

    let imported_rows = count_imported_sql_rows(&sql_bytes);

    if is_structure_only {
        emit_log(
            app,
            "info",
            format!(
                "[Tabel '{}'] DDL Struktur diterima ({}). Mengeksekusi pembuatan tabel di MySQL lokal...",
                table_name,
                format_byte_size(total_bytes)
            ),
        );
    } else {
        emit_log(
            app,
            "info",
            format!(
                "[Tabel '{}'] Data diterima {} (format: {}). Memulai impor ke MySQL lokal...",
                table_name,
                format_byte_size(total_bytes),
                if is_actual_gzip { "GZIP" } else { "SQL/raw" }
            ),
        );
    }

    let sql_bytes_arc = std::sync::Arc::new(sql_bytes);
    let total_sql_len = sql_bytes_arc.len();

    let max_retries = 3;
    let mut attempt = 0;
    let mut last_error = String::new();
    let cli_bin = get_mysql_cli_binary();
    let use_docker = local_config.use_docker && !local_config.docker_container.trim().is_empty();
    let container_name = local_config.docker_container.trim();

    while attempt < max_retries {
        attempt += 1;

        if is_user_cancelled() {
            return Err("__USER_CANCELLED__".to_string());
        }
        if is_aborted() {
            return Err("__WORKER_ABORTED__".to_string());
        }

        let docker_cli = if use_docker {
            get_docker_mysql_cli(container_name)
        } else {
            String::new()
        };

        let mut cmd = if use_docker {
            let mut c = Command::new("docker");
            c.arg("exec")
                .arg("-i")
                .arg(container_name)
                .arg(&docker_cli)
                .arg("--skip-ssl")
                .arg("--binary-mode")
                .arg("--quick")
                .arg("--max-allowed-packet=1024M")
                .arg("--net-buffer-length=1M")
                .arg("--connect-timeout=60")
                .arg("--default-character-set=utf8mb4")
                .arg("-u")
                .arg(&local_config.username);

            if !local_config.password.is_empty() {
                c.arg(format!("-p{}", local_config.password));
            }

            c.arg(db_name);
            c
        } else {
            let mut c = Command::new(cli_bin);
            c.arg("--skip-ssl")
                .arg("--binary-mode")
                .arg("--quick")
                .arg("--max-allowed-packet=1024M")
                .arg("--net-buffer-length=1M")
                .arg("--connect-timeout=60")
                .arg("--default-character-set=utf8mb4")
                .arg("-h")
                .arg(host)
                .arg("-P")
                .arg(&port_str)
                .arg("-u")
                .arg(&local_config.username);

            if !local_config.password.is_empty() {
                c.arg(format!("-p{}", local_config.password));
            }

            c.arg(db_name);
            c
        };

        cmd.stdin(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdout(Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                if use_docker {
                    return Err(format!(
                        "Gagal menjalankan perintah 'docker exec -i {} {}'. Pastikan Docker service berjalan dan nama kontainer benar. Error: {}",
                        container_name, docker_cli, e
                    ));
                } else {
                    return Err(format!(
                        "Gagal menjalankan perintah CLI '{}'. Pastikan client MySQL/MariaDB terinstall dan ada di PATH system. Error: {}",
                        cli_bin, e
                    ));
                }
            }
        };

        let mut child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Gagal membuka STDIN child process mysql".to_string())?;
        let mut child_stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Gagal membuka STDERR child process mysql".to_string())?;

        let sql_bytes_task = sql_bytes_arc.clone();
        let app_handle = app.clone();
        let table_name_clone = table_name.to_string();

        let stdin_task = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let drop_clause = if is_fresh {
                format!("DROP TABLE IF EXISTS `{}`;\n", table_name_clone.replace('`', "``"))
            } else {
                String::new()
            };
            let prelude_str = format!(
                "SET SESSION sql_log_bin=0;\n\
                 SET SESSION foreign_key_checks=0;\n\
                 SET SESSION unique_checks=0;\n\
                 SET SESSION autocommit=0;\n\
                 SET SESSION sql_mode='';\n\
                 SET SESSION net_read_timeout=600;\n\
                 SET SESSION net_write_timeout=600;\n\
                 SET SESSION wait_timeout=600;\n\
                 SET SESSION lock_wait_timeout=600;\n\
                 SET SESSION innodb_lock_wait_timeout=600;\n\
                 {}",
                drop_clause
            );

            if let Err(e) = child_stdin.write_all(prelude_str.as_bytes()).await {
                return Err(e);
            }

            let chunk_size = 1024 * 1024; // 1 MB per write chunk untuk throughput maksimal
            let mut written = 0usize;
            let mut last_log_time = std::time::Instant::now();
            let mut last_logged_written = 0usize;

            while written < total_sql_len {
                if is_user_cancelled() || is_aborted() {
                    return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Interrupted by user"));
                }
                let end = (written + chunk_size).min(total_sql_len);
                child_stdin.write_all(&sql_bytes_task[written..end]).await?;
                written = end;

                // Log live import streaming progress every 3 seconds for large SQL (> 3 MB)
                if total_sql_len >= 3 * 1024 * 1024 && last_log_time.elapsed().as_secs() >= 3 {
                    let delta_bytes = written - last_logged_written;
                    let delta_secs = last_log_time.elapsed().as_secs_f64();
                    let speed_mb = (delta_bytes as f64 / (1024.0 * 1024.0)) / delta_secs.max(0.1);
                    let percent = (written as f64 / total_sql_len as f64) * 100.0;
                    emit_log(
                        &app_handle,
                        "info",
                        format!(
                            "[Tabel '{}'] Mengalirkan ke MySQL: {} / {} ({:.1}%) (~{:.2} MB/s)...",
                            table_name_clone,
                            format_byte_size(written),
                            format_byte_size(total_sql_len),
                            percent,
                            speed_mb
                        ),
                    );
                    last_log_time = std::time::Instant::now();
                    last_logged_written = written;
                }
            }

            let postlude = b"\nCOMMIT;\nSET SESSION foreign_key_checks=1;\nSET SESSION unique_checks=1;\nSET SESSION autocommit=1;\n";
            if let Err(e) = child_stdin.write_all(postlude).await {
                return Err(e);
            }
            let _ = child_stdin.flush().await;
            drop(child_stdin);
            Ok::<(), std::io::Error>(())
        });

        let stderr_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut err_buf = Vec::new();
            let _ = child_stderr.read_to_end(&mut err_buf).await;
            err_buf
        });

        let mut cancelled = false;
        let status_res = loop {
            tokio::select! {
                status = child.wait() => {
                    break status;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(150)) => {
                    if is_user_cancelled() || is_aborted() {
                        cancelled = true;
                        let _ = child.start_kill();
                        break child.wait().await;
                    }
                }
            }
        };

        let stdin_res = stdin_task.await;
        let stderr_res = stderr_task.await;

        if cancelled {
            return Err("__USER_CANCELLED__".to_string());
        }

        let status = status_res.map_err(|e| format!("Gagal menunggu child process mysql: {}", e))?;
        let write_res = stdin_res.unwrap_or_else(|e| Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
        let stderr_raw = stderr_res.unwrap_or_default();
        let stderr_full = String::from_utf8_lossy(&stderr_raw);

        if status.success() {
            if let Err(e) = write_res {
                emit_log(
                    app,
                    "warn",
                    format!("[Tabel '{}'] MySQL selesai tetapi terdapat catatan stdin: {}", table_name, e),
                );
            }
            last_error.clear();
            break;
        }

        let stderr_tail = stderr_full
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        let write_context = write_res
            .err()
            .map(|e| format!(" (stdin: {})", e))
            .unwrap_or_default();
        last_error = format!("MySQL CLI import error: {}{}", stderr_tail, write_context);

        let is_real_deadlock = (stderr_full.contains("ERROR 1205")
            || stderr_full.contains("Lock wait timeout exceeded")
            || stderr_full.contains("ERROR 1213")
            || stderr_full.contains("Deadlock found"))
            && !is_schema_mismatch_error(&stderr_full);

        if is_real_deadlock && attempt < max_retries {
            emit_log(
                app,
                "warning",
                format!(
                    "[Tabel '{}'] Lock / Deadlock timeout (Percobaan {}/{}). Menunggu 2 detik lalu mencoba kembali...",
                    table_name, attempt, max_retries
                ),
            );
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }

        let will_auto_fallback = !is_fresh && is_schema_mismatch_error(&stderr_full);
        if !will_auto_fallback {
            emit_log(
                app,
                "error",
                format!(
                    "[Tabel '{}'] Executable MySQL gagal:\n{}{}",
                    table_name, stderr_tail, write_context
                ),
            );
        }
        return Err(last_error);
    }

    if !last_error.is_empty() {
        return Err(last_error);
    }


    let elapsed = table_start.elapsed();
    let elapsed_str = if elapsed.as_secs() >= 60 {
        format!("{}m {:.1}s", elapsed.as_secs() / 60, elapsed.as_secs_f64() % 60.0)
    } else {
        format!("{:.2}s", elapsed.as_secs_f64())
    };

    let rows_fmt = format_number(imported_rows);

    if is_structure_only {
        emit_log(
            app,
            "success",
            format!(
                "[Tabel '{}'] Struktur tabel berhasil dibuat di MySQL lokal dalam {}.",
                table_name, elapsed_str
            ),
        );
    } else {
        emit_log(
            app,
            "success",
            format!(
                "[Tabel '{}'] Selesai! ~{} row disinkronkan dalam {}.",
                table_name, rows_fmt, elapsed_str
            ),
        );
    }

    Ok(imported_rows)
}

/// Tahap 2 (Consumer with Auto-Fallback): Mencoba import data, dan jika gagal akibat Schema Mismatch, otomatis fallback ke Fresh Sync khusus tabel ini
async fn import_table_to_local_with_fallback(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    cached_endpoint: &std::sync::Arc<tokio::sync::Mutex<Option<(String, String)>>>,
    table_row_counts: &std::sync::Arc<HashMap<String, usize>>,
    pma_config: &PmaExportConfig,
    local_config: &LocalDbConfig,
    data: DownloadedTableData,
    app: &tauri::AppHandle,
) -> Result<usize, String> {
    let table_name = data.table_name.clone();
    let is_initially_fresh = pma_config.sync_mode.as_deref() == Some("fresh");

    let initial_res = import_table_to_local_internal(
        pma_config,
        local_config,
        data,
        false,
        app,
    )
    .await;

    match initial_res {
        Ok(rows) => Ok(rows),
        Err(err) => {
            if is_user_cancelled() {
                return Err("__USER_CANCELLED__".to_string());
            }
            if is_aborted() {
                return Err("__WORKER_ABORTED__".to_string());
            }

            // Jika bukan fresh mode dan terdeteksi ketidakcocokan skema (unknown column / structure mismatch)
            if !is_initially_fresh && is_schema_mismatch_error(&err) {
                let short_err = extract_short_schema_error(&err);
                emit_log(
                    app,
                    "warn",
                    format!(
                        "[Tabel '{}'] ⚠️ Terdeteksi ketidakcocokan skema tabel lokal dan server ({}). Melakukan auto-fallback ke FRESH SYNC untuk menyelaraskan skema dan data...",
                        table_name, short_err
                    ),
                );

                let mut fresh_pma_config = (*pma_config).clone();
                fresh_pma_config.sync_mode = Some("fresh".to_string());
                if let Some(ref mut wm) = fresh_pma_config.incremental_watermarks {
                    wm.remove(&table_name);
                }

                let fresh_download_res = fetch_table_export_stream(
                    client,
                    base_url,
                    csrf_token,
                    &fresh_pma_config,
                    &table_name,
                    cached_endpoint,
                    table_row_counts,
                    app,
                    None, // Tidak ada limit override pada fallback fresh re-download
                )
                .await;

                match fresh_download_res {
                    Ok(Some(fresh_data)) => {
                        let fresh_import_res = import_table_to_local_internal(
                            &fresh_pma_config,
                            local_config,
                            fresh_data,
                            true, // force_fresh = true -> DROP TABLE + CREATE TABLE
                            app,
                        )
                        .await;

                        match fresh_import_res {
                            Ok(fresh_rows) => {
                                emit_log(
                                    app,
                                    "success",
                                    format!(
                                        "[Tabel '{}'] ✅ Auto-fallback Fresh Sync berhasil! Skema tabel telah diselaraskan dengan server dan ~{} row disinkronkan.",
                                        table_name, format_number(fresh_rows)
                                    ),
                                );
                                Ok(fresh_rows)
                            }
                            Err(fresh_err) => {
                                emit_log(
                                    app,
                                    "error",
                                    format!(
                                        "[Tabel '{}'] Auto-fallback Fresh Sync gagal saat impor ke MySQL lokal: {}",
                                        table_name, fresh_err
                                    ),
                                );
                                Err(fresh_err)
                            }
                        }
                    }
                    Ok(None) => Ok(0),
                    Err(download_err) => {
                        emit_log(
                            app,
                            "error",
                            format!(
                                "[Tabel '{}'] Gagal mengunduh fresh export untuk auto-fallback: {}",
                                table_name, download_err
                            ),
                        );
                        Err(download_err)
                    }
                }
            } else {
                Err(err)
            }
        }
    }
}

fn sanitize_html_error(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            text.push(' ');
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            text.push(c);
        }
    }
    let cleaned: String = text.split_whitespace().collect::<Vec<&str>>().join(" ");
    if cleaned.len() > 300 {
        format!("{}...", &cleaned[..300])
    } else if cleaned.is_empty() {
        "PMA mengembalikan halaman HTML kosong.".to_string()
    } else {
        cleaned
    }
}

async fn ensure_local_database_exists(local_config: &LocalDbConfig, app: &tauri::AppHandle) {
    let host = if local_config.host.is_empty() {
        "127.0.0.1"
    } else {
        &local_config.host
    };
    let port_str = if local_config.port == 0 {
        "3306".to_string()
    } else {
        local_config.port.to_string()
    };

    let use_docker = local_config.use_docker && !local_config.docker_container.trim().is_empty();
    let container_name = local_config.docker_container.trim();

    let db_name = if local_config.database.is_empty() {
        "db_sync"
    } else {
        &local_config.database
    };
    let create_sql = format!(
        "CREATE DATABASE IF NOT EXISTS `{}`;",
        db_name.replace('`', "``")
    );

    let mut cmd = if use_docker {
        let docker_cli = get_docker_mysql_cli(container_name);
        let mut c = Command::new("docker");
        c.arg("exec")
            .arg(container_name)
            .arg(&docker_cli)
            .arg("--skip-ssl")
            .arg("--connect-timeout=60")
            .arg("-u")
            .arg(&local_config.username);

        if !local_config.password.is_empty() {
            c.arg(format!("-p{}", local_config.password));
        }

        c.arg("-e").arg(&create_sql);
        c
    } else {
        let cli_bin = get_mysql_cli_binary();
        let mut c = Command::new(cli_bin);
        c.arg("--skip-ssl")
            .arg("--connect-timeout=60")
            .arg("-h")
            .arg(host)
            .arg("-P")
            .arg(&port_str)
            .arg("-u")
            .arg(&local_config.username);

        if !local_config.password.is_empty() {
            c.arg(format!("-p{}", local_config.password));
        }

        c.arg("-e").arg(&create_sql);
        c
    };

    let target_desc = if use_docker {
        format!("Docker ('{}')", container_name)
    } else {
        "MySQL CLI".to_string()
    };

    match cmd.output().await {
        Ok(out) => {
            if !out.status.success() {
                let err = {
                    let s = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    if s.is_empty() {
                        let st = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if st.is_empty() {
                            format!("Status keluar: {:?}", out.status.code())
                        } else {
                            st
                        }
                    } else {
                        s
                    }
                };
                emit_log(
                    app,
                    "warn",
                    format!(
                        "Gagal membuat database lokal '{}' via {}: {}",
                        db_name,
                        target_desc,
                        err
                    ),
                );
            } else {
                emit_log(
                    app,
                    "info",
                    format!("Database lokal '{}' terverifikasi siap.", db_name),
                );
            }
        }
        Err(e) => {
            emit_log(
                app,
                "warn",
                format!(
                    "Gagal mengecek/membuat database lokal '{}' via {}: {}",
                    db_name, target_desc, e
                ),
            );
        }
    }
}

/// Menghitung jumlah worker paralel yang optimal & aman berdasarkan core CPU hardware
fn calculate_optimal_workers(total_tables: usize) -> (usize, usize) {
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);

    let optimal = match cpu_cores {
        1 => 1,
        2..=3 => 2,
        4..=6 => 3,
        7..=12 => 4,
        _ => 5,
    };

    let workers = optimal.min(total_tables).max(1);
    (workers, cpu_cores)
}

/// Main entry point for direct SQL/GZIP Stream export sync with Bounded Pre-fetching Pipeline
#[tauri::command]
pub async fn export_pma_database(
    pma_config: PmaExportConfig,
    local_config: LocalDbConfig,
    app: tauri::AppHandle,
) -> Result<String, String> {
    USER_CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    WORKER_ABORT_REQUESTED.store(false, Ordering::SeqCst);

    emit_log(
        &app,
        "info",
        "🚀 Memulai Sinkronisasi via Direct SQL/GZIP Stream (export.php)...",
    );

    ensure_local_database_exists(&local_config, &app).await;

    let (client, base_url, csrf_token) = authenticate_pma(&pma_config, &app).await?;

    let tables = if pma_config.tables.is_empty() {
        fetch_pma_tables(&pma_config, &app).await?
    } else {
        pma_config.tables.clone()
    };

    let mut pma_config = pma_config;
    pma_config.primary_key = pma_config
        .primary_key
        .take()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty());
    if let Some(keys) = pma_config.table_primary_keys.as_mut() {
        keys.retain(|_, key| {
            *key = key.trim().to_string();
            !key.is_empty()
        });
    }
    emit_log(&app, "info", "Mendeteksi Primary Key dan kolom 'updated_at' tiap tabel dari INFORMATION_SCHEMA...");
    let server_pk_map = fetch_all_primary_keys(&client, &base_url, &csrf_token, &pma_config.database, &app).await;
    if !server_pk_map.is_empty() {
        emit_log(&app, "info", format!("Primary key terdeteksi untuk {} tabel.", server_pk_map.len()));
        let mut merged = server_pk_map;
        if let Some(js_pks) = pma_config.table_primary_keys.take() {
            for (tbl, col) in js_pks {
                if !col.is_empty() {
                    merged.insert(tbl, col);
                }
            }
        }
        pma_config.table_primary_keys = Some(merged);
    }

    let tables_with_updated_at = fetch_tables_with_updated_at(&client, &base_url, &csrf_token, &pma_config.database, &app).await;
    if !tables_with_updated_at.is_empty() {
        emit_log(&app, "info", format!("Kolom 'updated_at' terdeteksi untuk {} tabel.", tables_with_updated_at.len()));
        pma_config.tables_with_updated_at = Some(tables_with_updated_at);
    }

    let table_row_counts = std::sync::Arc::new(
        fetch_table_row_counts(&client, &base_url, &csrf_token, &pma_config.database, &app).await
    );
    if !table_row_counts.is_empty() {
        emit_log(
            &app,
            "info",
            format!("Estimasi baris database terdeteksi untuk {} tabel.", table_row_counts.len())
        );
    }

    let total_tables = tables.len();
    let (concurrency, cpu_cores) = calculate_optimal_workers(total_tables);
    let buffer_capacity = 8.min(total_tables).max(2);
    // Remote PMA download concurrency dibatasi maksimal 2 worker agar session file remote tidak terkunci dan CPU remote hosting tidak di-throttle
    let download_concurrency = (cpu_cores / 4).clamp(1, 2).min(total_tables);

    emit_log(
        &app,
        "info",
        format!(
            "🚀 Memulai sinkronisasi pipeline {} tabel (terdeteksi {} CPU cores, {} download workers, {} import workers, buffer antrean: {} tabel)...",
            total_tables, cpu_cores, download_concurrency, concurrency, buffer_capacity
        ),
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<Option<DownloadedTableData>>(buffer_capacity);
    let rx = std::sync::Arc::new(tokio::sync::Mutex::new(rx));
    let cached_endpoint = std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let completed_tables = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_rows = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let client = std::sync::Arc::new(client);
    let base_url = std::sync::Arc::new(base_url);
    let csrf_token = std::sync::Arc::new(csrf_token);
    let pma_config = std::sync::Arc::new(pma_config);
    let local_config = std::sync::Arc::new(local_config);

    // 1. Parallel Producer Tasks: Pre-fetch tables from phpMyAdmin into bounded queue (capacity 5)
    let tables_queue = std::sync::Arc::new(tokio::sync::Mutex::new(
        tables.into_iter().collect::<std::collections::VecDeque<String>>()
    ));

    let mut download_handles = Vec::new();
    for _ in 0..download_concurrency {
        let queue_c = tables_queue.clone();
        let client_p = client.clone();
        let base_url_p = base_url.clone();
        let csrf_token_p = csrf_token.clone();
        let pma_config_p = pma_config.clone();
        let cached_endpoint_p = cached_endpoint.clone();
        let table_row_counts_p = table_row_counts.clone();
        let app_p = app.clone();
        let tx_c = tx.clone();

        let handle = tokio::spawn(async move {
            loop {
                if is_user_cancelled() {
                    return Err("__USER_CANCELLED__".to_string());
                }
                if is_aborted() {
                    return Err("__WORKER_ABORTED__".to_string());
                }

                let next_table = {
                    let mut lock = queue_c.lock().await;
                    lock.pop_front()
                };

                let table_name = match next_table {
                    Some(t) => t,
                    None => break,
                };

                // Eksekusi unduh export stream (timeout 120s dikelola mandiri per-chunk di dalam fetch_table_export_stream)
                let download_res = fetch_table_export_stream(
                    &client_p,
                    &base_url_p,
                    &csrf_token_p,
                    &pma_config_p,
                    &table_name,
                    &cached_endpoint_p,
                    &table_row_counts_p,
                    &app_p,
                    None,
                )
                .await;

                let maybe_data = match download_res {
                    Ok(data) => data,
                    Err(e) => {
                        if e == "__USER_CANCELLED__" {
                            return Err("__USER_CANCELLED__".to_string());
                        }
                        if e == "__WORKER_ABORTED__" {
                            return Err("__WORKER_ABORTED__".to_string());
                        }
                        emit_log(
                            &app_p,
                            "warn",
                            format!(
                                "⚠️ [Tabel '{}'] Gagal mengunduh export: {}. Melanjutkan ke tabel berikutnya...",
                                table_name, e
                            ),
                        );
                        None
                    }
                };

                let table_succeeded = maybe_data.is_some();

                if !table_succeeded {
                    // Kirim None ke consumer agar counter completed tetap bertambah
                    let _ = tx_c.send(None).await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                    continue;
                }

                {
                    let mut sent = false;
                    while !sent {
                        if is_user_cancelled() || is_aborted() {
                            return Err("__WORKER_ABORTED__".to_string());
                        }
                        tokio::select! {
                            send_res = tx_c.send(maybe_data.clone()) => {
                                if send_res.is_err() {
                                    return Ok(());
                                }
                                sent = true;
                            }
                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(150)) => {
                                // Periodic cancellation check
                            }
                        }
                    }

                    // Jeda throttling antar tabel (300ms) agar server remote hosting tidak kehabisan CPU / terkena CloudLinux LVE throttle
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                }
            }
            Ok::<(), String>(())
        });
        download_handles.push(handle);
    }
    // Drop the initial sender so channel closes when all download workers finish
    drop(tx);

    // 2. Consumer Worker Tasks: Import from channel queue into Local MySQL CLI
    let mut consumer_handles = Vec::new();
    for _ in 0..concurrency {
        let rx_c = rx.clone();
        let pma_config_c = pma_config.clone();
        let local_config_c = local_config.clone();
        let completed_c = completed_tables.clone();
        let total_rows_c = total_rows.clone();
        let client_c = client.clone();
        let base_url_c = base_url.clone();
        let csrf_token_c = csrf_token.clone();
        let cached_endpoint_c = cached_endpoint.clone();
        let table_row_counts_c = table_row_counts.clone();
        let app_c = app.clone();

        let handle = tokio::spawn(async move {
            loop {
                if is_user_cancelled() {
                    return Err("__USER_CANCELLED__".to_string());
                }
                if is_aborted() {
                    return Err("__WORKER_ABORTED__".to_string());
                }

                let maybe_item = loop {
                    if is_user_cancelled() || is_aborted() {
                        return Err("__WORKER_ABORTED__".to_string());
                    }
                    let mut lock = rx_c.lock().await;
                    tokio::select! {
                        item = lock.recv() => {
                            break item;
                        }
                        _ = tokio::time::sleep(tokio::time::Duration::from_millis(150)) => {
                            // Check cancellation periodically
                        }
                    }
                };

                match maybe_item {
                    Some(Some(data)) => {
                        let table_name = data.table_name.clone();
                        let import_res = import_table_to_local_with_fallback(
                            &client_c,
                            &base_url_c,
                            &csrf_token_c,
                            &cached_endpoint_c,
                            &table_row_counts_c,
                            &pma_config_c,
                            &local_config_c,
                            data,
                            &app_c,
                        )
                        .await;

                        let imported_rows = match import_res {
                            Ok(rows) => rows,
                            Err(e) => {
                                WORKER_ABORT_REQUESTED.store(true, Ordering::SeqCst);
                                return Err(e);
                            }
                        };

                        let current_completed = completed_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        let current_total_rows = total_rows_c.fetch_add(imported_rows, std::sync::atomic::Ordering::SeqCst) + imported_rows;

                        emit_progress(
                            &app_c,
                            current_completed,
                            total_tables,
                            &table_name,
                            imported_rows,
                            current_total_rows,
                            "syncing",
                        );
                    }
                    Some(None) => {
                        let current_completed = completed_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        let current_total_rows = total_rows_c.load(std::sync::atomic::Ordering::SeqCst);
                        emit_progress(
                            &app_c,
                            current_completed,
                            total_tables,
                            "",
                            0,
                            current_total_rows,
                            "syncing",
                        );
                    }
                    None => {
                        break;
                    }
                }
            }
            Ok::<(), String>(())
        });
        consumer_handles.push(handle);
    }

    let all_download_res = futures_util::future::join_all(download_handles).await;
    let all_consumer_res = futures_util::future::join_all(consumer_handles).await;

    let mut real_error: Option<String> = None;

    for res in all_download_res {
        match res {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => {
                WORKER_ABORT_REQUESTED.store(true, Ordering::SeqCst);
                if e != "__USER_CANCELLED__" && e != "__WORKER_ABORTED__" && !e.contains("dibatalkan oleh pengguna") && real_error.is_none() {
                    real_error = Some(e);
                }
            },
            Err(e) => {
                WORKER_ABORT_REQUESTED.store(true, Ordering::SeqCst);
                if real_error.is_none() {
                    real_error = Some(format!("Download worker execution error: {}", e));
                }
            }
        }
    }

    for res in all_consumer_res {
        match res {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => {
                WORKER_ABORT_REQUESTED.store(true, Ordering::SeqCst);
                if e != "__USER_CANCELLED__" && e != "__WORKER_ABORTED__" && !e.contains("dibatalkan oleh pengguna") && real_error.is_none() {
                    real_error = Some(e);
                }
            },
            Err(e) => {
                WORKER_ABORT_REQUESTED.store(true, Ordering::SeqCst);
                if real_error.is_none() {
                    real_error = Some(format!("Consumer worker execution error: {}", e));
                }
            }
        }
    }

    if is_user_cancelled() {
        emit_progress(&app, completed_tables.load(Ordering::SeqCst), total_tables, "", 0, total_rows.load(Ordering::SeqCst), "cancelled");
        emit_log(&app, "warn", "🛑 Sinkronisasi telah dihentikan oleh pengguna.");
        return Err("Sinkronisasi dibatalkan oleh pengguna.".to_string());
    }

    if let Some(err) = real_error {
        emit_progress(&app, completed_tables.load(Ordering::SeqCst), total_tables, "", 0, total_rows.load(Ordering::SeqCst), "error");
        emit_log(&app, "error", format!("Sinkronisasi gagal: {}", err));
        return Err(err);
    }

    let final_total_rows = total_rows.load(std::sync::atomic::Ordering::SeqCst);
    let final_completed = completed_tables.load(std::sync::atomic::Ordering::SeqCst);

    emit_progress(
        &app,
        final_completed,
        total_tables,
        "",
        0,
        final_total_rows,
        "finished",
    );
    emit_log(
        &app,
        "success",
        format!(
            "🎉 Direct SQL/GZIP Stream Sync Berhasil! {} tabel telah disinkronkan dengan {} worker paralel (Pipeline Pre-fetch Buffer).",
            total_tables, concurrency
        ),
    );

    Ok(format!(
        "Berhasil menyinkronkan {} tabel via Direct GZIP Stream ({} Workers).",
        total_tables, concurrency
    ))
}
