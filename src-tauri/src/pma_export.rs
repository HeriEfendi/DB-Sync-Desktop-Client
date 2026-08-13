use crate::commands::LocalDbConfig;
use flate2::read::MultiGzDecoder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::Emitter;

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

    let noisy_http = log_type == "info"
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
        log_type == "warn" && msg.contains("mengembalikan HTML response: 404 Not Found");

    if noisy_http || noisy_fallback {
        println!("[PMA-LOG] [{}]: {}", log_type, msg);
        return;
    }

    println!("[PMA-LOG] [{}]: {}", log_type, msg);
    let _ = app.emit(
        "pma-log",
        LogPayload {
            r#type: log_type.to_string(),
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
    app: &tauri::AppHandle,
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
            Err(e) => {
                emit_log(app, "warn", format!("[PK detect] Request ke {} gagal: {}", sql_url, e));
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
                emit_log(app, "warn", "[PK detect] Response bukan JSON valid, skip.");
                continue;
            }
        };

        let success = json_val
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            let err = json_val.get("error").or_else(|| json_val.get("message"))
                .and_then(|v| v.as_str()).unwrap_or("unknown");
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
        } else {
            emit_log(app, "warn", format!("[PK detect] Endpoint {} tidak menghasilkan PK, coba endpoint lain.", sql_url));
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

/// Execute stream export for a single table via export.php/route endpoints and pipe decompressed or raw stream directly to mysql STDIN
fn count_imported_sql_rows(sql_bytes: &[u8]) -> usize {
    let sql = String::from_utf8_lossy(sql_bytes);
    let mut count = 0usize;
    let mut in_values = false;

    for line in sql.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("INSERT INTO") || trimmed.starts_with("REPLACE INTO") {
            in_values = true;
            if trimmed.contains("VALUES") {
                let after_values = trimmed.split("VALUES").nth(1).unwrap_or("");
                let items = after_values.split("),").count();
                if items > 0 {
                    count += items;
                }
            }
            continue;
        }
        if in_values {
            if trimmed.starts_with('(') {
                count += 1;
            }
            if trimmed.ends_with(';') {
                in_values = false;
            }
        }
    }

    if count == 0 && (sql.contains("INSERT INTO") || sql.contains("REPLACE INTO")) {
        count = 1;
    }
    count
}

