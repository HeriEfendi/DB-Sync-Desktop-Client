<template>
  <div class="app-layout desktop-shell">
    <div v-if="!isTauri" class="browser-banner">
      <span class="banner-dot"></span>
      <span>Mode browser aktif. Jalankan <code>npm run tauri dev</code> untuk akses MySQL lokal.</span>
    </div>

    <div class="desktop-frame">
      <section class="desktop-content">
        <Navbar :pma-status="pmaStatus" :local-status="localStatus" :is-syncing="isSyncing" :app-version="appVersion" @trigger-sync="handleStartSync" />

        <main class="dashboard-body">
          <div class="workspace-heading">
            <div><span class="eyebrow">WORKSPACE / OVERVIEW</span><h1>Database sync workspace</h1><p>Kelola koneksi, tabel, dan proses sinkronisasi dari satu tempat.</p></div>
            <div class="heading-meta"><span class="live-indicator"></span> Live workspace</div>
          </div>

          <div class="config-stack">
            <ConnectionConfigSection
              v-model:pma-config="pmaConfig"
              v-model:local-config="localConfig"
              :testing-pma="testingPma"
              :testing-local="testingLocal"
              @test-pma="testPmaConnection"
              @test-local="testLocalConnection"
              @preset-changed="handlePresetChanged"
            />

            <TableConfig
              v-model:selected-tables="selectedTables"
              v-model:available-tables="availableTables"
              :fetching-tables="fetchingTables"
              :sync-table-states="tableStates"
              @fetch-tables="fetchTablesFromPma"
            />

            <SyncControl
              :is-syncing="isSyncing"
              v-model:auto-sync-interval="autoSyncInterval"
              :sync-mode="syncMode"
              :row-limit="rowLimit"
              :stats="stats"
              :sync-progress="syncProgress"
              :sync-table-states="tableStates"
              @start-sync="handleStartSync"
              @stop-sync="handleStopSync"
              @update:sync-mode="syncMode = $event"
              @update:row-limit="rowLimit = Number.isFinite(Number($event)) ? Number($event) : 0"
              @reset-table-state="handleResetTableState"
              @reset-all-table-states="handleResetAllTableStates"
            />
          </div>

          <div class="grid-bottom">
            <LogConsole :logs="logs" @clear-logs="logs = []" />
          </div>
        </main>
        <footer class="status-bar"><span><i class="status-dot"></i> Ready</span><span>Local workspace</span><span class="status-spacer"></span><span>UTF-8</span><span>v{{ appVersion }}</span></footer>
      </section>
    </div>
  </div>
</template>

<script setup>
import { ref, watch, onMounted, onUnmounted } from 'vue';
import Navbar from './components/Navbar.vue';
import ConnectionConfigSection from './components/ConnectionConfigSection.vue';
import TableConfig from './components/TableConfig.vue';
import SyncControl from './components/SyncControl.vue';
import LogConsole from './components/LogConsole.vue';
// import DataPreview from './components/DataPreview.vue';
import { PmaClient } from './services/pmaClient.js';
import { SyncEngine } from './services/syncEngine.js';
import { isTauriEnvironment, safeInvoke } from './services/tauriHelper.js';
import { getAllTableStates, clearTableState, clearAllTableStates } from './services/syncStateStore.js';
import packageJson from '../package.json';
import { getVersion } from '@tauri-apps/api/app';

const isTauri = ref(isTauriEnvironment());
const appVersion = ref(packageJson.version);
const activeNav = ref('connections');
const tableStates = ref([]);

const selectNav = (item) => {
  activeNav.value = item;

  if (item === 'connections') {
    document.querySelector('[data-tab="remote"]')?.click();
    document.querySelector('.grid-top')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    return;
  }

  if (item === 'tables') {
    document.querySelector('[data-tab="tables"]')?.click();
    document.querySelector('.grid-top')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    return;
  }

  document.querySelector('.sync-control-card')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
};

// Connection state
const pmaConfig = ref({
  url: 'http://localhost/phpmyadmin',
  username: 'root',
  password: '',
  database: 'sample_db',
  table: 'users',
  primaryKey: 'id',
});

const localConfig = ref({
  host: '127.0.0.1',
  port: 3306,
  username: 'root',
  password: '',
  database: 'sample_db',
  table: 'users',
});

