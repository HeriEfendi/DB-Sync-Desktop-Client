import { isTauriEnvironment, safeInvoke, safeListen } from './tauriHelper.js';
import { PmaClient } from './pmaClient.js';
import { getTableState, saveTableState, clearTableState } from './syncStateStore.js';

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
  }

  log(type, message) {
    this.onLog({
      type, // 'info' | 'success' | 'warning' | 'error'
      message,
      timestamp: new Date().toLocaleTimeString(),
    });
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
   * Helper to detect the primary key field from a row object
   */
  detectPrimaryKey(rowObj, preferredPk) {
    if (!preferredPk) {
      return null;
    }

    if (!rowObj || typeof rowObj !== 'object') return preferredPk || null;

    // 1. Direct match or case-insensitive match for preferredPk
    if (rowObj[preferredPk] !== undefined && rowObj[preferredPk] !== null) {
      return preferredPk;
    }
    const lowerPref = preferredPk.toLowerCase();
    const keys = Object.keys(rowObj);
    const matchedKey = keys.find((k) => k.toLowerCase() === lowerPref);
    if (matchedKey && rowObj[matchedKey] !== undefined && rowObj[matchedKey] !== null) {
      return matchedKey;
    }

    // 2. Common ID column patterns (e.g. 'id', 'product_id', 'id_product', etc.)
    const idKey = keys.find((k) => k.toLowerCase() === 'id' || k.toLowerCase().endsWith('_id') || k.toLowerCase().startsWith('id_'));
    if (idKey && rowObj[idKey] !== undefined && rowObj[idKey] !== null) {
      return idKey;
    }

    // 3. First key with a number or string value
    const firstValidKey = keys.find((k) => rowObj[k] !== undefined && rowObj[k] !== null);
    return firstValidKey || preferredPk || null;
  }

  /**
   * Execute multi-table synchronization cycle with options:
   * @param {Object} syncOptions
   * @param {Array<string|Object>} syncOptions.tables - List of table names or objects
   * @param {'incremental'|'fresh'} syncOptions.syncMode - Sync mode ('incremental' or 'fresh')
   * @param {number} syncOptions.rowLimit - Maximum total rows to fetch per table (0 = unlimited)
   * @param {number} syncOptions.batchSize - Batch size per chunk (default 500)
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
    const batchSize = parseInt(syncOptions.batchSize || 2000, 10);

    // Notify initial progress
    this.onProgress({
      currentTableIndex: 0,
      totalTables: tables.length,
      currentTableName: '',
      rowsSyncedForCurrentTable: 0,
      totalSyncedAllTables: 0,
      status: 'starting',
    });

    if (isTauriEnvironment()) {
      let latestTotalSyncedRows = 0;
      const unlistenLog = await safeListen('pma-log', (event) => {
        const payload = event.payload;
        if (payload && payload.message) {
          this.log(payload.type || 'info', payload.message);
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
            totalTables: event.payload.total_tables,
            currentTableName: event.payload.current_table_name,
            rowsSyncedCurrentTable: rowsCount,
            rowsSyncedForCurrentTable: rowsCount,
            totalSyncedAllTables: event.payload.total_synced_all_tables,
            status: event.payload.status,
          });
        }
      });

      try {
        const tableNames = tables.map((t) => (typeof t === 'string' ? t : t.name));
        // Buat map nama_tabel → primary_key (hanya yang punya primary key)
        const tablePrimaryKeys = {};
        tables.forEach((t) => {
          if (typeof t === 'object' && t.name && t.primaryKey) {
            tablePrimaryKeys[t.name] = t.primaryKey;
          }
        });
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
              config: this.localDbConfig,
              tableName,
              primaryKey,
              lastSyncedId: state.lastSyncedId,
            });
            incrementalWatermarks[tableName] = {
              last_synced_id: state.lastSyncedId,
              last_sync_time: state.lastSyncTime,
            };
            if (removedRows > 0) this.log('warning', `[Tabel '${tableName}'] ${removedRows} data lokal di atas Last ID ${state.lastSyncedId} dihapus untuk sinkron dengan server.`);
          }
        }
        if (this.shouldStop) {
          this.isSyncing = false;
          if (typeof unlistenLog === 'function') unlistenLog();
          if (typeof unlistenProgress === 'function') unlistenProgress();
          this.log('warning', '🛑 Sinkronisasi telah dihentikan oleh pengguna.');
          return { success: false, cancelled: true, error: 'Dibatalkan oleh pengguna' };
        }
        const exportResult = await safeInvoke('export_pma_database', {
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
          localConfig: {
            host: this.localDbConfig.host || '127.0.0.1',
            port: parseInt(this.localDbConfig.port || 3306, 10),
            username: this.localDbConfig.username || 'root',
            password: this.localDbConfig.password || '',
            database: this.localDbConfig.database || '',
          },
        });

        const lastSyncTime = formatMySQLDateTime();
        await Promise.all(tableNames.map(async (tableName) => {
          const primaryKey = tablePrimaryKeys[tableName] || this.pmaConfig.primaryKey?.trim() || 'id';
          const existingState = getTableState(this.serverHost, this.database, tableName);
          let lastSyncedId = null;
          try {
            lastSyncedId = await safeInvoke('get_last_local_id', {
              config: {
                host: this.localDbConfig.host || '127.0.0.1',
                port: parseInt(this.localDbConfig.port || 3306, 10),
                username: this.localDbConfig.username || 'root',
                password: this.localDbConfig.password || '',
                database: this.localDbConfig.database || '',
              },
              tableName,
              primaryKey,
            });
          } catch (error) {
            console.warn(`[Sync state] Gagal membaca MAX(${primaryKey}) untuk '${tableName}':`, error);
          }

          const isValidNewId = lastSyncedId !== null && lastSyncedId !== undefined && lastSyncedId !== 0 && lastSyncedId !== '0';
          const finalLastSyncedId = isValidNewId ? lastSyncedId : (existingState?.lastSyncedId ?? null);

          saveTableState(this.serverHost, this.database, tableName, {
            lastSyncedId: finalLastSyncedId,
            lastSyncTime,
            rowsSynced: 0,
            primaryKey,
          });
          this.onTableSynced(tableName);
        }));

        const elapsed = Math.round(performance.now() - startTime);
        this.log('success', `🎉 Sinkronisasi Selesai! (Waktu: ${elapsed}ms)`);

        this.isSyncing = false;
        if (typeof unlistenLog === 'function') unlistenLog();
        if (typeof unlistenProgress === 'function') unlistenProgress();

        return {
          success: true,
          count: latestTotalSyncedRows,
          totalRowsSynced: latestTotalSyncedRows,
          totalTables: tables.length,
          durationMs: elapsed,
        };
      } catch (err) {
        this.isSyncing = false;
        if (typeof unlistenLog === 'function') unlistenLog();
        if (typeof unlistenProgress === 'function') unlistenProgress();
        const errMsg = err.message || String(err);
        if (this.shouldStop || errMsg.toLowerCase().includes('dibatalkan')) {
          this.log('warning', '🛑 Sinkronisasi telah dihentikan oleh pengguna.');
          return { success: false, cancelled: true, error: 'Dibatalkan oleh pengguna' };
        }
        this.log('error', `Gagal sinkronisasi Direct GZIP Stream: ${errMsg}`);
        return { success: false, error: errMsg };
      }
    }

    try {
      let totalSyncedAllTables = 0;

      const withRetry = async (operationFn, label, maxRetries = 3, delayMs = 800) => {
        let attempt = 0;
        while (attempt < maxRetries) {
          attempt++;
          try {
            return await operationFn();
          } catch (err) {
            const errMsg = err.message || String(err);
            const isConnReset = errMsg.includes('104') ||
                                errMsg.toLowerCase().includes('reset') ||
                                errMsg.toLowerCase().includes('connection') ||
                                errMsg.toLowerCase().includes('timeout');

            if (isConnReset && attempt < maxRetries) {
              this.log('warning', `⚠️ [Retry ${attempt}/${maxRetries}] ${label} mengalami kendala koneksi sementara: (${errMsg}). Mencoba kembali dalam ${delayMs}ms...`);
              await new Promise((resolve) => setTimeout(resolve, delayMs));
              delayMs *= 1.5;
            } else {
              throw err;
            }
          }
        }
      };

      this.log('info', `[1/3] Inisialisasi koneksi ke PhpMyAdmin Remote (${this.pmaConfig.url})...`);

      const basePma = new PmaClient(this.pmaConfig);
      await basePma.authenticate();

      this.log('info', `[2/3] Menyiapkan sinkronisasi untuk ${tables.length} tabel (Mode: ${syncMode.toUpperCase()}, Limit: ${totalRowLimit > 0 ? totalRowLimit + ' baris/tabel' : 'Semua Baris'}, Chunk: ${batchSize} baris/batch).`);

      const dbConfigObj = {
        host: this.localDbConfig.host || '127.0.0.1',
        port: parseInt(this.localDbConfig.port || 3306, 10),
        username: this.localDbConfig.username || 'root',
        password: this.localDbConfig.password || '',
        database: this.localDbConfig.database || '',
      };

      const maxConcurrency = Math.max(1, Math.min(4, parseInt(syncOptions.concurrency || 2, 10)));
      const tableIndex = { value: 0 };
      const nextTable = () => {
        if (tableIndex.value >= tables.length) return null;
        const idx = tableIndex.value++;
        return { idx, tableItem: tables[idx] };
      };

      const processTable = async (tableItem, idx) => {
        const tableName = tableItem.name;
        const pma = new PmaClient({ ...this.pmaConfig, table: tableName, primaryKey: tableItem.primaryKey || this.pmaConfig.primaryKey || null });
        pma.cookieHeader = basePma.cookieHeader;
        pma.token = basePma.token;
        pma.activeBaseUrl = basePma.activeBaseUrl;

        let pk = tableItem.primaryKey || this.pmaConfig.primaryKey || null;
        try {
          const resolvedPk = await pma.resolvePrimaryKey(tableName);
          pk = resolvedPk || null;
        } catch (e) {
          pk = null;
        }

        this.onProgress({
          currentTableIndex: idx + 1,
          totalTables: tables.length,
          currentTableName: tableName,
          rowsSyncedForCurrentTable: 0,
          totalSyncedAllTables,
          status: 'syncing',
        });

        if (syncMode === 'fresh') {
          try {
            await withRetry(
              () => safeInvoke('truncate_local_table', { config: dbConfigObj, tableName }),
              `Mengosongkan tabel '${tableName}'`
            );
            clearTableState(this.serverHost, this.database, tableName);
            this.log('success', `[Tabel '${tableName}'] Tabel lokal berhasil dikosongkan (TRUNCATE) & state direset.`);
          } catch (tErr) {
            this.log('error', `[Tabel '${tableName}'] Gagal mengosongkan tabel lokal: ${tErr.message || tErr}`);
            throw tErr;
          }
        }

        let lastId = null;
        if (syncMode === 'incremental' && pk) {
          // Prioritaskan lastSyncedId dari store agar data manual lokal tidak tertimpa
          const storedState = getTableState(this.serverHost, this.database, tableName);
          if (storedState && storedState.lastSyncedId !== null && storedState.lastSyncedId !== undefined) {
            lastId = storedState.lastSyncedId;
          } else {
            // Fallback: ambil MAX(id) dari tabel lokal jika belum pernah sync
            try {
              lastId = await withRetry(
                () => safeInvoke('get_last_local_id', { config: dbConfigObj, tableName, primaryKey: pk }),
                `Mengecek ID lokal '${tableName}'`
              );
            } catch (e) {
              this.log('warning', `[Tabel '${tableName}'] Tidak dapat mengecek ID lokal (${e.message || e}), mulai dari awal.`);
            }
          }
        } else if (syncMode === 'incremental' && !pk) {
          this.log('warning', `[Tabel '${tableName}'] Tidak memiliki primary key. Sinkronisasi incremental akan memakai fallback full scan tanpa filter PK.`);
        }

        pma.table = tableName;
        pma.primaryKey = pk;

        let totalFetchedForTable = 0;
        let hasMoreData = true;
        let fetchedRowsForThisTable = [];

        while (hasMoreData) {
          if (this.shouldStop) {
            this.log('warning', `🛑 Sinkronisasi tabel '${tableName}' dihentikan oleh pengguna.`);
            hasMoreData = false;
            break;
          }

          let currentBatchSize = batchSize;
          if (totalRowLimit > 0) {
            const remainingAllowed = totalRowLimit - totalFetchedForTable;
            if (remainingAllowed <= 0) break;
            if (currentBatchSize > remainingAllowed) {
              currentBatchSize = remainingAllowed;
            }
          }

          const newRows = await withRetry(
            () => pma.fetchIncrementalData(lastId, currentBatchSize),
            `Penarikan data '${tableName}' (PMA)`
          );

          if (!newRows || newRows.length === 0) {
            hasMoreData = false;
            break;
          }

          const syncResult = await withRetry(
            () => safeInvoke('sync_to_local_db', {
              config: dbConfigObj,
              tableName,
              primaryKey: pk,
              rows: newRows,
            }),
            `Penulisan data '${tableName}' ke MySQL Lokal`
          );

          const actualProcessed = syncResult?.rows_processed ?? newRows.length;
          totalFetchedForTable += actualProcessed;
          totalSyncedAllTables += actualProcessed;
          fetchedRowsForThisTable.push(...newRows);

          this.onProgress({
            currentTableIndex: idx + 1,
            totalTables: tables.length,
            currentTableName: tableName,
            rowsSyncedForCurrentTable: totalFetchedForTable,
            totalSyncedAllTables,
            status: 'syncing',
          });

          if (this.shouldStop) {
            this.log('warning', `🛑 Sinkronisasi tabel '${tableName}' dihentikan oleh pengguna.`);
            hasMoreData = false;
            break;
          }

          const lastObj = newRows[newRows.length - 1];
          const detectedPk = this.detectPrimaryKey(lastObj, pk);
          if (detectedPk !== pk) {
            pk = detectedPk;
            pma.primaryKey = pk;
          }

          const previousLastId = lastId;
          const nextLastId = lastObj ? lastObj[pk] : null;

          if (nextLastId !== undefined && nextLastId !== null) {
            if (String(nextLastId) === String(previousLastId)) {
              hasMoreData = false;
              break;
            }
            lastId = nextLastId;
          } else {
            hasMoreData = false;
            break;
          }

          if (newRows.length < currentBatchSize) {
            hasMoreData = false;
          }

          if (totalRowLimit > 0 && totalFetchedForTable >= totalRowLimit) {
            hasMoreData = false;
          }
        }

        // Simpan state sync terakhir untuk tabel ini
        if (lastId !== null && lastId !== undefined) {
          saveTableState(this.serverHost, this.database, tableName, {
            _server: this.serverHost,
            _database: this.database,
            _table: tableName,
            lastSyncedId: lastId,
            lastSyncTime: new Date().toISOString(),
            rowsSynced: totalFetchedForTable,
            primaryKey: pk,
          });
        }

        if (syncMode === 'incremental' && !this.shouldStop) {
          let localMaxUpdatedAt = null;
          try {
            localMaxUpdatedAt = await withRetry(
              () => safeInvoke('get_local_max_updated_at', { config: dbConfigObj, tableName }),
              `Mengecek max updated_at lokal '${tableName}'`
            );
          } catch (e) {
            this.log('warning', `[Tabel '${tableName}'] Tidak dapat mengecek updated_at lokal: ${e.message || e}`);
          }

          if (localMaxUpdatedAt !== null && localMaxUpdatedAt !== undefined && localMaxUpdatedAt !== '') {
            let updateOffset = 0;
            let hasMoreUpdates = true;

            while (hasMoreUpdates && !this.shouldStop) {
              const updatedRows = await withRetry(
                () => pma.fetchUpdatedRows(localMaxUpdatedAt, batchSize, updateOffset),
                `Fetch updated rows '${tableName}'`
              );

              if (!updatedRows || updatedRows.length === 0) {
                hasMoreUpdates = false;
                break;
              }

              const updateResult = await withRetry(
                () => safeInvoke('sync_to_local_db', {
                  config: dbConfigObj,
                  tableName,
                  primaryKey: pk,
                  rows: updatedRows,
                }),
                `Update rows '${tableName}' ke MySQL Lokal`
              );

              const updatedProcessed = updateResult?.rows_processed ?? updatedRows.length;
              totalFetchedForTable += updatedProcessed;
              totalSyncedAllTables += updatedProcessed;
              updateOffset += updatedRows.length;

              this.onProgress({
                currentTableIndex: idx + 1,
                totalTables: tables.length,
                currentTableName: tableName,
                rowsSyncedForCurrentTable: totalFetchedForTable,
                totalSyncedAllTables,
                status: 'syncing',
              });

              if (updatedRows.length < batchSize) hasMoreUpdates = false;
            }

          } 
        }

        this.log('success', `✅ [Tabel '${tableName}'] Selesai! Total ${totalFetchedForTable} baris data berhasil disinkronkan.`);

        // Update state sync terakhir (termasuk Phase 2 updated_at)
        const existingState = getTableState(this.serverHost, this.database, tableName);
        const isValidNewId = lastId !== null && lastId !== undefined && lastId !== 0 && lastId !== '0';
        const finalLastId = isValidNewId ? lastId : (existingState?.lastSyncedId ?? null);

        saveTableState(this.serverHost, this.database, tableName, {
          _server: this.serverHost,
          _database: this.database,
          _table: tableName,
          lastSyncedId: finalLastId,
          lastSyncTime: formatMySQLDateTime(),
          rowsSynced: totalFetchedForTable,
          primaryKey: pk,
        });
        this.onTableSynced(tableName);

        return { tableName, totalFetchedForTable, fetchedRows: fetchedRowsForThisTable };
      };

      const workers = [];
      for (let workerIdx = 0; workerIdx < maxConcurrency; workerIdx++) {
        workers.push((async () => {
          while (!this.shouldStop) {
            const next = nextTable();
            if (!next) break;
            const { idx, tableItem } = next;
            await processTable(tableItem, idx);
          }
        })());
      }

      await Promise.all(workers);

      const elapsed = Math.round(performance.now() - startTime);

      if (this.shouldStop) {
        this.log('warning', `🛑 Sinkronisasi Dihentikan oleh pengguna! Total ${totalSyncedAllTables} baris data yang sudah masuk tetap tersimpan di database lokal. (Waktu: ${elapsed}ms)`);
      } else {
        this.log('success', `🎉 Sinkronisasi Selesai! Total ${totalSyncedAllTables} baris data disinkronkan di ${tables.length} tabel. (Waktu: ${elapsed}ms)`);
      }

      this.onProgress({
        currentTableIndex: tables.length,
        totalTables: tables.length,
        currentTableName: '',
        rowsSyncedForCurrentTable: 0,
        totalSyncedAllTables,
        status: 'finished',
      });

      this.isSyncing = false;
      return {
        success: true,
        count: totalSyncedAllTables,
        durationMs: elapsed,
      };

    } catch (err) {
      this.isSyncing = false;
      const errMsg = err.message || String(err);
      this.log('error', `Gagal melakukan sinkronisasi: ${errMsg}`);
      return { success: false, error: errMsg };
    }
  }

  /**
   * Legacy single-table incremental sync alias
   */
  async runIncrementalSync(fetchLimit = 500) {
    return this.runMultiTableSync({
      tables: [this.pmaConfig.table || 'users'],
      syncMode: 'incremental',
      rowLimit: fetchLimit,
    });
  }
}