async fn stream_export_single_table(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    pma_config: &PmaExportConfig,
    local_config: &LocalDbConfig,
    table_name: &str,
    cached_endpoint: &std::sync::Arc<tokio::sync::Mutex<Option<(String, String)>>>,
    app: &tauri::AppHandle,
) -> Result<usize, String> {
    let table_start = std::time::Instant::now();

    emit_log(
        app,
        "info",
        format!("[Tabel '{}'] Mempersiapkan sinkronisasi...", table_name),
    );

    let sql_type_val = if pma_config.sync_mode.as_deref() == Some("fresh") {
        "INSERT"
    } else {
        "REPLACE"
    };

    let (cached_url, cached_comp) = {
        let guard = cached_endpoint.lock().await;
        guard.clone().unzip()
    };

    let export_urls = if let Some(ref u) = cached_url {
        vec![u.clone()]
    } else {
        vec![
            format!("{}/export.php", base_url),
            format!("{}/index.php?route=/export", base_url),
            format!("{}/index.php?route=/table/export", base_url),
            format!("{}/index.php?route=/database/export", base_url),
        ]
    };

    let compressions = if let Some(ref c) = cached_comp {
        vec![c.clone()]
    } else {
        vec!["gzip".to_string(), "none".to_string()]
    };

    let mut valid_response: Option<(reqwest::Response, bool)> = None; // (response, is_gzip)
    let mut last_html_err = String::new();

    'endpoint_loop: for export_url in &export_urls {
        for compression_mode in &compressions {
            let is_gz = compression_mode == "gzip";
            let is_incremental = pma_config.sync_mode.as_deref() == Some("incremental");
            let struct_or_data = if is_incremental { "data" } else { "structure_and_data" };
            let sql_structure = if is_incremental { "0" } else { "1" };
            let sql_create = if is_incremental { "false" } else { "true" };

            let mut form: Vec<(String, String)> = vec![
                ("db".to_string(), pma_config.database.clone()),
                ("table".to_string(), table_name.to_string()),
                ("table_select[]".to_string(), table_name.to_string()),
                ("table_structure[]".to_string(), table_name.to_string()),
                ("table_data[]".to_string(), table_name.to_string()),
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
                ("sql_data".to_string(), "1".to_string()),
                ("sql_create_table".to_string(), sql_create.to_string()),
                ("sql_drop_table".to_string(), "false".to_string()),
                ("sql_if_not_exists".to_string(), "true".to_string()),
                ("sql_auto_increment".to_string(), "1".to_string()),
                ("sql_backquotes".to_string(), "1".to_string()),
                ("sql_type".to_string(), sql_type_val.to_string()),
            ];

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
                return Ok(0);
            }

            if pma_config.row_limit.filter(|limit| *limit > 0).is_some() || has_incremental_watermark {
                let limit = pma_config.row_limit.filter(|limit| *limit > 0).unwrap_or(usize::MAX);
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

                form.push(("sql_query".to_string(), limited_query));
                form.push(("query_type".to_string(), "table".to_string()));
                form.push(("sql_query_input".to_string(), "true".to_string()));
            }
            if !csrf_token.is_empty() {
                form.push(("token".to_string(), csrf_token.to_string()));
            }

            // emit_log(
            //     app,
            //     "info",
            //     format!("[Tabel '{}'] Mulai mengunduh SQL dari PMA...", table_name),
            // );

            let response_res = client.post(export_url).form(&form).send().await;
            let response = match response_res {
                Ok(r) => r,
                Err(e) => {
                    emit_log(
                        app,
                        "warn",
                        format!(
                            "[Tabel '{}'] POST ke {} gagal: {}",
                            table_name, export_url, e
                        ),
                    );
                    continue;
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
                emit_log(
                    app,
                    "warn",
                    format!(
                        "[Tabel '{}'] Endpoint {} (comp={}) mengembalikan HTML response: {}",
                        table_name, export_url, compression_mode, last_html_err
                    ),
                );
                continue;
            }

            emit_log(
                app,
                "info",
                format!(
                    "[Tabel '{}'] Endpoint PMA valid ditemukan ({}, Content-Type: {})",
                    table_name, export_url, content_type
                ),
            );
            valid_response = Some((response, is_gz));

            // Cache valid working endpoint for next tables
            let mut guard = cached_endpoint.lock().await;
            if guard.is_none() {
                *guard = Some((export_url.clone(), compression_mode.clone()));
            }
            break 'endpoint_loop;
        }
    }

    let (response, is_gzip) = match valid_response {
        Some(res) => res,
        None => {
            return Err(format!(
                "Export GAGAL untuk tabel '{}'. Semua endpoint PMA mengembalikan HTML Error. Pesan terakhir: {}",
                table_name, last_html_err
            ));
        }
    };

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
    let db_name = if local_config.database.is_empty() {
        "db_sync"
    } else {
        &local_config.database
    };

    if pma_config.sync_mode.as_deref() == Some("fresh") {
        // emit_log(app, "info", format!("[Tabel '{}'] Mode Fresh Sync — Menghapus (DROP TABLE IF EXISTS) tabel lokal terlebih dahulu...", table_name));
        let mut drop_cmd = Command::new("mysql");
        drop_cmd
            .arg("-h")
            .arg(host)
            .arg("-P")
            .arg(&port_str)
            .arg("-u")
            .arg(&local_config.username);
        if !local_config.password.is_empty() {
            drop_cmd.arg(format!("-p{}", local_config.password));
        }
        drop_cmd.arg(db_name).arg("-e").arg(format!(
            "SET FOREIGN_KEY_CHECKS=0; DROP TABLE IF EXISTS `{}`; SET FOREIGN_KEY_CHECKS=1;",
            table_name.replace('`', "``")
        ));
        let drop_output = drop_cmd
            .output()
            .map_err(|e| format!("Gagal menjalankan DROP TABLE untuk '{}': {}", table_name, e))?;
        if !drop_output.status.success() {
            let stderr = String::from_utf8_lossy(&drop_output.stderr);
            return Err(format!(
                "Gagal menghapus tabel lokal '{}': {}",
                table_name,
                stderr.trim()
            ));
        }
    }

    // Spawn child process `mysql` CLI
    let mut cmd = Command::new("mysql");
    cmd.arg("-h")
        .arg(host)
        .arg("-P")
        .arg(&port_str)
        .arg("-u")
        .arg(&local_config.username);

    if !local_config.password.is_empty() {
        cmd.arg(format!("-p{}", local_config.password));
    }

    cmd.arg(db_name);
    cmd.stdin(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdout(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(format!(
                "Gagal menjalankan perintah CLI 'mysql'. Pastikan client MySQL/MariaDB terinstall dan ada di PATH system. Error: {}",
                e
            ));
        }
    };

    // Read export body once. Detect real GZIP signature; PMA may label plain SQL as application/x-gzip.
    let response_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Gagal membaca body export PMA: {}", e))?;
    let total_bytes = response_bytes.len();
    if total_bytes == 0 {
        return Err(format!(
            "Export tabel '{}' mengembalikan response kosong.",
            table_name
        ));
    }

    let is_actual_gzip =
        response_bytes.len() >= 2 && response_bytes[0] == 0x1f && response_bytes[1] == 0x8b;
    emit_log(
        app,
        "info",
        format!(
            "[Tabel '{}'] Export menerima {} byte; format aktual: {} (Content-Type gzip: {}).",
            table_name,
            total_bytes,
            if is_actual_gzip { "GZIP" } else { "SQL/raw" },
            is_gzip
        ),
    );

    let mut sql_bytes = if is_actual_gzip {
        let mut decoder = MultiGzDecoder::new(std::io::Cursor::new(response_bytes));
        let mut decoded = Vec::new();
        decoder
            .read_to_end(&mut decoded)
            .map_err(|e| format!("GZIP export tidak valid: {}", e))?;
        decoded
    } else {
        response_bytes.to_vec()
    };
    let processed_bytes = sql_bytes.len();
    if processed_bytes == 0 {
        return Err(format!(
            "Export tabel '{}' menghasilkan SQL kosong.",
            table_name
        ));
    }

    if pma_config.sync_mode.as_deref() == Some("incremental") {
        let sql_text = String::from_utf8_lossy(&sql_bytes);
        let safe_lines: Vec<&str> = sql_text
            .lines()
            .filter(|line| {
                let trimmed = line.trim().to_uppercase();
                !trimmed.starts_with("DROP TABLE")
                    && !trimmed.starts_with("TRUNCATE TABLE")
                    && !trimmed.starts_with("TRUNCATE ")
            })
            .collect();
        sql_bytes = safe_lines.join("\n").into_bytes();
    }

    // Import table independently; referenced tables may be synchronized later.
    let mut import_sql = b"SET FOREIGN_KEY_CHECKS=0;\n".to_vec();
    import_sql.append(&mut sql_bytes);
    import_sql.extend_from_slice(b"\nSET FOREIGN_KEY_CHECKS=1;\n");
    let sql_bytes = import_sql;

    let mut mysql_stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Gagal membuka STDIN child process mysql".to_string())?;
    use std::io::Write;
    let write_result = mysql_stdin.write_all(&sql_bytes);
    drop(mysql_stdin);

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Gagal menghentikan child process mysql: {}", e))?;
    if !output.status.success() {
        let stderr_full = String::from_utf8_lossy(&output.stderr);
        let stderr_tail = stderr_full
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\\n");
        let write_context = write_result
            .err()
            .map(|e| format!(" (stdin: {})", e))
            .unwrap_or_default();
        emit_log(
            app,
            "error",
            format!(
                "[Tabel '{}'] Executable MySQL gagal:\\n{}{}",
                table_name, stderr_tail, write_context
            ),
        );
        return Err(format!(
            "MySQL CLI import error: {}{}",
            stderr_tail, write_context
        ));
    }
    if let Err(e) = write_result {
        return Err(format!("Gagal mengirim SQL ke MySQL: {}", e));
    }

    let imported_rows = count_imported_sql_rows(&sql_bytes);

    let elapsed = table_start.elapsed();
    let elapsed_str = if elapsed.as_secs() >= 60 {
        format!("{}m {:.1}s", elapsed.as_secs() / 60, elapsed.as_secs_f64() % 60.0)
    } else {
        format!("{:.2}s", elapsed.as_secs_f64())
    };

    let rows_fmt = {
        let s = imported_rows.to_string();
        let mut out = String::with_capacity(s.len() + s.len() / 3);
        for (i, c) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 { out.push('.'); }
            out.push(c);
        }
        out.chars().rev().collect::<String>()
    };

    emit_log(
        app,
        "success",
        format!(
            "[Tabel '{}'] Selesai! ~{} row disinkronkan dalam {}.",
            table_name, rows_fmt, elapsed_str
        ),
    );

    Ok(imported_rows)
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