// Multi-table & Sync options state
const availableTables = ref([]);
const selectedTables = ref([]);
const syncMode = ref('incremental'); // 'incremental' | 'fresh'
const rowLimit = ref(0); // 0 = unlimited

const pmaStatus = ref({ connected: false });
const localStatus = ref({ connected: false });
const testingPma = ref(false);
const testingLocal = ref(false);
const fetchingTables = ref(false);

const isSyncing = ref(false);
const autoSyncInterval = ref(0);
let timerId = null;

const logs = ref([]);

const stats = ref({
  totalSynced: 0,
  lastDuration: 0,
  lastSyncTime: '',
});

let logSaveTimer = null;

const addLog = (entry) => {
  const msg = entry.message || '';
  const tableMatch = msg.match(/\[Tabel\s+'([^']+)'\]/);
  const tableName = tableMatch ? tableMatch[1] : null;

  const isTransientProgress =
    msg.includes('Mengunduh stream') ||
    msg.includes('Menunggu server remote PMA') ||
    msg.includes('Mengalirkan ke MySQL:') ||
    msg.includes('Mengunduh dalam cicilan') ||
    msg.includes('selesai diunduh: ~');

  const isTableFinished =
    msg.includes('Selesai! ~') ||
    msg.includes('Melewati proses import lokal') ||
    msg.includes('Di-skip pada sinkronisasi') ||
    msg.includes('Export GAGAL') ||
    msg.includes('MySQL CLI import error') ||
    msg.includes('Gagal:');

  if (tableName && isTransientProgress) {
    // If there is already an active progress log for this table, update it in place
    const existingIdx = logs.value.findIndex(
      (l) => l.isTransient && l.tableName === tableName
    );
    if (existingIdx !== -1) {
      logs.value[existingIdx] = {
        ...entry,
        isTransient: true,
        tableName,
      };
      return;
    } else {
      logs.value.push({
        ...entry,
        isTransient: true,
        tableName,
      });
      if (logs.value.length > 500) logs.value.shift();
      return;
    }
  }

  if (tableName && isTableFinished) {
    // Clean up temporary in-flight progress logs for this table upon finish
    logs.value = logs.value.filter(
      (l) => !(l.isTransient && l.tableName === tableName)
    );
  }

  // Push permanent clean log
  logs.value.push(entry);
  if (logs.value.length > 500) {
    logs.value.shift();
  }

  if (!logSaveTimer) {
    logSaveTimer = setTimeout(() => {
      try {
        localStorage.setItem(
          'db_sync_logs',
          JSON.stringify(logs.value.filter((l) => !l.isTransient).slice(-100))
        );
      } catch (e) {}
      logSaveTimer = null;
    }, 2500);
  }
};

const refreshTableStates = () => {
  tableStates.value = getAllTableStates(pmaConfig.value.url, pmaConfig.value.database);
};

const handleResetTableState = (st) => {
  clearTableState(st.server, st.database, st.table);
  refreshTableStates();
  addLog({
    type: 'info',
    message: `State sync tabel '${st.table}' di-reset. Sync berikutnya akan mengecek dari awal.`,
    timestamp: new Date().toLocaleTimeString(),
  });
};

const handleResetAllTableStates = () => {
  clearAllTableStates();
  refreshTableStates();
  addLog({
    type: 'warning',
    message: `Semua riwayat sync state tabel berhasil di-reset.`,
    timestamp: new Date().toLocaleTimeString(),
  });
};

