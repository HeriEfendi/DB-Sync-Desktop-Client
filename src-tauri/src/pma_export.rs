use serde::{Deserialize, Serialize};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::Emitter;
use futures_util::StreamExt;
use flate2::read::GzDecoder;
use crate::commands::LocalDbConfig;

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

/// Reader that converts a tokio mpsc channel of byte chunks into std::io::Read
struct ChannelReader {
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    cursor: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cursor >= self.buffer.len() {
            match self.rx.blocking_recv() {
                Some(data) => {
                    self.buffer = data;
                    self.cursor = 0;
                }
                None => return Ok(0), // EOF
            }
        }
        let remaining = self.buffer.len() - self.cursor;
        let to_copy = buf.len().min(remaining);
        buf[..to_copy].copy_from_slice(&self.buffer[self.cursor..self.cursor + to_copy]);
        self.cursor += to_copy;
        Ok(to_copy)
    }
}

fn current_timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn emit_log(app: &tauri::AppHandle, log_type: &str, message: impl Into<String>) {
    let msg = message.into();
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
                        if val.len() >= 16 && val.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                            return Some(val.to_string());
                        }
                    }
                }
            }
            if line.contains("value='") {
                for part in line.split("value='").skip(1) {
                    if let Some(end) = part.find('\'') {
                        let val = part[..end].trim();
                        if val.len() >= 16 && val.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
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
    emit_log(app, "info", format!("Inisialisasi HTTP Session ke PMA: {}", base_url));

    let client = reqwest::Client::builder()
        .cookie_store(true)
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
    emit_log(app, "info", format!("PMA final URL setelah redirect: {}", final_url));

    let mut effective_base_url = if final_url.contains("/index.php") {
        final_url.split("/index.php").next().unwrap_or(&base_url).to_string()
    } else {
        base_url.clone()
    };

    let mut html = resp.text().await.map_err(|e| format!("Gagal membaca response HTML dari PMA: {}", e))?;
    emit_log(app, "info", format!("HTML index.php len={}, preview={}", html.len(), &html[..html.len().min(200)]));

    // Check for HTML meta refresh or JS redirect (e.g. redirecting to ./public/ or cPanel subpath)
    if let Some(redirect_path) = find_html_redirect(&html) {
        emit_log(app, "info", format!("Redirect HTML ditemukan: {}", redirect_path));
        let resolved_url = resolve_relative_url(&index_url, &redirect_path);
        emit_log(app, "info", format!("Resolved redirect URL: {}", resolved_url));

        // Update effective_base_url to the new subpath (e.g. https://host/public)
        effective_base_url = if resolved_url.contains("/index.php") {
            resolved_url.split("/index.php").next().unwrap_or(&resolved_url).to_string()
        } else {
            resolved_url.trim_end_matches('/').to_string()
        };
        emit_log(app, "info", format!("effective_base_url diperbarui: {}", effective_base_url));

        // Re-fetch from the actual PMA location
        let new_index_url = format!("{}/index.php", effective_base_url);
        if let Ok(r) = client.get(&new_index_url).send().await {
            if let Ok(new_html) = r.text().await {
                emit_log(app, "info", format!("Re-fetch dari {}: len={}", new_index_url, new_html.len()));
                html = new_html;
            }
        }
    }

    let mut csrf_token = extract_csrf_token(&html).unwrap_or_default();
    emit_log(app, "info", format!("CSRF token diekstrak: '{}' (len={})", &csrf_token[..csrf_token.len().min(16)], csrf_token.len()));

    if !pma_config.username.is_empty() {
        emit_log(app, "info", format!("Melakukan otentikasi login PMA untuk user '{}'...", pma_config.username));
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

        let login_resp = client
            .post(&login_url)
            .form(&form)
            .send()
            .await;

        let login_resp = match login_resp {
            Ok(r) => r,
            Err(_) => {
                let alt_login_url = format!("{}/index.php", effective_base_url);
                emit_log(app, "warn", format!("Login route=/login gagal, mencoba: {}", alt_login_url));
                client.post(&alt_login_url).form(&form).send().await
                    .map_err(|e| format!("Gagal login ke PMA: {}", e))?
            }
        };

        let login_status = login_resp.status();
        let login_final_url = login_resp.url().to_string();
        let login_html = login_resp.text().await.map_err(|e| format!("Gagal membaca response login: {}", e))?;

        emit_log(app, "info", format!(
            "Login response: status={}, final_url={}, len={}, preview={}",
            login_status.as_u16(),
            login_final_url,
            login_html.len(),
            &login_html[..login_html.len().min(300)]
        ));

        if let Some(new_token) = extract_csrf_token(&login_html) {
            emit_log(app, "info", format!("Token baru dari login response: '{}' (len={})", &new_token[..new_token.len().min(16)], new_token.len()));
            csrf_token = new_token;
        } else {
            emit_log(app, "warn", "Tidak ada token baru di login response, menggunakan token lama");
        }

        if login_html.contains("Access denied") || login_html.contains("Cannot log in to the MySQL server") {
            return Err("Login PMA Gagal: Username atau Password salah atau akses ditolak.".to_string());
        }

        // Check if login was actually successful by looking for authenticated page indicators
        let login_ok = !login_html.contains("pma_username") && !login_html.contains("\"name\":\"pma_username\"")
            && (login_html.contains("main_pane_left") || login_html.contains("navigation_tree")
                || login_html.contains("pma-core") || login_html.contains("\"success\":true")
                || login_html.contains("db=") || !csrf_token.is_empty());

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
                let end = rest.find('"').or_else(|| rest.find('\'')).unwrap_or(rest.len());
                return Some(rest[..end].trim().to_string());
            }
            let start_opt = line.find("window.location=").or_else(|| line.find("window.location ="));
            if let Some(start) = start_opt {
                if let Some(quote_start) = line[start..].find('"').or_else(|| line[start..].find('\'')) {
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
        let origin_end = base_dir[protocol_end..].find('/').map(|i| i + protocol_end).unwrap_or(base_dir.len());
        return format!("{}{}", &base_dir[..origin_end], relative.trim_end_matches('/'));
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

    emit_log(app, "info", format!("Mengambil daftar tabel dari database '{}'...", db));

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

            emit_log(app, "info", format!("Mencoba AJAX SQL ke '{}': {}", sql_url, &query[..query.len().min(60)]));

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
                    emit_log(app, "warn", format!("AJAX SQL request gagal ke '{}': {}", sql_url, e));
                    continue;
                }
            };

            let status = resp.status();
            let raw_text = resp.text().await.unwrap_or_default();
            emit_log(app, "info", format!("AJAX SQL response status={}, len={}, preview={}",
                status.as_u16(), raw_text.len(), &raw_text[..raw_text.len().min(300)]));

            // Try parse JSON
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&raw_text) {
                let success = json_val.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                if !success {
                    let err_msg = json_val.get("error")
                        .or_else(|| json_val.get("message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    emit_log(app, "warn", format!("AJAX SQL error dari PMA: {}", err_msg));
                    continue;
                }

                let mut tables = extract_tables_from_json(&json_val, db);
                if !tables.is_empty() {
                    emit_log(app, "success", format!("AJAX SQL berhasil, {} tabel ditemukan pada batch awal", tables.len()));

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

                        emit_log(app, "info", format!("Mengekstrak halaman lanjutan offset {}...", offset));
                        if let Ok(paged_resp) = client
                            .post(sql_url)
                            .header("X-Requested-With", "XMLHttpRequest")
                            .header("Content-Type", "application/x-www-form-urlencoded")
                            .form(&paged_form)
                            .send()
                            .await
                        {
                            if let Ok(paged_text) = paged_resp.text().await {
                                if let Ok(paged_json) = serde_json::from_str::<serde_json::Value>(&paged_text) {
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

                emit_log(app, "warn", "AJAX SQL success=true tapi tidak ada tabel di response JSON");
            } else {
                emit_log(app, "warn", format!("AJAX SQL response bukan JSON valid (len={})", raw_text.len()));
            }
        }
    }

    // === FALLBACK: HTML scraping dari database structure & export pages ===
    if found_tables.is_empty() {
        emit_log(app, "warn", "AJAX SQL tidak menghasilkan tabel, mencoba HTML scraping...");

        let db_encoded = urlencoding::encode(db);
        let token_param = if !csrf_token.is_empty() {
            format!("&token={}", urlencoding::encode(&csrf_token))
        } else {
            String::new()
        };

        // Prioritize export page because export pages list ALL tables without pagination
        let candidate_urls = vec![
            format!("{}/index.php?route=/database/export&db={}{}", base_url, db_encoded, token_param),
            format!("{}/export.php?db={}{}", base_url, db_encoded, token_param),
            format!("{}/index.php?route=/database/structure&db={}{}", base_url, db_encoded, token_param),
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
                    emit_log(app, "info", format!("HTML scraping dari '{}' menemukan {} tabel", url, tables.len()));
                    found_tables = tables;
                    break;
                } else {
                    emit_log(app, "warn", format!("HTML scraping dari '{}': tidak ada tabel (len={})", url, html.len()));
                }
            }
        }
    }

    if !found_tables.is_empty() {
        found_tables.sort();
        found_tables.dedup();
        emit_log(app, "success", format!("Ditemukan {} tabel dari database '{}'.", found_tables.len(), db));
        return Ok(found_tables);
    }

    Err(format!(
        "Tidak ada tabel yang ditemukan di database '{}'. Pastikan nama database benar, credentials memiliki akses, dan coba lakukan 'Tes Koneksi PMA' terlebih dahulu.",
        db
    ))
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
        "TABLE_NAME", "table_name", "Tables_in_db", "Name", "name",
        "Table", "table",
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
        !matches!(nl.as_str(),
            "phpmyadmin" | "index" | "true" | "false" | "select" | "structure" |
            "sql" | "export" | "import" | "checkall" | "select_all" | "uncheck_all" |
            "none" | "null" | "yes" | "no" | "ok" | "on" | "off" | "go" |
            "db" | "server" | "action" | "table" | "view" | "all" | "new" |
            "ansi" | "db2" | "maxdb" | "mssql" | "mysql323" | "mysql40" | "oracle" |
            "traditional" | "insert" | "replace" | "update" | "structure_and_data" |
            "texytext" | "textext" | "toon" | "win" | "xml" | "yaml" | "zip" |
            "gzip" | "bzip2" | "codegen" | "csv" | "excel" | "htmldir" | "htmlword" |
            "json" | "latex" | "mediawiki" | "ods" | "odt" | "pdf" | "phparray" |
            "shift_jis" | "sjis" | "utf8" | "utf8mb4" | "latin1" | "ascii" |
            "quick" | "custom" | "quick_export" | "sendit" | "asfile"
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
        if line_lower.contains("<select") && (line_lower.contains("table_select") || line_lower.contains("table_structure") || line_lower.contains("selected_tbl")) {
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
async fn stream_export_single_table(
    client: &reqwest::Client,
    base_url: &str,
    csrf_token: &str,
    pma_config: &PmaExportConfig,
    local_config: &LocalDbConfig,
    table_name: &str,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let sql_type_val = if pma_config.sync_mode.as_deref() == Some("fresh") {
        "INSERT"
    } else {
        "REPLACE"
    };

    let export_urls = vec![
        format!("{}/export.php", base_url),
        format!("{}/index.php?route=/export", base_url),
        format!("{}/index.php?route=/table/export", base_url),
        format!("{}/index.php?route=/database/export", base_url),
    ];

    let compressions = vec!["gzip", "none"];

    let mut valid_response: Option<(reqwest::Response, bool)> = None; // (response, is_gzip)
    let mut last_html_err = String::new();

    'endpoint_loop: for export_url in &export_urls {
        for compression_mode in &compressions {
            let is_gz = *compression_mode == "gzip";
            let mut form = vec![
                ("db", pma_config.database.as_str()),
                ("table", table_name),
                ("table_select[]", table_name),
                ("table_structure[]", table_name),
                ("table_data[]", table_name),
                ("single_table", "true"),
                ("what", "sql"),
                ("export_type", "table"),
                ("export_method", "quick"),
                ("quick_or_custom", "quick"),
                ("quick_export", "true"),
                ("output_format", "sendit"),
                ("compression", compression_mode),
                ("asfile", "sendit"),
                ("sql_structure_or_data", "structure_and_data"),
                ("sql_if_not_exists", "true"),
                ("sql_auto_increment", "1"),
                ("sql_backquotes", "1"),
                ("sql_type", sql_type_val),
            ];

            if !csrf_token.is_empty() {
                form.push(("token", csrf_token));
            }

            emit_log(app, "info", format!("[Tabel '{}'] POST request ke export (URL: {}, comp={})...", table_name, export_url, compression_mode));

            let response_res = client.post(export_url).form(&form).send().await;
            let response = match response_res {
                Ok(r) => r,
                Err(e) => {
                    emit_log(app, "warn", format!("[Tabel '{}'] POST ke {} gagal: {}", table_name, export_url, e));
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
                emit_log(app, "warn", format!("[Tabel '{}'] Endpoint {} (comp={}) mengembalikan HTML response: {}", table_name, export_url, compression_mode, last_html_err));
                continue;
            }

            emit_log(app, "info", format!("[Tabel '{}'] Endpoint PMA valid ditemukan ({}, Content-Type: {})", table_name, export_url, content_type));
            valid_response = Some((response, is_gz));
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

    let host = if local_config.host.is_empty() { "127.0.0.1" } else { &local_config.host };
    let port_str = if local_config.port == 0 { "3306".to_string() } else { local_config.port.to_string() };
    let db_name = if local_config.database.is_empty() { "db_sync" } else { &local_config.database };

    if pma_config.sync_mode.as_deref() == Some("fresh") {
        emit_log(app, "info", format!("[Tabel '{}'] Mode Fresh Sync — Menghapus (DROP TABLE IF EXISTS) tabel lokal terlebih dahulu...", table_name));
        let mut drop_cmd = Command::new("mysql");
        drop_cmd.arg("-h").arg(host).arg("-P").arg(&port_str).arg("-u").arg(&local_config.username);
        if !local_config.password.is_empty() {
            drop_cmd.arg(format!("-p{}", local_config.password));
        }
        drop_cmd.arg(db_name).arg("-e").arg(format!("DROP TABLE IF EXISTS `{}`;", table_name.replace('`', "``")));
        let _ = drop_cmd.output();
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

    let mut mysql_stdin = child.stdin.take().ok_or_else(|| "Gagal membuka STDIN child process mysql".to_string())?;

    // Channel for streaming HTTP bytes to decompression reader thread
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    let channel_reader = ChannelReader { rx, buffer: Vec::new(), cursor: 0 };

    let pipe_handle = tokio::task::spawn_blocking(move || {
        if is_gzip {
            let mut gz_decoder = GzDecoder::new(channel_reader);
            std::io::copy(&mut gz_decoder, &mut mysql_stdin)
        } else {
            let mut reader = channel_reader;
            std::io::copy(&mut reader, &mut mysql_stdin)
        }
    });

    let mut stream = response.bytes_stream();
    let mut total_bytes: usize = 0;

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(bytes) => {
                total_bytes += bytes.len();
                if tx.send(bytes.to_vec()).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                emit_log(app, "error", format!("[Tabel '{}'] Error saat streaming bytes dari PMA: {}", table_name, e));
                return Err(format!("Stream error: {}", e));
            }
        }
    }

    drop(tx);

    let copy_result = pipe_handle.await.map_err(|e| format!("Join error: {}", e))?;
    let processed_bytes = copy_result.map_err(|e| format!("Stream read / STDIN write error: {}", e))?;

    let output = child.wait_with_output().map_err(|e| format!("Gagal menghentikan child process mysql: {}", e))?;
    if !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        emit_log(app, "error", format!("[Tabel '{}'] Executable MySQL gagal: {}", table_name, stderr_str));
        return Err(format!("MySQL CLI import error: {}", stderr_str));
    }

    emit_log(
        app,
        "success",
        format!(
            "[Tabel '{}'] Selesai! Streamed {} KB HTTP -> {} KB SQL langsung ke MySQL lokal.",
            table_name,
            total_bytes / 1024,
            processed_bytes / 1024
        ),
    );

    Ok(())
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
    let host = if local_config.host.is_empty() { "127.0.0.1" } else { &local_config.host };
    let port_str = if local_config.port == 0 { "3306".to_string() } else { local_config.port.to_string() };

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

    let db_name = if local_config.database.is_empty() { "db_sync" } else { &local_config.database };
    let create_sql = format!("CREATE DATABASE IF NOT EXISTS `{}`;", db_name.replace('`', "``"));

    cmd.arg("-e").arg(&create_sql);

    match cmd.output() {
        Ok(out) => {
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                emit_log(app, "warn", format!("Penyiapan DB lokal '{}': {}", db_name, err));
            } else {
                emit_log(app, "info", format!("Database lokal '{}' terverifikasi siap.", db_name));
            }
        }
        Err(e) => {
            emit_log(app, "warn", format!("Gagal memeriksa/membuat DB lokal via CLI mysql: {}", e));
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
    emit_log(&app, "info", "🚀 Memulai Sinkronisasi via Direct SQL/GZIP Stream (export.php)...");

    // Ensure target local database exists before streaming tables
    ensure_local_database_exists(&local_config, &app);

    let (client, base_url, csrf_token) = authenticate_pma(&pma_config, &app).await?;

    let tables = if pma_config.tables.is_empty() {
        fetch_pma_tables(&pma_config, &app).await?
    } else {
        pma_config.tables.clone()
    };

    let total_tables = tables.len();
    emit_log(&app, "info", format!("Total {} tabel akan disinkronkan berurutan.", total_tables));

    let throttle_ms = pma_config.throttle_ms.unwrap_or(400);

    for (idx, table_name) in tables.iter().enumerate() {
        emit_progress(&app, idx + 1, total_tables, table_name, 0, idx, "syncing");

        match stream_export_single_table(&client, &base_url, &csrf_token, &pma_config, &local_config, table_name, &app).await {
            Ok(_) => {
                emit_progress(&app, idx + 1, total_tables, table_name, 1, idx + 1, "syncing");
            }
            Err(e) => {
                emit_log(&app, "error", format!("⚠️ Gagal memproses tabel '{}': {}. Menghentikan eksekusi.", table_name, e));
                emit_progress(&app, idx + 1, total_tables, table_name, 0, idx, "error");
                return Err(format!("Proses sinkronisasi dihentikan karena error pada tabel '{}': {}", table_name, e));
            }
        }

        // Throttle sleep to prevent remote server CPU spike
        if idx + 1 < total_tables {
            tokio::time::sleep(Duration::from_millis(throttle_ms)).await;
        }
    }

    emit_progress(&app, total_tables, total_tables, "", 0, total_tables, "finished");
    emit_log(&app, "success", format!("🎉 Direct SQL/GZIP Stream Sync Berhasil! {} tabel telah disinkronkan.", total_tables));

    Ok(format!("Berhasil menyinkronkan {} tabel via Direct GZIP Stream.", total_tables))
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
