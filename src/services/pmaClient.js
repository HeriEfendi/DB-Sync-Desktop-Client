import { safeFetch } from './tauriHelper.js';

/**
 * Strip PMA sort-order indicators from field names.
 * PMA sometimes appends " 1", " 2" etc. to column names in AJAX responses
 * to indicate sort position. Example: "id 1" → "id", "name 2" → "name".
 * Only strips trailing " <digit(s)>" — leaves names like "address2" untouched.
 */
function cleanFieldName(name) {
  if (!name || typeof name !== 'string') return name;
  return name.replace(/\s+\d+$/, '').trim();
}

/**
 * PhpMyAdmin HTTP Native Client Service
 * Handles authentication, CSRF token management, and query execution against remote PMA servers.
 */
export class PmaClient {
  constructor(config) {
    this.baseUrl = config.url ? config.url.trim().replace(/\/$/, '') : '';
    this.username = config.username || '';
    this.password = config.password || '';
    this.database = config.database || '';
    this.table = config.table || '';
    this.primaryKey = config.primaryKey || 'id';
    this.cookieHeader = '';
    this.token = '';
  }

  /**
   * Helper to normalize PMA base endpoint
   */
  getEndpoint(path = '') {
    const base = this.activeBaseUrl || this.baseUrl;
    if (!path) return base;
    return `${base}/${path.replace(/^\//, '')}`;
  }