onMounted(async () => {
  try {
    const savedPma = localStorage.getItem('db_sync_pma_config');
    const savedLocal = localStorage.getItem('db_sync_local_config');
    const savedTables = localStorage.getItem('db_sync_selected_tables');
    const savedAvailable = localStorage.getItem('db_sync_available_tables');
    const savedMode = localStorage.getItem('db_sync_mode');
    const savedLimit = localStorage.getItem('db_sync_row_limit');
    const savedStats = localStorage.getItem('db_sync_stats');
    const savedLogs = localStorage.getItem('db_sync_logs');

    if (savedPma) Object.assign(pmaConfig.value, JSON.parse(savedPma));
    if (savedLocal) Object.assign(localConfig.value, JSON.parse(savedLocal));
    if (savedAvailable) availableTables.value = JSON.parse(savedAvailable);
    if (savedTables) selectedTables.value = JSON.parse(savedTables);
    if (savedMode) syncMode.value = savedMode;
    if (savedLimit !== null && savedLimit !== undefined) rowLimit.value = parseInt(savedLimit, 10);
    if (savedStats) Object.assign(stats.value, JSON.parse(savedStats));
    if (savedLogs) {
      try {
        const parsed = JSON.parse(savedLogs);
        if (Array.isArray(parsed) && parsed.length > 0) logs.value = parsed;
      } catch (e) {}
    }
  } catch (e) {
    console.error('Failed reading saved config:', e);
  }

  refreshTableStates();

  if (isTauri.value) {
    try {
      const tauriVer = await getVersion();
      if (tauriVer) appVersion.value = tauriVer;
    } catch (e) {}
    addLog({
      type: 'success',
      message: 'Runtime Desktop Tauri terdeteksi dan aktif.',
      timestamp: new Date().toLocaleTimeString(),
    });
  } else {
    addLog({
      type: 'warning',
      message: 'Aplikasi berjalan di Web Browser. Untuk sinkronisasi ke port MySQL lokal, jalankan: npm run tauri dev',
      timestamp: new Date().toLocaleTimeString(),
    });
  }

  addLog({
    type: 'info',
    message: 'DB-Sync Desktop Client siap. Pilih tabel dan mode sinkronisasi, lalu tekan "Mulai Sinkronisasi Data".',
    timestamp: new Date().toLocaleTimeString(),
  });
});

const saveTimerMap = {};
const debounceStorageSave = (key, value) => {
  if (saveTimerMap[key]) clearTimeout(saveTimerMap[key]);
  saveTimerMap[key] = setTimeout(() => {
    try {
      localStorage.setItem(key, typeof value === 'string' ? value : JSON.stringify(value));
    } catch (e) {}
    delete saveTimerMap[key];
  }, 300);
};

watch(pmaConfig, (val) => {
  debounceStorageSave('db_sync_pma_config', val);
  refreshTableStates();
}, { deep: true });
watch(localConfig, (val) => debounceStorageSave('db_sync_local_config', val), { deep: true });
watch(availableTables, (val) => debounceStorageSave('db_sync_available_tables', val), { deep: true });
watch(selectedTables, (val) => debounceStorageSave('db_sync_selected_tables', val), { deep: true });
watch(syncMode, (val) => debounceStorageSave('db_sync_mode', val));
watch(rowLimit, (val) => debounceStorageSave('db_sync_row_limit', String(val)));
watch(stats, (val) => debounceStorageSave('db_sync_stats', val), { deep: true });

const handlePresetChanged = () => {
  pmaStatus.value.connected = false;
  localStatus.value.connected = false;
  refreshTableStates();
};

watch(autoSyncInterval, (sec) => {
  if (timerId) clearInterval(timerId);

  if (sec > 0) {
    addLog({
      type: 'warning',
      message: `Sinkronisasi Otomatis Dihidupkan (Setiap ${sec} detik).`,
      timestamp: new Date().toLocaleTimeString(),
    });

    timerId = setInterval(() => {
      if (!isSyncing.value) {
        handleStartSync();
      }
    }, sec * 1000);
  } else {
    addLog({
      type: 'info',
      message: 'Sinkronisasi Otomatis Dimatikan.',
      timestamp: new Date().toLocaleTimeString(),
    });
  }
});

onUnmounted(() => {
  if (timerId) clearInterval(timerId);
});

