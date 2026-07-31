import { safeInvoke } from './tauriHelper.js';
import { PmaClient } from './pmaClient.js';

export class SyncEngine {
  constructor(pmaConfig, localDbConfig, options = {}) {
    this.pmaConfig = pmaConfig;
    this.localDbConfig = localDbConfig;
    this.onLog = options.onLog || (() => {});
    this.isSyncing = false;
  }

  log(type, message) {
    this.onLog({
      type, // 'info' | 'success' | 'warning' | 'error'
      message,
      timestamp: new Date().toLocaleTimeString(),
    });
  }

  /**
   * Execute full incremental sync process:
   * 1. Get last ID stored in local MySQL via Rust IPC (safeInvoke)
   * 2. Authenticate & Query remote PhpMyAdmin for rows > last ID
   * 3. Send fetched array to Rust IPC for bulk upsert to local MySQL
   */
  async runIncrementalSync(fetchLimit = 500) {
    if (this.isSyncing) {
      this.log('warning', 'Proses sinkronisasi sedang berjalan...');
      return { success: false, reason: 'already_running' };
    }

    this.isSyncing = true;
    const startTime = performance.now();

    try {
      this.log('info', `[1/4] Menghubungkan ke MySQL lokal di ${this.localDbConfig.host}:${this.localDbConfig.port || 3306}...`);

      // Step 1: Query local MySQL to get latest primary key value
      let lastId = null;
      try {
        lastId = await safeInvoke('get_last_local_id', {
          config: {
            host: this.localDbConfig.host || '127.0.0.1',
            port: parseInt(this.localDbConfig.port || 3306, 10),
            username: this.localDbConfig.username || 'root',
            password: this.localDbConfig.password || '',
            database: this.localDbConfig.database || '',
          },
          tableName: this.localDbConfig.table || this.pmaConfig.table,
          primaryKey: this.pmaConfig.primaryKey || 'id',
        });
        
        this.log('info', `State ID lokal saat ini (${this.pmaConfig.primaryKey}): ${lastId !== null && lastId !== undefined ? lastId : 'Kosong / Belum ada data'}`);
      } catch (err) {
        this.log('error', `Gagal mengecek state MySQL lokal: ${err.message || err}`);
        throw err;
      }

      // Step 2: Initialize PMA Client & Authenticate
      this.log('info', `[2/4] Menghubungkan ke PhpMyAdmin Remote (${this.pmaConfig.url})...`);
      const pma = new PmaClient(this.pmaConfig);
      await pma.authenticate();

      // Step 3: Fetch incremental rows from PhpMyAdmin
      this.log('info', `[3/4] Mengambil data baru dari PMA dengan kueri incremental (${this.pmaConfig.primaryKey} > ${lastId !== null ? lastId : 0})...`);
      const newRows = await pma.fetchIncrementalData(lastId, fetchLimit);

      if (!newRows || newRows.length === 0) {
        this.log('success', 'Database lokal sudah dalam kondisi paling up-to-date! Tidak ada baris baru ditarik.');
        this.isSyncing = false;
        return {
          success: true,
          count: 0,
          durationMs: Math.round(performance.now() - startTime),
        };
      }

      this.log('success', `Berhasil mengekstrak ${newRows.length} baris data dari PhpMyAdmin.`);

      // Step 4: Native SQL Upsert to local MySQL via Rust Backend
      this.log('info', `[4/4] Mengirim ${newRows.length} baris data ke backend Rust untuk bulk upsert ke MySQL lokal...`);
      
      const syncResult = await safeInvoke('sync_to_local_db', {
        config: {
          host: this.localDbConfig.host || '127.0.0.1',
          port: parseInt(this.localDbConfig.port || 3306, 10),
          username: this.localDbConfig.username || 'root',
          password: this.localDbConfig.password || '',
          database: this.localDbConfig.database || '',
        },
        tableName: this.localDbConfig.table || this.pmaConfig.table,
        primaryKey: this.pmaConfig.primaryKey || 'id',
        rows: newRows,
      });

      const elapsed = Math.round(performance.now() - startTime);
      this.log('success', `Sinkronisasi Selesai! ${syncResult.message} (Waktu: ${elapsed}ms)`);

      this.isSyncing = false;
      return {
        success: true,
        count: newRows.length,
        durationMs: elapsed,
        fetchedRows: newRows,
      };

    } catch (err) {
      this.isSyncing = false;
      const errMsg = err.message || String(err);
      this.log('error', `Gagal melakukan sinkronisasi: ${errMsg}`);
      return { success: false, error: errMsg };
    }
  }
}