  /**
   * Authenticate with remote PhpMyAdmin server & obtain session tokens
   */
  async authenticate() {
    if (!this.baseUrl) {
      throw new Error('URL PhpMyAdmin belum diisi.');
    }

    try {
      // Step 1: Initial GET to obtain session cookie & CSRF token
      let currentUrl = this.getEndpoint('index.php');
      let initRes = await safeFetch(currentUrl, {
        method: 'GET',
        headers: {
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) DB-Sync-Client/1.0',
        },
      });

      let initHtml = await initRes.text();

      // Check if response contains HTML meta-refresh redirect (e.g. redirecting to ./public/)
      const metaRedirect = initHtml.match(/<meta\s+http-equiv=["']Refresh["']\s+content=["']\d+;\s*url=([^"']+)["']/i) ||
                           initHtml.match(/window\.location\s*=\s*(?:decodeURI\()?["']([^"']+)["']/i);

      if (metaRedirect && metaRedirect[1]) {
        const redirectPath = metaRedirect[1].trim();
        const resolvedUrl = new URL(redirectPath, currentUrl).href;
        this.activeBaseUrl = resolvedUrl.replace(/\/index\.php.*$/, '').replace(/\/$/, '');

        initRes = await safeFetch(resolvedUrl, {
          method: 'GET',
          headers: {
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) DB-Sync-Client/1.0',
          },
        });
        initHtml = await initRes.text();
      }

      this.token = this.extractTokenFromHtml(initHtml);

      // Extract set-cookie if available
      const setCookie = initRes.headers.get('set-cookie');
      if (setCookie) {
        this.cookieHeader = setCookie.split(';')[0];
      }

      // Step 2: If credentials provided, attempt PMA Login POST
      if (this.username) {
        const loginUrl = this.getEndpoint('index.php?route=/login') || initUrl;
        const formData = new URLSearchParams();
        formData.append('pma_username', this.username);
        formData.append('pma_password', this.password);
        formData.append('server', '1');
        formData.append('target', 'index.php');
        if (this.token) {
          formData.append('token', this.token);
        }

        const headers = {
          'Content-Type': 'application/x-www-form-urlencoded',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) DB-Sync-Client/1.0',
        };
        if (this.cookieHeader) {
          headers['Cookie'] = this.cookieHeader;
        }

        const loginRes = await safeFetch(loginUrl, {
          method: 'POST',
          headers,
          body: formData.toString(),
        });

        const loginSetCookie = loginRes.headers.get('set-cookie');
        if (loginSetCookie) {
          this.cookieHeader = loginSetCookie.split(';')[0];
        }

        const loginHtml = await loginRes.text();
        const newToken = this.extractTokenFromHtml(loginHtml);
        if (newToken) {
          this.token = newToken;
        }
      }

      return {
        success: true,
        message: 'Koneksi ke PhpMyAdmin remote berhasil diinisialisasi.',
        token: this.token,
      };
    } catch (err) {
      throw new Error(`Gagal terhubung ke PMA (${this.baseUrl}): ${err.message}`);
    }
  }

  /**
   * Helper to parse CSRF token from PMA HTML responses
   */
  extractTokenFromHtml(html) {
    if (!html) return '';
    const tokenMatch = html.match(/name=["']token["']\s+value=["']([^"']+)["']/i) ||
                       html.match(/["']token["']:\s*["']([^"']+)["']/i) ||
                       html.match(/pma_token\s*=\s*["']([^"']+)["']/i);
    return tokenMatch ? tokenMatch[1] : '';
  }

  /**
   * Fetch list of tables existing in remote PMA database.
   * Tries multiple PMA endpoints/methods to ensure compatibility with different PMA versions.
   */
  async fetchTablesList() {
    if (!this.database) {
      throw new Error('Nama database remote PMA belum dikonfigurasi.');
    }

    // Strategy 1: SHOW TABLES via AJAX SQL endpoint
    let rawRows = [];
    let lastError = null;

    const sqlQuery = `SHOW TABLES FROM \`${this.database}\``;
    try {
      rawRows = await this.executePmaAjaxSql(sqlQuery);
    } catch (err) {
      lastError = err;
      console.warn('[fetchTablesList] AJAX SQL failed:', err.message);
    }

    // Strategy 2: Fallback via database structure route (returns tbl_info JSON in PMA6+)
    if (!rawRows || rawRows.length === 0) {
      try {
        const structUrl = this.getEndpoint(`index.php?route=/database/structure&db=${encodeURIComponent(this.database)}&ajax_request=true`);
        const headers = {
          'X-Requested-With': 'XMLHttpRequest',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
        };
        if (this.cookieHeader) headers['Cookie'] = this.cookieHeader;

        const res = await safeFetch(structUrl, { method: 'GET', headers });
        const rawText = await res.text();

        // Try JSON parse
        let json = null;
        try { json = JSON.parse(rawText); } catch (_) {}

        if (json) {
          // PMA6 database/structure returns tbl_group or message with HTML
          if (json.message) {
            rawRows = this._extractTablesFromStructureHtml(json.message);
          } else if (Array.isArray(json.tables)) {
            rawRows = json.tables.map((t) => ({ table_name: typeof t === 'string' ? t : (t.Name || t.TABLE_NAME || '') }));
          }
        }

        if (!rawRows || rawRows.length === 0) {
          // Try parsing HTML directly
          rawRows = this._extractTablesFromStructureHtml(rawText);
        }
      } catch (err) {
        console.warn('[fetchTablesList] Structure route failed:', err.message);
      }
    }

    // Strategy 3: INFORMATION_SCHEMA query as last resort
    if (!rawRows || rawRows.length === 0) {
      try {
        const infoQuery = `SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '${this.database}' ORDER BY TABLE_NAME`;
        rawRows = await this.executePmaAjaxSql(infoQuery);
      } catch (err) {
        console.warn('[fetchTablesList] information_schema fallback failed:', err.message);
        if (lastError) throw new Error(`Gagal mengambil daftar tabel: ${lastError.message}`);
      }
    }

    if (!rawRows || !Array.isArray(rawRows)) return [];

    // Extract table names from various response shapes
    const tables = [];
    for (const row of rawRows) {
      if (typeof row === 'string') {
        tables.push(row);
      } else if (row && typeof row === 'object') {
        // Try all common field names for table names
        const val = row.TABLE_NAME ?? row.table_name ?? row.Tables_in_db
          ?? Object.values(row)[0] ?? null;
        if (val && typeof val === 'string' && val.trim()) {
          tables.push(val.trim());
        }
      }
    }

    return Array.from(new Set(tables)).sort();
  }

  /**
   * Extract table names from PMA database structure HTML page
   */
  _extractTablesFromStructureHtml(html) {
    if (!html) return [];
    const parser = new DOMParser();
    const doc = parser.parseFromString(html, 'text/html');
    const tables = [];

    // PMA structure page has table links or rows with table names
    const anchors = doc.querySelectorAll('th.tbl_name a, td a[href*="table="], .tableName');
    anchors.forEach((a) => {
      const name = a.textContent.trim();
      if (name && !name.includes(' ') && name.length > 0) {
        tables.push(name);
      }
    });

    return tables.map((t) => ({ table_name: t }));
  }

  /**
   * Execute incremental query SELECT * FROM table WHERE pk > lastId ORDER BY pk ASC LIMIT fetchLimit
   */
  async fetchIncrementalData(lastId = 0, limit = 500) {
    if (!this.database || !this.table) {
      throw new Error('Nama database dan tabel remote PMA belum dikonfigurasi.');
    }

    let sqlQuery = '';
    if (lastId !== null && lastId !== undefined && lastId !== '') {
      const formattedLastId = typeof lastId === 'number' ? lastId : `'${lastId}'`;
      sqlQuery = `SELECT * FROM \`${this.table}\` WHERE \`${this.primaryKey}\` > ${formattedLastId} ORDER BY \`${this.primaryKey}\` ASC LIMIT ${limit}`;
    } else {
      sqlQuery = `SELECT * FROM \`${this.table}\` ORDER BY \`${this.primaryKey}\` ASC LIMIT ${limit}`;
    }

    console.debug('[PMA] fetchIncrementalData SQL:', sqlQuery);

    // Try Export method first (returns structured JSON directly)
    try {
      const jsonExportData = await this.executePmaJsonExport(sqlQuery);
      if (jsonExportData && Array.isArray(jsonExportData) && jsonExportData.length > 0) {
        console.debug(`[PMA] fetchIncrementalData via JSON Export: ${jsonExportData.length} rows`);
        return jsonExportData;
      }
      console.debug('[PMA] JSON Export returned empty/null, falling back to AJAX SQL...');
    } catch (e) {
      console.warn('[PMA] JSON Export fallback needed, attempting AJAX SQL endpoint:', e.message);
    }

    // Fallback to AJAX SQL Query execution
    const ajaxResult = await this.executePmaAjaxSql(sqlQuery);
    console.debug(`[PMA] fetchIncrementalData via AJAX SQL: ${ajaxResult?.length ?? 0} rows`);
    return ajaxResult;
  }

  /**
   * Fetch rows from PMA where updated_at > sinceTimestamp (for update detection in incremental mode).
   * Returns empty array if table has no updated_at column or no newer rows found.
   */
  async fetchUpdatedRows(sinceTimestamp, limit = 500, offsetRows = 0) {
    if (!this.database || !this.table) {
      throw new Error('Nama database dan tabel remote PMA belum dikonfigurasi.');
    }

    const safeTs = sinceTimestamp.replace(/'/g, "''");
    const sqlQuery = `SELECT * FROM \`${this.table}\` WHERE \`updated_at\` > '${safeTs}' ORDER BY \`updated_at\` ASC LIMIT ${limit} OFFSET ${offsetRows}`;

    console.debug('[PMA] fetchUpdatedRows SQL:', sqlQuery);

    try {
      const jsonExportData = await this.executePmaJsonExport(sqlQuery);
      if (jsonExportData && Array.isArray(jsonExportData) && jsonExportData.length > 0) {
        return jsonExportData;
      }
    } catch (e) {
      console.warn('[PMA] fetchUpdatedRows JSON Export failed, trying AJAX:', e.message);
    }

    const ajaxResult = await this.executePmaAjaxSql(sqlQuery);
    return ajaxResult || [];
  }

  /**
   * Execute SQL query via PMA JSON Export route
   */
  async executePmaJsonExport(sqlQuery) {
    const exportUrl = this.getEndpoint('index.php?route=/export') || this.getEndpoint('export.php');
    
    const formData = new URLSearchParams();
    formData.append('db', this.database);
    formData.append('table', this.table);
    formData.append('single_table', 'true');
    formData.append('export_type', 'table');
    formData.append('export_method', 'quick');
    formData.append('what', 'json');
    formData.append('sql_query', sqlQuery);
    formData.append('knob_sql_query', sqlQuery);
    if (this.token) formData.append('token', this.token);

    const headers = {
      'Content-Type': 'application/x-www-form-urlencoded',
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
    };
    if (this.cookieHeader) headers['Cookie'] = this.cookieHeader;

    const res = await safeFetch(exportUrl, {
      method: 'POST',
      headers,
      body: formData.toString(),
    });

    const responseText = await res.text();
    return this.parseJsonExportContent(responseText);
  }

  /**
   * Execute SQL query via PMA AJAX SQL execution route.
   * Handles both classic HTML table responses and PMA6+ structured JSON API responses.
   */
  async executePmaAjaxSql(sqlQuery) {
    const sqlUrl = this.getEndpoint('index.php?route=/sql');

    const formData = new URLSearchParams();
    formData.append('db', this.database);
    formData.append('table', this.table || '');
    formData.append('sql_query', sqlQuery);
    formData.append('ajax_request', 'true');
    formData.append('token', this.token);

    const headers = {
      'Content-Type': 'application/x-www-form-urlencoded',
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
      'X-Requested-With': 'XMLHttpRequest',
    };
    if (this.cookieHeader) headers['Cookie'] = this.cookieHeader;

    const res = await safeFetch(sqlUrl, {
      method: 'POST',
      headers,
      body: formData.toString(),
    });

    const rawText = await res.text();
    console.debug('[PMA] executePmaAjaxSql raw response (first 500 chars):', rawText.slice(0, 500));

    // Try parsing as JSON (PMA returns JSON for AJAX calls)
    let resData = null;
    try {
      resData = JSON.parse(rawText);
    } catch (_) {
      // Not JSON: try to parse HTML table directly
      return this.parsePmaHtmlTable(rawText);
    }

    if (!resData) return [];

    // PMA returns success=false with error message
    if (resData.success === false) {
      const errMsg = resData.error || resData.message || 'Unknown PMA error';
      // Strip HTML tags from PMA error strings
      const plainErr = errMsg.replace(/<[^>]*>/g, '').trim();
      throw new Error(plainErr);
    }

    // PMA6+ structured API: fields[] + rows[] format (used for SELECT/SHOW queries)
    if (Array.isArray(resData.fields) && Array.isArray(resData.rows)) {
      const fieldNames = resData.fields.map((f) => {
        const raw = typeof f === 'object' ? (f.name || f.Field || String(f)) : String(f);
        return cleanFieldName(raw);
      });
      return resData.rows.map((row) => {
        const obj = {};
        fieldNames.forEach((name, idx) => {
          obj[name] = Array.isArray(row) ? row[idx] : row[name] ?? null;
        });
        return obj;
      });
    }

    // PMA classic: message contains HTML table
    if (resData.message) {
      return this.parsePmaHtmlTable(resData.message);
    }

    // PMA sometimes returns resultset as array directly
    if (Array.isArray(resData)) return resData;

    console.warn('[PMA] executePmaAjaxSql: unrecognized response shape', Object.keys(resData));
    return [];
  }

  /**
   * Parse PMA JSON Export response payload
   */
  parseJsonExportContent(content) {
    if (!content) return [];
    
    try {
      const parsed = JSON.parse(content);
      if (Array.isArray(parsed)) {
        let rows = [];
        for (const item of parsed) {
          if (item && (item.type === 'header' || item.type === 'database')) continue;
          if (item && item.type === 'table' && Array.isArray(item.data)) {
            rows.push(...item.data);
          } else if (item && typeof item === 'object' && !item.type) {
            rows.push(item);
          }
        }
        return rows.length > 0 ? rows : parsed.filter((i) => !i.type);
      }
      if (typeof parsed === 'object') {
        for (const key of Object.keys(parsed)) {
          if (Array.isArray(parsed[key])) {
            return parsed[key];
          }
        }
      }
    } catch (_) {
      const match = content.match(/\[\s*\{.*\}\s*\]/s);
      if (match) {
        return JSON.parse(match[0]);
      }
    }
    return [];
  }

  /**
   * Parse HTML Table from PMA AJAX response into array of JSON objects
   * Uses direct td data-column-name attributes for precision column matching.
   */
  parsePmaHtmlTable(html) {
    if (!html) return [];

    const parser = new DOMParser();
    const doc = parser.parseFromString(html, 'text/html');

    const table = doc.querySelector('table.table_results') || doc.querySelector('table');
    if (!table) return [];

    // Map column names from <thead> th elements
    const headerColsMap = [];
    const headerThs = table.querySelectorAll('thead tr th, tr:first-child th');
    headerThs.forEach((th) => {
      let colName = th.getAttribute('data-column-name');
      if (!colName) {
        const anchor = th.querySelector('a');
        if (anchor) colName = anchor.textContent.trim();
      }
      if (!colName) {
        const firstText = [...th.childNodes].find((n) => n.nodeType === Node.TEXT_NODE && n.textContent.trim());
        if (firstText) colName = firstText.textContent.trim();
      }
      if (colName) {
        colName = cleanFieldName(colName);
      }
      if (colName && !['Edit', 'Copy', 'Delete', ''].includes(colName)) {
        headerColsMap.push(colName);
      } else {
        headerColsMap.push(null);
      }
    });

    const validHeaders = headerColsMap.filter(Boolean);
    const rows = [];
    const trElements = table.querySelectorAll('tbody tr, tr.odd, tr.even');

    trElements.forEach((tr) => {
      const cells = tr.querySelectorAll('td');
      if (cells.length === 0) return;

      const rowObj = {};
      let fallbackColIdx = 0;

      cells.forEach((td, cellIdx) => {
        if (td.classList.contains('edit_row_anchor') ||
            td.classList.contains('select_row') ||
            td.classList.contains('del_row')) {
          return;
        }

        // Prioritas 1: Ambil nama kolom langsung dari atribut data-column-name sel td!
        let key = td.getAttribute('data-column-name');
        if (key) {
          key = cleanFieldName(key);
        }

        // Prioritas 2: Pemetaan dari index sel jika data-column-name tidak ada pada td
        if (!key && cellIdx < headerColsMap.length) {
          key = headerColsMap[cellIdx];
        }

        // Prioritas 3: Fallback urutan validHeaders
        if (!key && fallbackColIdx < validHeaders.length) {
          key = validHeaders[fallbackColIdx];
        }

        if (key && !['Edit', 'Copy', 'Delete', ''].includes(key)) {
          let val = td.innerText.trim();
          if (val === 'NULL' || val === 'null') {
            val = null;
          } else if (/^-?\d+$/.test(val)) {
            val = parseInt(val, 10);
          } else if (/^-?\d+\.\d+$/.test(val)) {
            val = parseFloat(val);
          }
          rowObj[key] = val;
          fallbackColIdx++;
        }
      });

      if (Object.keys(rowObj).length > 0) {
        rows.push(rowObj);
      }
    });

    return rows;
  }
}