// Fetch list of tables dynamically from remote PMA
const fetchTablesFromPma = async () => {
  fetchingTables.value = true;
  addLog({
    type: 'info',
    message: `Menarik daftar tabel dari database PMA remote (${pmaConfig.value.database})...`,
    timestamp: new Date().toLocaleTimeString(),
  });

  let pmaLogUnlisten = null;

  try {
    let tables = [];
    if (isTauri.value) {
      // Listen to pma-log events from Rust so diagnostics appear in the app console
      try {
        pmaLogUnlisten = await safeListen('pma-log', (event) => {
          const { type: t, message } = event.payload || {};
          const logType = t === 'warn' ? 'warning' : (t || 'info');
          addLog({ type: logType, message: `[PMA] ${message}`, timestamp: new Date().toLocaleTimeString() });
        });
      } catch (_) {}

      try {
        tables = await safeInvoke('get_pma_tables', {
          pmaConfig: {
            url: pmaConfig.value.url,
            username: pmaConfig.value.username,
            password: pmaConfig.value.password,
            database: pmaConfig.value.database,
            tables: [],
          },
        });
      } catch (e) {
        addLog({
          type: 'warning',
          message: `Engine native Rust gagal ekstraksi tabel (${e.message || e}), mencoba fallback client JS...`,
          timestamp: new Date().toLocaleTimeString(),
        });
      }
    }

    if (!tables || tables.length === 0) {
      addLog({ type: 'info', message: 'Mencoba fallback JS PmaClient...', timestamp: new Date().toLocaleTimeString() });
      try {
        const client = new PmaClient(pmaConfig.value);
        await client.authenticate();
        tables = await client.fetchTablesList();
      } catch (jsErr) {
        addLog({ type: 'warning', message: `JS PmaClient fallback gagal: ${jsErr.message}`, timestamp: new Date().toLocaleTimeString() });
      }
    }

    if (tables && tables.length > 0) {
      availableTables.value = tables;
      // Default: check all tables
      selectedTables.value = [...tables];
      pmaStatus.value.connected = true;

      addLog({
        type: 'success',
        message: `✅ Berhasil mengekstrak ${tables.length} tabel dari PMA: ${tables.slice(0, 10).join(', ')}${tables.length > 10 ? ` ... +${tables.length - 10} lainnya` : ''}`,
        timestamp: new Date().toLocaleTimeString(),
      });
    } else {
      addLog({
        type: 'error',
        message: `Tidak ada tabel yang ditemukan di database '${pmaConfig.value.database}'. Pastikan nama database benar, credentials memiliki akses, dan coba lakukan "Tes Koneksi PMA" terlebih dahulu sebelum fetch tabel.`,
        timestamp: new Date().toLocaleTimeString(),
      });
    }
  } catch (err) {
    addLog({
      type: 'error',
      message: `Gagal menarik daftar tabel: ${err.message || err}`,
      timestamp: new Date().toLocaleTimeString(),
    });
  } finally {
    fetchingTables.value = false;
    if (pmaLogUnlisten) pmaLogUnlisten();
  }
};


// Test PMA Remote connection
const testPmaConnection = async () => {
  testingPma.value = true;
  addLog({
    type: 'info',
    message: `Menguji koneksi HTTP ke PMA remote: ${pmaConfig.value.url}...`,
    timestamp: new Date().toLocaleTimeString(),
  });

  try {
    const client = new PmaClient(pmaConfig.value);
    const res = await client.authenticate();
    pmaStatus.value.connected = true;
    addLog({
      type: 'success',
      message: `PMA Remote OK: ${res.message}`,
      timestamp: new Date().toLocaleTimeString(),
    });
  } catch (err) {
    pmaStatus.value.connected = false;
    addLog({
      type: 'error',
      message: `Tes PMA Remote Gagal: ${err.message}`,
      timestamp: new Date().toLocaleTimeString(),
    });
  } finally {
    testingPma.value = false;
  }
};

// Test Local MySQL connection via Rust IPC
const testLocalConnection = async () => {
  testingLocal.value = true;
  addLog({
    type: 'info',
    message: `Menguji koneksi port native MySQL lokal (${localConfig.value.host}:${localConfig.value.port || 3306})...`,
    timestamp: new Date().toLocaleTimeString(),
  });

  try {
    const res = await safeInvoke('test_local_connection', {
      config: {
        host: localConfig.value.host || '127.0.0.1',
        port: parseInt(localConfig.value.port || 3306, 10),
        username: localConfig.value.username || 'root',
        password: localConfig.value.password || '',
        database: localConfig.value.database || '',
      },
    });

    localStatus.value.connected = true;
    addLog({
      type: 'success',
      message: `MySQL Lokal OK: ${res}`,
      timestamp: new Date().toLocaleTimeString(),
    });
  } catch (err) {
    localStatus.value.connected = false;
    addLog({
      type: 'error',
      message: `Tes MySQL Lokal Gagal: ${err.message || err}`,
      timestamp: new Date().toLocaleTimeString(),
    });
  } finally {
    testingLocal.value = false;
  }
};

