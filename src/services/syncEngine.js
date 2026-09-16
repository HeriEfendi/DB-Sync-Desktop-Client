import { isTauriEnvironment, safeInvoke, safeListen } from './tauriHelper.js';
import { getTableState, saveTableState } from './syncStateStore.js';

function formatMySQLDateTime(date = new Date()) {
  const d = new Date(date);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export class SyncEngine {
  constructor(pmaConfig, localDbConfig, options = {}) {
    this.pmaConfig = pmaConfig;
    this.localDbConfig = localDbConfig;
    this.serverHost = pmaConfig.url || '';
    this.database = pmaConfig.database || '';
    this.onLog = options.onLog || (() => {});
    this.onProgress = options.onProgress || (() => {});
    this.onTableSynced = options.onTableSynced || (() => {});
    this.isSyncing = false;
    this.shouldStop = false;
    this.unlisteners = [];
  }

  cleanupListeners() {
    if (Array.isArray(this.unlisteners)) {
      this.unlisteners.forEach((fn) => {
        if (typeof fn === 'function') {
          try {
            fn();
          } catch (_) {}
        }
      });
      this.unlisteners = [];
    }
  }

  log(type, message) {
    this.onLog({
      type, // 'info' | 'success' | 'warning' | 'error'
      message,
      timestamp: new Date().toLocaleTimeString(),
    });
  }

  normalizeLocalDbConfig() {
    return {
      host: this.localDbConfig?.host || '127.0.0.1',
      port: parseInt(this.localDbConfig?.port || 3306, 10),
      username: this.localDbConfig?.username || 'root',
      password: this.localDbConfig?.password || '',
      database: this.localDbConfig?.database || '',
      use_docker: Boolean(this.localDbConfig?.use_docker),
      docker_container: this.localDbConfig?.docker_container || '',
    };
  }

  async stopSync() {
    if (this.isSyncing) {
      this.shouldStop = true;
      this.log('warning', '🛑 Sinyal pembatalan diterima. Menghentikan sinkronisasi...');
      if (isTauriEnvironment()) {
        try {
          await safeInvoke('cancel_pma_export');
        } catch (err) {
          console.warn('Gagal memanggil cancel_pma_export:', err);
        }
      }
    }
  }

  /**
   * Execute multi-table synchronization cycle via Direct GZIP Stream (export.php)
   * @param {Object} syncOptions
   * @param {Array<string|Object>} syncOptions.tables - List of table names or objects
   * @param {'incremental'|'fresh'} syncOptions.syncMode - Sync mode ('incremental' or 'fresh')
   * @param {number} syncOptions.rowLimit - Maximum total rows to fetch per table (0 = unlimited)
   * @param {number} syncOptions.batchSize - Batch size per chunk
   */
  async runMultiTableSync(syncOptions = {}) {
    if (this.isSyncing) {
      this.log('warning', 'Proses sinkronisasi sedang berjalan...');
      return { success: false, reason: 'already_running' };
    }

    this.isSyncing = true;
    this.shouldStop = false;
    const startTime = performance.now();

    const rawTables = syncOptions.tables && syncOptions.tables.length > 0
      ? syncOptions.tables
      : [this.pmaConfig.table || 'users'];

    const tables = rawTables.map((t) => typeof t === 'string' ? { name: t, primaryKey: this.pmaConfig.primaryKey || null } : t);
    const syncMode = syncOptions.syncMode || 'incremental';
    const totalRowLimit = parseInt(syncOptions.rowLimit || 0, 10);
    const totalTablesCount = tables.length;

    // Bersihkan listener event lama agar tidak terjadi memory leak / duplikasi event
    this.cleanupListeners();

    // Notify initial progress
    this.onProgress({
      currentTableIndex: 0,
      totalTables: totalTablesCount,
      currentTableName: '',
      rowsSyncedForCurrentTable: 0,
      totalSyncedAllTables: 0,
      status: 'starting',
    });

    if (!isTauriEnvironment()) {
      this.isSyncing = false;
      const errMsg = 'Sinkronisasi ke MySQL lokal port 3306 memerlukan runtime desktop Tauri.';
      this.log('error', errMsg);
      return { success: false, error: errMsg };
    }

    let latestTotalSyncedRows = 0;
    const unlistenLog = await safeListen('pma-log', (event) => {
      const payload = event.payload;
      if (payload && payload.message) {
        const logType = payload.type === 'warn' ? 'warning' : (payload.type || 'info');
        this.log(logType, payload.message);
      }
    });

    const unlistenProgress = await safeListen('pma-progress', (event) => {
      if (event.payload) {
        const rowsCount = event.payload.rows_synced_current_table || 0;
        if (event.payload.total_synced_all_tables) {
          latestTotalSyncedRows = event.payload.total_synced_all_tables;
        }
        this.onProgress({
          currentTableIndex: event.payload.current_table_index,
          totalTables: totalTablesCount, // Selalu kunci ke jumlah tabel yang aktif disinkronkan
          currentTableName: event.payload.current_table_name,
          rowsSyncedCurrentTable: rowsCount,
          rowsSyncedForCurrentTable: rowsCount,
          totalSyncedAllTables: event.payload.total_synced_all_tables,
          status: event.payload.status,
        });
      }
    });

    this.unlisteners.push(unlistenLog, unlistenProgress);

    try {
      const tableNames = tables.map((t) => (typeof t === 'string' ? t : t.name));
      const tablePrimaryKeys = {};
      tables.forEach((t) => {
        if (typeof t === 'object' && t.name && t.primaryKey) {
          tablePrimaryKeys[t.name] = t.primaryKey;
        }
      });

      const localConfig = this.normalizeLocalDbConfig();
      const incrementalWatermarks = {};

      if (syncMode === 'incremental') {
        for (const tableName of tableNames) {
          if (this.shouldStop) {
            this.isSyncing = false;
            if (typeof unlistenLog === 'function') unlistenLog();
            if (typeof unlistenProgress === 'function') unlistenProgress();
            this.log('warning', '🛑 Sinkronisasi telah dihentikan oleh pengguna.');
            return { success: false, cancelled: true, error: 'Dibatalkan oleh pengguna' };
          }
          const state = getTableState(this.serverHost, this.database, tableName);
          const primaryKey = tablePrimaryKeys[tableName] || this.pmaConfig.primaryKey?.trim() || 'id';
          if (!state || state.lastSyncedId === null || state.lastSyncedId === undefined || state.lastSyncedId === 0 || state.lastSyncedId === '0' || !state.lastSyncTime) continue;

          const removedRows = await safeInvoke('delete_local_rows_after_id', {
            config: localConfig,
            tableName,
            primaryKey,
            lastSyncedId: state.lastSyncedId,
          });

          incrementalWatermarks[tableName] = {
            last_synced_id: state.lastSyncedId,
            last_sync_time: state.lastSyncTime,
          };
          if (removedRows > 0) {
            this.log('warning', `[Tabel '${tableName}'] ${removedRows} data lokal di atas Last ID ${state.lastSyncedId} dihapus untuk sinkron dengan server.`);
          }
        }
      }

      if (this.shouldStop) {
        this.isSyncing = false;
        if (typeof unlistenLog === 'function') unlistenLog();
        if (typeof unlistenProgress === 'function') unlistenProgress();
        this.log('warning', '🛑 Sinkronisasi telah dihentikan oleh pengguna.');
        return { success: false, cancelled: true, error: 'Dibatalkan oleh pengguna' };
      }

      await safeInvoke('export_pma_database', {
        pmaConfig: {
          url: this.pmaConfig.url,
          username: this.pmaConfig.username,
          password: this.pmaConfig.password,
          database: this.pmaConfig.database,
          tables: tableNames,
          sync_mode: syncMode,
          row_limit: totalRowLimit,
          throttle_ms: 400,
          table_primary_keys: Object.keys(tablePrimaryKeys).length > 0 ? tablePrimaryKeys : null,
          primary_key: this.pmaConfig.primaryKey?.trim() || 'id',
          incremental_watermarks: Object.keys(incrementalWatermarks).length ? incrementalWatermarks : null,
        },
        localConfig,
      });

      if (syncMode !== 'structure_only') {
        const lastSyncTime = formatMySQLDateTime();

        let batchResults = null;
        try {
          batchResults = await safeInvoke('get_all_tables_last_local_ids', {
            config: localConfig,
            tables: tableNames,
          });
        } catch (batchErr) {
          console.warn('[Sync state] Batch last ID query gagal, fallback ke sequential:', batchErr);
        }

        for (const tableName of tableNames) {
          const tableInfo = batchResults ? batchResults[tableName] : null;
          const detectedPk = tableInfo?.primary_key || tablePrimaryKeys[tableName] || this.pmaConfig.primaryKey?.trim() || 'id';
          const existingState = getTableState(this.serverHost, this.database, tableName);
          let lastSyncedId = tableInfo?.last_id ?? null;

          if (!tableInfo) {
            try {
              lastSyncedId = await safeInvoke('get_last_local_id', {
                config: localConfig,
                tableName,
                primaryKey: detectedPk,
              });
            } catch (error) {
              console.warn(`[Sync state] Gagal membaca MAX(${detectedPk}) untuk '${tableName}':`, error);
            }
          }

          const isValidNewId = lastSyncedId !== null && lastSyncedId !== undefined && lastSyncedId !== 0 && lastSyncedId !== '0';
          const finalLastSyncedId = isValidNewId ? lastSyncedId : (existingState?.lastSyncedId ?? null);

          saveTableState(this.serverHost, this.database, tableName, {
            lastSyncedId: finalLastSyncedId,
            lastSyncTime,
            rowsSynced: 0,
            primaryKey: detectedPk,
          });
          this.onTableSynced(tableName);
        }
      }

      const elapsed = Math.round(performance.now() - startTime);
      if (syncMode === 'structure_only') {
        this.log('success', `🎉 Struktur ${tableNames.length} tabel selesai dibuat di MySQL lokal! (Waktu: ${elapsed}ms)`);
      } else {
        this.log('success', `🎉 Sinkronisasi Selesai! (Waktu: ${elapsed}ms)`);
      }

      this.isSyncing = false;
      this.cleanupListeners();

      return {
        success: true,
        count: latestTotalSyncedRows,
        totalRowsSynced: latestTotalSyncedRows,
        totalTables: totalTablesCount,
        durationMs: elapsed,
      };
    } catch (err) {
      this.isSyncing = false;
      this.cleanupListeners();
      const errMsg = err?.message || String(err);
      if (this.shouldStop) {
        this.log('warning', '🛑 Sinkronisasi telah dihentikan oleh pengguna.');
        return { success: false, cancelled: true, error: 'Dibatalkan oleh pengguna' };
      }
      this.log('error', `Gagal sinkronisasi Direct GZIP Stream: ${errMsg}`);
      return { success: false, error: errMsg };
    }
  }
}
