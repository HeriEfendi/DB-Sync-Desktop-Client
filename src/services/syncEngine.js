import { isTauriEnvironment, safeInvoke, safeListen } from './tauriHelper.js';
import { PmaClient } from './pmaClient.js';

export class SyncEngine {
  constructor(pmaConfig, localDbConfig, options = {}) {
    this.pmaConfig = pmaConfig;
    this.localDbConfig = localDbConfig;
    this.onLog = options.onLog || (() => {});
    this.onProgress = options.onProgress || (() => {});
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

  stopSync() {
    if (this.isSyncing) {
      this.shouldStop = true;
      this.log('warning', '🛑 Sinyal pembatalan diterima. Menghentikan sinkronisasi...');
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
      this.log('info', `[Tauri Native Engine] Memulai Direct SQL/GZIP Stream export.php (${tables.length} tabel)...`);

      let latestTotalSyncedRows = 0;

      const unlistenLog = await safeListen('pma-log', (event) => {
        if (event.payload) {
          this.log(event.payload.type, event.payload.message);
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
          },
          localConfig: {
            host: this.localDbConfig.host || '127.0.0.1',
            port: parseInt(this.localDbConfig.port || 3306, 10),
            username: this.localDbConfig.username || 'root',
            password: this.localDbConfig.password || '',
            database: this.localDbConfig.database || '',
          },
        });

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
          this.log('warning', `[Tabel '${tableName}'] Tidak dapat memastikan primary key metadata, memakai fallback tanpa PK.`);
          pk = null;
        }

        this.log('info', `➡️ [Tabel ${idx + 1}/${tables.length}] Memproses tabel '${tableName}'${pk ? ` (PK: ${pk})` : ' (Tanpa PK, mode fallback)'}...`);

        this.onProgress({
          currentTableIndex: idx + 1,
          totalTables: tables.length,
          currentTableName: tableName,
          rowsSyncedForCurrentTable: 0,
          totalSyncedAllTables,
          status: 'syncing',
        });

        if (syncMode === 'fresh') {
          this.log('warning', `[Tabel '${tableName}'] Mengosongkan (TRUNCATE) tabel lokal untuk mode Fresh Sync...`);
          try {
            await withRetry(
              () => safeInvoke('truncate_local_table', { config: dbConfigObj, tableName }),
              `Mengosongkan tabel '${tableName}'`
            );
            this.log('success', `[Tabel '${tableName}'] Tabel lokal berhasil dikosongkan (TRUNCATE).`);
          } catch (tErr) {
            this.log('error', `[Tabel '${tableName}'] Gagal mengosongkan tabel lokal: ${tErr.message || tErr}`);
            throw tErr;
          }
        }

        let lastId = null;
        if (syncMode === 'incremental' && pk) {
          try {
            lastId = await withRetry(
              () => safeInvoke('get_last_local_id', { config: dbConfigObj, tableName, primaryKey: pk }),
              `Mengecek ID lokal '${tableName}'`
            );
            this.log('info', `[Tabel '${tableName}'] State ID lokal saat ini (${pk}): ${lastId !== null && lastId !== undefined ? lastId : 'Kosong'}`);
          } catch (e) {
            this.log('warning', `[Tabel '${tableName}'] Tidak dapat mengecek ID lokal (${e.message || e}), mulai dari awal.`);
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

          this.log('info', `[Tabel '${tableName}'] Mengambil batch data dari PMA (${pk} > ${lastId !== null ? lastId : 0}, limit: ${currentBatchSize})...`);

          const newRows = await withRetry(
            () => pma.fetchIncrementalData(lastId, currentBatchSize),
            `Penarikan data '${tableName}' (PMA)`
          );

          if (!newRows || newRows.length === 0) {
            this.log('info', `[Tabel '${tableName}'] Tidak ada lagi data baru untuk ditarik.`);
            hasMoreData = false;
            break;
          }

          this.log('success', `[Tabel '${tableName}'] Mengekstrak ${newRows.length} baris dari PMA, mengirim ke MySQL lokal...`);

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

          this.log('success', `[Tabel '${tableName}'] Berhasil menyimpan ${actualProcessed} baris ke database lokal.`);

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
            this.log('info', `[Tabel '${tableName}'] Primary key terdeteksi dari data: '${detectedPk}' (sebelumnya '${pk}')`);
            pk = detectedPk;
            pma.primaryKey = pk;
          }

          const previousLastId = lastId;
          const nextLastId = lastObj ? lastObj[pk] : null;

          if (nextLastId !== undefined && nextLastId !== null) {
            if (String(nextLastId) === String(previousLastId)) {
              this.log('warning', `[Tabel '${tableName}'] Primary key '${pk}' nilainya tidak bertambah (${nextLastId}). Menghentikan perulangan untuk mencegah infinite loop.`);
              hasMoreData = false;
              break;
            }
            lastId = nextLastId;
          } else {
            this.log('warning', `[Tabel '${tableName}'] Primary key '${pk}' tidak ditemukan pada baris data. Menghentikan perulangan batch.`);
            hasMoreData = false;
            break;
          }

          if (newRows.length < currentBatchSize) {
            hasMoreData = false;
          }

          if (totalRowLimit > 0 && totalFetchedForTable >= totalRowLimit) {
            this.log('info', `[Tabel '${tableName}'] Batas limit row (${totalRowLimit}) telah tercapai.`);
            hasMoreData = false;
          }
        }

        this.log('success', `✅ [Tabel '${tableName}'] Phase 1 selesai! ${totalFetchedForTable} baris baru berhasil ditarik.`);

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
            this.log('info', `[Tabel '${tableName}'] Phase 2 — Mencari baris yang diperbarui di server sejak: ${localMaxUpdatedAt}...`);
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

              this.log('info', `[Tabel '${tableName}'] Phase 2 — Mengekstrak ${updatedRows.length} baris yang diperbarui, mengirim ke MySQL lokal...`);

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

              this.log('success', `[Tabel '${tableName}'] Phase 2 — ${updatedProcessed} baris berhasil di-update.`);

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

            if (!this.shouldStop) {
              this.log('success', `[Tabel '${tableName}'] Phase 2 selesai.`);
            }
          } else {
            this.log('info', `[Tabel '${tableName}'] Tabel tidak memiliki kolom updated_at, Phase 2 dilewati.`);
          }
        }

        this.log('success', `✅ [Tabel '${tableName}'] Selesai! Total ${totalFetchedForTable} baris data berhasil disinkronkan.`);
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
