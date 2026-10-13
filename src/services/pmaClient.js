import { safeFetch } from './tauriHelper.js';

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
    this.baseUrl = config.url ? config.url.trim().replace(/\/index\.php.*$/, '').replace(/\/$/, '') : '';
    this.username = (config.username || '').trim();
    this.password = config.password || '';
    this.database = (config.database || '').trim();
    this.table = (config.table || '').trim();
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

  updateCookies(responseHeaders) {
    if (!responseHeaders) return;

    let setCookieHeaders = [];
    if (typeof responseHeaders.getSetCookie === 'function') {
      setCookieHeaders = responseHeaders.getSetCookie();
    } else {
      const raw = responseHeaders.get('set-cookie');
      if (raw) {
        setCookieHeaders = raw.split(/,\s*(?=[A-Za-z0-9_%\-]+=[^;]+)/);
      }
    }

    const currentMap = new Map();
    if (this.cookieHeader) {
      this.cookieHeader.split(';').forEach((pair) => {
        const [k, v] = pair.split('=').map((s) => s.trim());
        if (k && v) currentMap.set(k, v);
      });
    }

    for (const headerStr of setCookieHeaders) {
      const firstPair = headerStr.split(';')[0].trim();
      const eqIdx = firstPair.indexOf('=');
      if (eqIdx > 0) {
        const k = firstPair.slice(0, eqIdx).trim();
        const v = firstPair.slice(eqIdx + 1).trim();
        if (k && v) currentMap.set(k, v);
      }
    }

    const merged = [];
    currentMap.forEach((v, k) => merged.push(`${k}=${v}`));
    this.cookieHeader = merged.join('; ');
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
      const metaRedirect = initHtml.match(/<meta[^>]*http-equiv=["']?Refresh["']?[^>]*content=["'][^"']*url=([^"';\s>]+)/i) ||
                           initHtml.match(/(?:window\.)?location(?:\.href)?\s*=\s*(?:decodeURI\()?["']([^"']+)["']/i) ||
                           initHtml.match(/location\.replace\s*\(\s*["']([^"']+)["']/i);

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
      this.updateCookies(initRes.headers);

      // Step 2: Check if session is already authenticated or credentials provided
      const isAlreadyAuth = (initHtml.includes('id="pma_navigation"') ||
                             initHtml.includes("id='pma_navigation'") ||
                             initHtml.includes('floating_user') ||
                             initHtml.includes('serverinfo') ||
                             initHtml.includes('route=/server/databases') ||
                             initHtml.includes('db_structure.php') ||
                             initHtml.includes('id="pma_console"') ||
                             initHtml.includes('navigation_tree')) &&
                            !initHtml.includes('name="pma_password"') &&
                            !initHtml.includes('id="input_password"');

      if (this.username && !isAlreadyAuth) {
        const formActionMatch = initHtml.match(/<form[^>]+id=["']login_form["'][^>]*action=["']([^"']+)["']/i) ||
                                initHtml.match(/<form[^>]*action=["']([^"']+)["']/i);
        let formAction = formActionMatch ? formActionMatch[1] : 'index.php';
        if (formAction.includes('route=/login')) formAction = 'index.php';
        const loginUrl = new URL(formAction, currentUrl).href;

        const formData = new URLSearchParams();
        formData.append('pma_username', this.username);
        formData.append('pma_password', this.password);

        // Extract hidden inputs from initial HTML: set_session, token, server, target
        const setSessionMatch = initHtml.match(/name=["']set_session["']\s+value=["']([^"']+)["']/i) ||
                                initHtml.match(/value=["']([^"']+)["']\s+name=["']set_session["']/i);
        if (setSessionMatch && setSessionMatch[1]) {
          formData.append('set_session', setSessionMatch[1]);
        }

        const serverMatch = initHtml.match(/name=["']server["']\s+value=["']([^"']+)["']/i) ||
                            initHtml.match(/value=["']([^"']+)["']\s+name=["']server["']/i);
        formData.append('server', serverMatch ? serverMatch[1] : '1');

        const targetMatch = initHtml.match(/name=["']target["']\s+value=["']([^"']+)["']/i) ||
                            initHtml.match(/value=["']([^"']+)["']\s+name=["']target["']/i);
        formData.append('target', targetMatch ? targetMatch[1] : 'index.php');

        if (this.database) formData.append('db', this.database);
        if (this.token) formData.append('token', this.token);

        const headers = {
          'Content-Type': 'application/x-www-form-urlencoded',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) DB-Sync-Client/1.0',
        };
        if (this.cookieHeader) {
          headers['Cookie'] = this.cookieHeader;
        }

        let loginRes = await safeFetch(loginUrl, {
          method: 'POST',
          headers,
          body: formData.toString(),
        });

        let loginHtml = await loginRes.text();
        this.updateCookies(loginRes.headers);
        const newToken = this.extractTokenFromHtml(loginHtml);
        if (newToken) {
          this.token = newToken;
        }

        const isAuthSuccess = loginHtml.includes('pma_navigation') ||
                              loginHtml.includes('floating_user') ||
                              loginHtml.includes('serverinfo') ||
                              loginHtml.includes('databases') ||
                              loginHtml.includes('"success":true');
        const hasPasswordField = loginHtml.includes('name="pma_password"') || loginHtml.includes('id="input_password"');

        if (hasPasswordField && !isAuthSuccess) {
          const errText = this.extractPmaError(loginHtml);
          throw new Error(`Login PMA Gagal: ${errText || 'Kredensial ditolak atau form login masih tampil'}`);
        }
      }

      return {
        success: true,
        message: 'Koneksi ke PhpMyAdmin remote berhasil diinisialisasi.',
        token: this.token,
      };
    } catch (err) {
      const msg = err?.message || (typeof err === 'string' ? err : JSON.stringify(err));
      throw new Error(`Gagal terhubung ke PMA (${this.baseUrl}): ${msg}`);
    }
  }

  /**
   * Helper to parse error banner from PMA HTML response, stripping noscript and template alerts
   */
  extractPmaError(html) {
    if (!html) return '';
    const cleanHtml = html.replace(/<noscript[\s\S]*?<\/noscript>/gi, '');
    const matches = cleanHtml.matchAll(/class=["'](?:alert alert-danger|error|message error)["'][^>]*>([\s\S]*?)<\/div>/gi);
    for (const match of matches) {
      const text = match[1].replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
      if (text &&
          !text.includes('Javascript must be enabled') &&
          !text.includes('mismatch between HTTPS') &&
          text.length < 500) {
        return text;
      }
    }
    if (cleanHtml.includes('Cannot log in to the database server') || cleanHtml.includes('Cannot log in to the MySQL server')) {
      return 'Cannot log in to the database server (koneksi MySQL ditolak)';
    }
    if (cleanHtml.includes('Access denied')) {
      return 'Access denied (Username atau Password salah)';
    }
    return '';
  }

  /**
   * Helper to parse CSRF token from PMA HTML responses
   */
  extractTokenFromHtml(html) {
    if (!html) return '';
    const tokenMatch = html.match(/name=["']token["']\s+value=["']([^"']+)["']/i) ||
                       html.match(/value=["']([^"']+)["']\s+name=["']token["']/i) ||
                       html.match(/data-token=["']([^"']+)["']/i) ||
                       html.match(/["']token["']:\s*["']([^"']+)["']/i) ||
                       html.match(/CommonParams\.set\(\s*["']token["']\s*,\s*["']([^"']+)["']/i) ||
                       html.match(/token=([a-zA-Z0-9_-]{16,})/i) ||
                       html.match(/pma_token\s*=\s*["']([^"']+)["']/i);
    return tokenMatch ? tokenMatch[1] : '';
  }

  /**
   * Fetch list of tables existing in remote PMA database.
   */
  async fetchTablesList() {
    if (!this.database) {
      throw new Error('Nama database remote PMA belum dikonfigurasi.');
    }

    const isValidTableName = (name) => {
      if (!name || typeof name !== 'string') return false;
      const n = name.trim();
      if (n.length < 2 || n.length > 64) return false;
      if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(n)) return false;
      const reserved = new Set([
        'phpmyadmin', 'index', 'true', 'false', 'select', 'structure',
        'sql', 'export', 'import', 'checkall', 'select_all', 'uncheck_all',
        'none', 'null', 'yes', 'no', 'ok', 'on', 'off', 'go',
        'db', 'server', 'action', 'table', 'tables', 'table_name', 'tables_in',
        'view', 'views', 'all', 'new', 'browse', 'search', 'empty', 'drop', 'operations',
        'ansi', 'db2', 'maxdb', 'mssql', 'mysql323', 'mysql40', 'oracle',
        'traditional', 'insert', 'replace', 'update', 'structure_and_data',
        'texytext', 'textext', 'toon', 'win', 'xml', 'yaml', 'zip',
        'gzip', 'bzip2', 'codegen', 'csv', 'excel', 'htmldir', 'htmlword',
        'json', 'latex', 'mediawiki', 'ods', 'odt', 'pdf', 'phparray',
        'shift_jis', 'sjis', 'utf8', 'utf8mb4', 'latin1', 'ascii',
        'quick', 'custom', 'quick_export', 'sendit', 'asfile',
      ]);
      return !reserved.has(n.toLowerCase());
    };

    let rawRows = [];
    const escapedDb = this.database.replace(/`/g, '``').replace(/'/g, "''");

    // Query 1: case-insensitive INFORMATION_SCHEMA match
    const infoQuery = `SELECT TABLE_NAME FROM information_schema.TABLES WHERE LOWER(TABLE_SCHEMA) = LOWER('${escapedDb}') ORDER BY TABLE_NAME LIMIT 100000`;

    try {
      rawRows = await this.executePmaAjaxSql(infoQuery);
      if (rawRows && rawRows.length > 0 && rawRows.length % 250 === 0) {
        let offset = rawRows.length;
        while (offset % 250 === 0) {
          const pagedQuery = `SELECT TABLE_NAME FROM information_schema.TABLES WHERE LOWER(TABLE_SCHEMA) = LOWER('${escapedDb}') ORDER BY TABLE_NAME LIMIT 1000 OFFSET ${offset}`;
          try {
            const nextRows = await this.executePmaAjaxSql(pagedQuery);
            if (!nextRows || nextRows.length === 0) break;
            const countBefore = rawRows.length;
            rawRows.push(...nextRows);
            if (rawRows.length === countBefore) break;
            offset = rawRows.length;
          } catch (_) {
            break;
          }
        }
      }
    } catch (err) {
      console.warn('[fetchTablesList] information_schema query failed:', err.message);
    }

    // Query 2: SHOW FULL TABLES for BASE TABLE
    if (!rawRows || rawRows.length === 0) {
      try {
        rawRows = await this.executePmaAjaxSql(`SHOW FULL TABLES FROM \`${escapedDb}\` WHERE Table_type = 'BASE TABLE'`);
      } catch (err) {
        console.warn('[fetchTablesList] SHOW FULL TABLES query failed:', err.message);
      }
    }

    // Query 3: SHOW TABLES
    if (!rawRows || rawRows.length === 0) {
      try {
        rawRows = await this.executePmaAjaxSql(`SHOW TABLES FROM \`${escapedDb}\``);
      } catch (err) {
        console.warn('[fetchTablesList] SHOW TABLES query failed:', err.message);
      }
    }

    if (!rawRows || rawRows.length === 0) {
      try {
        const tokenParam = this.token ? `&token=${encodeURIComponent(this.token)}` : '';
        const candidateUrls = [
          this.getEndpoint(`index.php?route=/database/structure&db=${encodeURIComponent(this.database)}&server=1${tokenParam}`),
          this.getEndpoint(`db_structure.php?db=${encodeURIComponent(this.database)}&server=1${tokenParam}`),
          this.getEndpoint(`index.php?route=/navigation&db=${encodeURIComponent(this.database)}&server=1${tokenParam}`),
          this.getEndpoint(`navigation.php?db=${encodeURIComponent(this.database)}&server=1${tokenParam}`),
          this.getEndpoint(`index.php?route=/database/export&db=${encodeURIComponent(this.database)}&server=1&export_type=database${tokenParam}`),
          this.getEndpoint(`export.php?db=${encodeURIComponent(this.database)}&server=1&export_type=database${tokenParam}`),
          this.getEndpoint(`index.php?db=${encodeURIComponent(this.database)}&server=1${tokenParam}`),
          this.getEndpoint(`index.php?route=/database/structure&db=${encodeURIComponent(this.database)}${tokenParam}`),
          this.getEndpoint(`db_structure.php?db=${encodeURIComponent(this.database)}${tokenParam}`),
          this.getEndpoint(`export.php?db=${encodeURIComponent(this.database)}${tokenParam}`),
        ];

        const headers = { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' };
        if (this.cookieHeader) headers.Cookie = this.cookieHeader;

        const tableNames = new Set();
        for (const url of candidateUrls) {
          try {
            const databaseRes = await safeFetch(url, { method: 'GET', headers });
            const databaseHtml = await databaseRes.text();

            // Pattern 1: data-table="..." or data-filter-row="..."
            for (const match of databaseHtml.matchAll(/(?:data-table|data-name|data-filter-row)="([^"]+)"/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }

            // Pattern 2: options inside <select name="table_select[]"> block
            const tableSelectMatch = databaseHtml.match(/<select[^>]*name=["']table_select[^"']*["'][^>]*>([\s\S]*?)<\/select>/i);
            if (tableSelectMatch && tableSelectMatch[1]) {
              for (const match of tableSelectMatch[1].matchAll(/value=["']([^"']+)["']/gi)) {
                if (isValidTableName(match[1])) tableNames.add(match[1].trim());
              }
            }

            // Pattern 3: selected_tbl checkboxes
            for (const match of databaseHtml.matchAll(/(?:selected_tbl|table_select|table_structure|table_data)[^>]*value=["']([^"']+)["']/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }
            for (const match of databaseHtml.matchAll(/value=["']([^"']+)["'][^>]*(?:selected_tbl|table_select)/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }

            // Pattern 4: table= in query strings
            for (const match of databaseHtml.matchAll(/[?&](?:table|dbtable)=([a-zA-Z_][a-zA-Z0-9_]*)/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }

            // Pattern 5: table results cells
            for (const match of databaseHtml.matchAll(/<td[^>]*class=["'][^"']*data[^"']*["'][^>]*>(?:<[^>]+>)*\s*([a-zA-Z0-9_]{2,64})\s*(?:<\/[^>]+>)*<\/td>/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }

            // Pattern 6: navigation hover title
            for (const match of databaseHtml.matchAll(/hover_show_full[^>]*title=["']([^"']+)["']/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }

            if (tableNames.size > 0) break;
          } catch (e) {
            console.warn(`[fetchTablesList] URL ${url} failed:`, e.message);
          }
        }

        rawRows = Array.from(tableNames, (table_name) => ({ table_name }));
      } catch (err) {
        console.warn('[fetchTablesList] database page fallback failed:', err.message);
      }
    }

    if (!rawRows || !Array.isArray(rawRows)) return [];

    const tables = [];
    for (const row of rawRows) {
      let value = null;
      if (typeof row === 'string') {
        value = row;
      } else if (row && typeof row === 'object') {
        value = row.TABLE_NAME ?? row.table_name ?? row.Tables_in_db ?? row.Name ?? row.name ?? null;
      }

      if (isValidTableName(value)) {
        tables.push(value.trim());
      }
    }

    return Array.from(new Set(tables)).sort();
  }

  /**
   * Extract table names from PMA database structure JSON payload.
   */
  _extractTablesFromStructureJson(payload) {
    const tables = [];
    const walk = (value) => {
      if (!value) return;
      if (Array.isArray(value)) {
        value.forEach(walk);
        return;
      }
      if (typeof value !== 'object') return;

      const directName = value.table_name || value.TABLE_NAME || value.Name || value.name || value.Table || value.table || null;
      if (typeof directName === 'string' && directName.trim()) {
        tables.push(directName.trim());
      }

      for (const key of Object.keys(value)) {
        const nested = value[key];
        if (key === 'tables' || key === 'tbl_group' || key === 'data' || key === 'rows' || key === 'results' || key === 'items') {
          walk(nested);
        } else if (nested && typeof nested === 'object') {
          walk(nested);
        }
      }
    };

    walk(payload);
    return Array.from(new Set(tables)).map((t) => ({ table_name: t }));
  }

  /**
   * Execute SQL query via PMA AJAX SQL execution route (used as fallback for table listing).
   */
  async executePmaAjaxSql(sqlQuery) {
    const endpoints = [
      this.getEndpoint('index.php?route=/sql'),
      this.getEndpoint('sql.php'),
    ];

    const formData = new URLSearchParams();
    formData.append('db', this.database);
    formData.append('table', this.table || '');
    formData.append('server', '1');
    formData.append('sql_query', sqlQuery);
    formData.append('sql_delimiter', ';');
    formData.append('ajax_request', 'true');
    formData.append('submit_query', 'Go');
    formData.append('session_max_rows', 'all');
    formData.append('max_rows', '100000');
    formData.append('limit', '100000');
    if (this.token) formData.append('token', this.token);

    const headers = {
      'Content-Type': 'application/x-www-form-urlencoded',
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
      'X-Requested-With': 'XMLHttpRequest',
    };
    if (this.cookieHeader) headers['Cookie'] = this.cookieHeader;

    let res = null;
    let rawText = '';

    for (const sqlUrl of endpoints) {
      try {
        res = await safeFetch(sqlUrl, {
          method: 'POST',
          headers,
          body: formData.toString(),
        });
        rawText = await res.text();
        if (rawText && !rawText.includes('Cannot POST') && !rawText.includes('404 Not Found')) {
          break;
        }
      } catch (err) {
        console.warn(`[PMA] executePmaAjaxSql error on ${sqlUrl}:`, err.message);
      }
    }

    if (!rawText) return [];

    let resData = null;
    try {
      resData = JSON.parse(rawText);
    } catch (_) {
      // If raw response is HTML
      const htmlMatches = new Set();
      for (const m of rawText.matchAll(/<td[^>]*class=["'][^"']*data[^"']*["'][^>]*>(?:<[^>]+>)*\s*([a-zA-Z0-9_]{2,64})\s*(?:<\/[^>]+>)*<\/td>/gi)) {
        htmlMatches.add(m[1].trim());
      }
      return Array.from(htmlMatches).map((t) => ({ table_name: t }));
    }

    if (!resData) return [];

    if (resData.success === false) {
      const errMsg = resData.error || resData.message || 'Unknown PMA error';
      const plainErr = errMsg.replace(/<[^>]*>/g, '').trim();
      throw new Error(plainErr);
    }

    // Check if resData.message contains HTML table results
    if (typeof resData.message === 'string' && resData.message.includes('<')) {
      const htmlMatches = new Set();
      for (const m of resData.message.matchAll(/<td[^>]*class=["'][^"']*data[^"']*["'][^>]*>(?:<[^>]+>)*\s*([a-zA-Z0-9_]{2,64})\s*(?:<\/[^>]+>)*<\/td>/gi)) {
        htmlMatches.add(m[1].trim());
      }
      for (const m of resData.message.matchAll(/<td[^>]*>(?:<[^>]+>)*\s*([a-zA-Z0-9_]{2,64})\s*(?:<\/[^>]+>)*<\/td>/gi)) {
        htmlMatches.add(m[1].trim());
      }
      if (htmlMatches.size > 0) {
        return Array.from(htmlMatches).map((t) => ({ table_name: t }));
      }
    }

    const normalizeRows = (payload) => {
      if (!payload) return [];
      if (Array.isArray(payload)) return payload;
      if (Array.isArray(payload.fields) && Array.isArray(payload.rows)) {
        const names = payload.fields.map((field) => cleanFieldName(
          typeof field === 'object' ? (field.name || field.Name || field.Field || '') : String(field)
        ));
        return payload.rows.map((row) => {
          if (!Array.isArray(row)) return row;
          return Object.fromEntries(names.map((name, index) => [name, row[index] ?? null]));
        });
      }
      for (const value of Object.values(payload)) {
        if (value && typeof value === 'object') {
          const rows = normalizeRows(value);
          if (rows.length) return rows;
        }
      }
      return [];
    };

    const normalizedRows = normalizeRows(resData);
    if (normalizedRows.length) return normalizedRows;

    console.warn('[PMA] executePmaAjaxSql: unrecognized response shape', Object.keys(resData));
    return [];
  }

}
