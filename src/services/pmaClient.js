import { safeFetch } from './tauriHelper.js';

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
    if (!path) return this.baseUrl;
    return `${this.baseUrl}/${path.replace(/^\//, '')}`;
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
      const initUrl = this.getEndpoint('index.php');
      const initRes = await safeFetch(initUrl, {
        method: 'GET',
        headers: {
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) DB-Sync-Client/1.0',
        },
      });

      const initHtml = await initRes.text();
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

    // Try Export method first (returns structured JSON directly)
    try {
      const jsonExportData = await this.executePmaJsonExport(sqlQuery);
      if (jsonExportData && Array.isArray(jsonExportData)) {
        return jsonExportData;
      }
    } catch (e) {
      console.warn('JSON Export fallback needed, attempting AJAX SQL endpoint:', e.message);
    }

    // Fallback to AJAX SQL Query execution
    return await this.executePmaAjaxSql(sqlQuery);
  }

  /**
   * Execute SQL query via PMA JSON Export route
   */
  async executePmaJsonExport(sqlQuery) {
    const exportUrl = this.getEndpoint('index.php?route=/export/template') || this.getEndpoint('export.php');
    
    const formData = new URLSearchParams();
    formData.append('db', this.database);
    formData.append('table', this.table);
    formData.append('export_type', 'database');
    formData.append('export_method', 'quick');
    formData.append('what', 'json');
    formData.append('sql_query', sqlQuery);
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
   * Execute SQL query via PMA AJAX SQL execution route
   */
  async executePmaAjaxSql(sqlQuery) {
    const sqlUrl = this.getEndpoint('index.php?route=/sql') || this.getEndpoint('import.php');

    const formData = new URLSearchParams();
    formData.append('db', this.database);
    formData.append('table', this.table);
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

    const resData = await res.json();
    if (resData.success === false && resData.error) {
      throw new Error(resData.error);
    }

    const messageHtml = resData.message || (typeof resData === 'string' ? resData : '');
    return this.parsePmaHtmlTable(messageHtml);
  }

  /**
   * Parse PMA JSON Export response payload
   */
  parseJsonExportContent(content) {
    if (!content) return [];
    
    try {
      const parsed = JSON.parse(content);
      if (Array.isArray(parsed)) return parsed;
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
   */
  parsePmaHtmlTable(html) {
    if (!html) return [];

    const parser = new DOMParser();
    const doc = parser.parseFromString(html, 'text/html');

    const table = doc.querySelector('table.table_results') || doc.querySelector('table');
    if (!table) return [];

    const headers = [];
    const headerElements = table.querySelectorAll('th');
    headerElements.forEach((th) => {
      const colName = th.getAttribute('data-column-name') || th.innerText.trim();
      if (colName && !['Edit', 'Copy', 'Delete', ''].includes(colName)) {
        headers.push(colName);
      }
    });

    const rows = [];
    const trElements = table.querySelectorAll('tbody tr, tr.odd, tr.even');

    trElements.forEach((tr) => {
      const cells = tr.querySelectorAll('td');
      if (cells.length === 0) return;

      const rowObj = {};
      let colIdx = 0;

      cells.forEach((td) => {
        if (td.classList.contains('edit_row_anchor') || td.classList.contains('select_row') || td.classList.contains('del_row')) {
          return;
        }

        if (colIdx < headers.length) {
          const key = headers[colIdx];
          let val = td.innerText.trim();

          if (val === 'NULL' || val === 'null') {
            val = null;
          } else if (/^-?\d+$/.test(val)) {
            val = parseInt(val, 10);
          } else if (/^-?\d+\.\d+$/.test(val)) {
            val = parseFloat(val);
          }

          rowObj[key] = val;
          colIdx++;
        }
      });

      if (Object.keys(rowObj).length > 0) {
        rows.push(rowObj);
      }
    });

    return rows;
  }
}