fn ensure_local_database_exists(local_config: &LocalDbConfig, app: &tauri::AppHandle) {
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

    let mut cmd = Command::new("mysql");
    cmd.arg("-h")
        .arg(host)
        .arg("-P")
        .arg(&port_str)
        .arg("-u")
        .arg(&local_config.username);

    if !local_config.password.is_empty() {
        cmd.arg(format!("-p{}", local_config.password));
    }

    let db_name = if local_config.database.is_empty() {
        "db_sync"
    } else {
        &local_config.database
    };
    let create_sql = format!(
        "CREATE DATABASE IF NOT EXISTS `{}`;",
        db_name.replace('`', "``")
    );

    cmd.arg("-e").arg(&create_sql);

    match cmd.output() {
        Ok(out) => {
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                emit_log(
                    app,
                    "warn",
                    format!("Penyiapan DB lokal '{}': {}", db_name, err),
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
                format!("Gagal memeriksa/membuat DB lokal via CLI mysql: {}", e),
            );
        }
    }
}

/// Main entry point for direct SQL/GZIP Stream export sync
#[tauri::command]
pub async fn export_pma_database(
    pma_config: PmaExportConfig,
    local_config: LocalDbConfig,
    app: tauri::AppHandle,
) -> Result<String, String> {
    emit_log(
        &app,
        "info",
        "🚀 Memulai Sinkronisasi via Direct SQL/GZIP Stream (export.php)...",
    );

    // Ensure target local database exists before streaming tables
    ensure_local_database_exists(&local_config, &app);

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
        // Merge: server_pk_map jadi base, lalu override dengan data dari JS (lebih spesifik)
        let mut merged = server_pk_map;
        if let Some(js_pks) = pma_config.table_primary_keys.take() {
            for (tbl, col) in js_pks {
                if !col.is_empty() {
                    merged.insert(tbl, col);
                }
            }
        }
        pma_config.table_primary_keys = Some(merged);
    } else {
        emit_log(&app, "warn", "Primary key server tidak terdeteksi via metadata; memakai fallback per tabel.");
    }

    let tables_with_updated_at = fetch_tables_with_updated_at(&client, &base_url, &csrf_token, &pma_config.database, &app).await;
    if !tables_with_updated_at.is_empty() {
        emit_log(&app, "info", format!("Kolom 'updated_at' terdeteksi untuk {} tabel.", tables_with_updated_at.len()));
        pma_config.tables_with_updated_at = Some(tables_with_updated_at);
    }

    let total_tables = tables.len();
    let concurrency = 3usize;
    emit_log(
        &app,
        "info",
        format!(
            "🚀 Memulai ekspor paralel {} tabel (menggunakan {} worker simultan)...",
            total_tables, concurrency
        ),
    );

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let cached_endpoint = std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let completed_tables = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_rows = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let client = std::sync::Arc::new(client);
    let base_url = std::sync::Arc::new(base_url);
    let csrf_token = std::sync::Arc::new(csrf_token);
    let pma_config = std::sync::Arc::new(pma_config);
    let local_config = std::sync::Arc::new(local_config);

    let mut join_handles = Vec::new();

    for table_name in tables {
        let sem = semaphore.clone();
        let client_c = client.clone();
        let base_url_c = base_url.clone();
        let csrf_token_c = csrf_token.clone();
        let pma_config_c = pma_config.clone();
        let local_config_c = local_config.clone();
        let cached_endpoint_c = cached_endpoint.clone();
        let completed_c = completed_tables.clone();
        let total_rows_c = total_rows.clone();
        let app_handle = app.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.map_err(|e| e.to_string())?;

            let imported_rows = stream_export_single_table(
                &client_c,
                &base_url_c,
                &csrf_token_c,
                &pma_config_c,
                &local_config_c,
                &table_name,
                &cached_endpoint_c,
                &app_handle,
            )
            .await?;

            let current_completed = completed_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let current_total_rows = total_rows_c.fetch_add(imported_rows, std::sync::atomic::Ordering::SeqCst) + imported_rows;

            emit_progress(
                &app_handle,
                current_completed,
                total_tables,
                &table_name,
                imported_rows,
                current_total_rows,
                "syncing",
            );

            Ok::<usize, String>(imported_rows)
        });

        join_handles.push(handle);
    }

    for handle in join_handles {
        match handle.await {
            Ok(Ok(_)) => {},
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(format!("Worker execution error: {}", e)),
        }
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
            "🎉 Direct SQL/GZIP Stream Sync Berhasil! {} tabel telah disinkronkan dengan 3 worker paralel.",
            total_tables
        ),
    );

    Ok(format!(
        "Berhasil menyinkronkan {} tabel via Direct GZIP Stream (3 Workers).",
        total_tables
    ))
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
