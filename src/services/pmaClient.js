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
      this.updateCookies(initRes.headers);

      // Step 2: If credentials provided, attempt PMA Login POST
      if (this.username) {
        const loginUrl = this.getEndpoint('index.php?route=/login') || currentUrl;
        const formData = new URLSearchParams();
        formData.append('pma_username', this.username);
        formData.append('pma_password', this.password);
        formData.append('server', '1');
        formData.append('target', 'index.php');
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

        // Fallback POST to index.php if route=/login fails
        let loginHtml = await loginRes.text();
        if (loginHtml.includes('404') || loginHtml.includes('Cannot POST') || !loginRes.ok) {
          const altLoginUrl = this.getEndpoint('index.php');
          loginRes = await safeFetch(altLoginUrl, {
            method: 'POST',
            headers,
            body: formData.toString(),
          });
          loginHtml = await loginRes.text();
        }

        this.updateCookies(loginRes.headers);
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
                       html.match(/value=["']([^"']+)["']\s+name=["']token["']/i) ||
                       html.match(/["']token["']:\s*["']([^"']+)["']/i) ||
                       html.match(/token=([a-zA-Z0-9_-]{16,})/i) ||
                       html.match(/pma_token\s*=\s*["']([^"']+)["']/i);
    return tokenMatch ? tokenMatch[1] : '';
  }

  /**
   * Fetch list of tables existing in remote PMA database.
   * Uses the structure endpoint as the primary metadata source and falls back to SQL-backed discovery only when needed.
   */
  async fetchTablesList() {
    if (!this.database) {
      throw new Error('Nama database remote PMA belum dikonfigurasi.');
    }

    // Strict MySQL table name validator: [a-zA-Z_][a-zA-Z0-9_]{1,63}
    const isValidTableName = (name) => {
      if (!name || typeof name !== 'string') return false;
      const n = name.trim();
      if (n.length < 2 || n.length > 64) return false;
      if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(n)) return false;
      const reserved = new Set([
        'phpmyadmin', 'index', 'true', 'false', 'select', 'structure',
        'sql', 'export', 'import', 'checkall', 'select_all', 'uncheck_all',
        'none', 'null', 'yes', 'no', 'ok', 'on', 'off', 'go',
        'db', 'server', 'action', 'table', 'view', 'all', 'new',
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

    // Ambil metadata langsung dari MySQL melalui endpoint query PMA.
    const escapedDb = this.database.replace(/`/g, '``').replace(/'/g, "''");
    const infoQuery = `SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '${escapedDb}' ORDER BY TABLE_NAME LIMIT 100000`;

    try {
      rawRows = await this.executePmaAjaxSql(infoQuery);
      if (rawRows && rawRows.length > 0 && rawRows.length % 250 === 0) {
        let offset = rawRows.length;
        while (offset % 250 === 0) {
          const pagedQuery = `SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '${escapedDb}' ORDER BY TABLE_NAME LIMIT 1000 OFFSET ${offset}`;
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

    if (!rawRows || rawRows.length === 0) {
      try {
        rawRows = await this.executePmaAjaxSql(`SHOW TABLES FROM \`${escapedDb}\``);
      } catch (err) {
        console.warn('[fetchTablesList] SHOW TABLES query failed:', err.message);
      }
    }

    if (!rawRows || rawRows.length === 0) {
      try {
        const candidateUrls = [
          this.getEndpoint(`index.php?route=/database/export&db=${encodeURIComponent(this.database)}`),
          this.getEndpoint(`export.php?db=${encodeURIComponent(this.database)}`),
          this.getEndpoint(`index.php?route=/database/structure&db=${encodeURIComponent(this.database)}`),
          this.getEndpoint(`index.php?db=${encodeURIComponent(this.database)}`),
        ];

        const headers = { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' };
        if (this.cookieHeader) headers.Cookie = this.cookieHeader;

        const tableNames = new Set();
        for (const url of candidateUrls) {
          try {
            const databaseRes = await safeFetch(url, { method: 'GET', headers });
            const databaseHtml = await databaseRes.text();

            // Pattern 1: data-table="table_name" — most reliable
            for (const match of databaseHtml.matchAll(/data-table="([^"]+)"/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }

            // Pattern 2: options inside <select name="table_select[]"> block
            const tableSelectMatch = databaseHtml.match(/<select[^>]*name=["']table_select[^"']*["'][^>]*>([\s\S]*?)<\/select>/i);
            if (tableSelectMatch && tableSelectMatch[1]) {
              for (const match of tableSelectMatch[1].matchAll(/value=["']([^"']+)["']/gi)) {
                if (isValidTableName(match[1])) tableNames.add(match[1].trim());
              }
            }

            // Pattern 3: selected_tbl / table_select / table_structure checkboxes
            for (const match of databaseHtml.matchAll(/(?:selected_tbl|table_select|table_structure|table_data)[^>]*value=["']([^"']+)["']/gi)) {
              if (isValidTableName(match[1])) tableNames.add(match[1].trim());
            }

            // Pattern 4: table= in query strings
            for (const match of databaseHtml.matchAll(/[?&](?:table|dbtable)=([a-zA-Z_][a-zA-Z0-9_]*)/gi)) {
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
    const sqlUrl = this.getEndpoint('index.php?route=/sql');

    const formData = new URLSearchParams();
    formData.append('db', this.database);
    formData.append('table', this.table || '');
    formData.append('server', '1');
    formData.append('sql_query', sqlQuery);
    formData.append('sql_delimiter', ';');
    formData.append('ajax_request', 'true');
    formData.append('ajax_page_request', 'true');
    formData.append('submit_query', 'Go');
    formData.append('session_max_rows', 'all');
    formData.append('max_rows', '100000');
    formData.append('limit', '100000');
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

    let resData = null;
    try {
      resData = JSON.parse(rawText);
    } catch (_) {
      throw new Error('Endpoint /sql PMA tidak mengembalikan format JSON yang didukung untuk query data.');
    }

    if (!resData) return [];

    if (resData.success === false) {
      const errMsg = resData.error || resData.message || 'Unknown PMA error';
      const plainErr = errMsg.replace(/<[^>]*>/g, '').trim();
      throw new Error(plainErr);
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