// Fetch preview of local MySQL table
const fetchLocalPreview = async () => {
  const targetTable = (selectedTables.value && selectedTables.value.length > 0)
    ? selectedTables.value[0]
    : (localConfig.value.table || pmaConfig.value.table);

  addLog({
    type: 'info',
    message: `Mengambil data preview dari tabel MySQL lokal: ${targetTable}...`,
    timestamp: new Date().toLocaleTimeString(),
  });

  try {
    const rows = await safeInvoke('get_local_table_preview', {
      config: {
        host: localConfig.value.host || '127.0.0.1',
        port: parseInt(localConfig.value.port || 3306, 10),
        username: localConfig.value.username || 'root',
        password: localConfig.value.password || '',
        database: localConfig.value.database || '',
      },
      tableName: targetTable,
      limit: 20,
    });

    previewRows.value = rows;
    addLog({
      type: 'success',
      message: `Berhasil mengambil ${rows.length} baris preview dari tabel lokal '${targetTable}'.`,
      timestamp: new Date().toLocaleTimeString(),
    });
  } catch (err) {
    addLog({
      type: 'error',
      message: `Gagal mengambil data preview lokal: ${err.message || err}`,
      timestamp: new Date().toLocaleTimeString(),
    });
  }
};

// Live sync progress tracking
let activeEngineInstance = null;

const syncProgress = ref({
  currentTableIndex: 0,
  totalTables: 0,
  currentTableName: '',
  rowsSyncedCurrentTable: 0,
  totalSyncedAllTables: 0,
  status: 'idle',
});

// Stop ongoing sync cycle safely
const handleStopSync = () => {
  if (activeEngineInstance) {
    activeEngineInstance.stopSync();
  }
};

// Execute full sync cycle across selected tables
const handleStartSync = async () => {
  if (isSyncing.value) return;

  const tablesToSync = selectedTables.value.length > 0
    ? selectedTables.value
    : [pmaConfig.value.table || 'users'];

  isSyncing.value = true;
  syncProgress.value = {
    currentTableIndex: 0,
    totalTables: tablesToSync.length,
    currentTableName: tablesToSync[0] || '',
    rowsSyncedCurrentTable: 0,
    totalSyncedAllTables: 0,
    status: 'starting',
  };

  const engine = new SyncEngine(pmaConfig.value, localConfig.value, {
    onLog: addLog,
    onTableSynced: refreshTableStates,
    onProgress: (p) => {
      const rowsCount = p.rowsSyncedCurrentTable ?? p.rowsSyncedForCurrentTable ?? 0;
      syncProgress.value = {
        currentTableIndex: p.currentTableIndex || 0,
        totalTables: p.totalTables || 0,
        currentTableName: p.currentTableName || '',
        rowsSyncedCurrentTable: rowsCount,
        rowsSyncedForCurrentTable: rowsCount,
        totalSyncedAllTables: p.totalSyncedAllTables || 0,
        status: p.status || 'idle',
      };
    },
  });
  activeEngineInstance = engine;

  try {
    const res = await engine.runMultiTableSync({
      tables: tablesToSync,
      syncMode: syncMode.value,
      rowLimit: rowLimit.value,
      batchSize: 2000,
    });

    const countSynced = (res && res.totalRowsSynced !== undefined ? res.totalRowsSynced : res?.count) || syncProgress.value.totalSyncedAllTables || 0;
    stats.value.totalSynced = countSynced;
    if (res && res.durationMs) stats.value.lastDuration = res.durationMs;
    stats.value.lastSyncTime = new Date().toLocaleTimeString();

    if (res && res.success) {
      pmaStatus.value.connected = true;
      localStatus.value.connected = true;
      if (res.fetchedRows && res.fetchedRows.length > 0) previewRows.value = res.fetchedRows;
    }
  } catch (err) {
    addLog({
      type: 'error',
      message: `Sinkronisasi gagal: ${err?.message || err}`,
      timestamp: new Date().toLocaleTimeString(),
    });
  } finally {
    activeEngineInstance = null;
    isSyncing.value = false;
    refreshTableStates();
  }
};
</script>

.app-layout { display:flex; flex-direction:column; width:100%; height:100vh; min-width:0; overflow:hidden; }
.desktop-frame { flex:1 1 auto; width:100%; min-width:0; }
.desktop-content { width:100%; min-width:0; }
.browser-banner { flex:0 0 auto; }

