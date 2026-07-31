<template>
  <div class="app-layout">
    <!-- Environment Mode Warning Banner if opened in Web Browser -->
    <div v-if="!isTauri" class="browser-banner">
      <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>
      <span>Mode Pratinjau Web Browser terdeteksi. Untuk mengaktifkan koneksi port native MySQL Local, buka aplikasi desktop Tauri dengan perintah: <code>npm run tauri dev</code></span>
    </div>

    <!-- Navbar Header -->
    <Navbar
      :pma-status="pmaStatus"
      :local-status="localStatus"
      :is-syncing="isSyncing"
      @trigger-sync="handleStartSync"
    />

    <!-- Main Dashboard Content -->
    <main class="dashboard-body">
      <!-- Top Grid: Config & Controls -->
      <div class="grid-top">
        <ConnectionConfig
          v-model:pma-config="pmaConfig"
          v-model:local-config="localConfig"
          v-model:selected-tables="selectedTables"
          v-model:available-tables="availableTables"
          v-model:sync-mode="syncMode"
          v-model:row-limit="rowLimit"
          :testing-pma="testingPma"
          :testing-local="testingLocal"
          :fetching-tables="fetchingTables"
          @test-pma="testPmaConnection"
          @test-local="testLocalConnection"
          @fetch-tables="fetchTablesFromPma"
          @preset-changed="handlePresetChanged"
        />

        <SyncControl
          :is-syncing="isSyncing"
          v-model:auto-sync-interval="autoSyncInterval"
          :stats="stats"
          :sync-progress="syncProgress"
          @start-sync="handleStartSync"
          @stop-sync="handleStopSync"
        />
      </div>

      <!-- Bottom Grid: Log Console & Data Inspector -->
      <div class="grid-bottom">
        <LogConsole
          :logs="logs"
          @clear-logs="logs = []"
        />

        <!-- <DataPreview
          :rows="previewRows"
          @refresh-local-preview="fetchLocalPreview"
        /> -->
      </div>
    </main>
  </div>
</template>

<script setup>
import { ref, watch, onMounted, onUnmounted } from 'vue';
import Navbar from './components/Navbar.vue';
import ConnectionConfig from './components/ConnectionConfig.vue';
import SyncControl from './components/SyncControl.vue';
import LogConsole from './components/LogConsole.vue';
// import DataPreview from './components/DataPreview.vue';
import { PmaClient } from './services/pmaClient.js';
import { SyncEngine } from './services/syncEngine.js';
import { isTauriEnvironment, safeInvoke } from './services/tauriHelper.js';

const isTauri = ref(isTauriEnvironment());

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

// Sync execution state
const isSyncing = ref(false);
const autoSyncInterval = ref(0);
let timerId = null;

const logs = ref([]);
const previewRows = ref([]);

const stats = ref({
  totalSynced: 0,
  lastDuration: 0,
  lastSyncTime: '',
});

const addLog = (entry) => {
  logs.value.push(entry);
};

onMounted(() => {
  try {
    const savedPma = localStorage.getItem('db_sync_pma_config');
    const savedLocal = localStorage.getItem('db_sync_local_config');
    const savedTables = localStorage.getItem('db_sync_selected_tables');
    const savedAvailable = localStorage.getItem('db_sync_available_tables');
    const savedMode = localStorage.getItem('db_sync_mode');
    const savedLimit = localStorage.getItem('db_sync_row_limit');

    if (savedPma) Object.assign(pmaConfig.value, JSON.parse(savedPma));
    if (savedLocal) Object.assign(localConfig.value, JSON.parse(savedLocal));
    if (savedAvailable) availableTables.value = JSON.parse(savedAvailable);
    if (savedTables) selectedTables.value = JSON.parse(savedTables);
    if (savedMode) syncMode.value = savedMode;
    if (savedLimit !== null && savedLimit !== undefined) rowLimit.value = parseInt(savedLimit, 10);
  } catch (e) {
    console.error('Failed reading saved config:', e);
  }

  if (isTauri.value) {
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

watch(pmaConfig, (val) => localStorage.setItem('db_sync_pma_config', JSON.stringify(val)), { deep: true });
watch(localConfig, (val) => localStorage.setItem('db_sync_local_config', JSON.stringify(val)), { deep: true });
watch(availableTables, (val) => localStorage.setItem('db_sync_available_tables', JSON.stringify(val)), { deep: true });
watch(selectedTables, (val) => localStorage.setItem('db_sync_selected_tables', JSON.stringify(val)), { deep: true });
watch(syncMode, (val) => localStorage.setItem('db_sync_mode', val));
watch(rowLimit, (val) => localStorage.setItem('db_sync_row_limit', String(val)));

const handlePresetChanged = () => {
  pmaStatus.value.connected = false;
  localStatus.value.connected = false;
};


// Auto-sync scheduler
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

  try {
    const client = new PmaClient(pmaConfig.value);
    await client.authenticate();
    const tables = await client.fetchTablesList();

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
    onProgress: (p) => {
      syncProgress.value = {
        currentTableIndex: p.currentTableIndex,
        totalTables: p.totalTables,
        currentTableName: p.currentTableName,
        rowsSyncedCurrentTable: p.rowsSyncedForCurrentTable,
        totalSyncedAllTables: p.totalSyncedAllTables,
        status: p.status,
      };
    },
  });
  activeEngineInstance = engine;

  const res = await engine.runMultiTableSync({
    tables: tablesToSync,
    syncMode: syncMode.value,
    rowLimit: rowLimit.value,
    batchSize: 2000,
  });

  const countSynced = (res && res.count) || syncProgress.value.totalSyncedAllTables || 0;
  if (countSynced > 0) {
    stats.value.totalSynced += countSynced;
  }
  if (res && res.durationMs) {
    stats.value.lastDuration = res.durationMs;
  }
  stats.value.lastSyncTime = new Date().toLocaleTimeString();

  if (res && res.success) {
    pmaStatus.value.connected = true;
    localStatus.value.connected = true;

    if (res.fetchedRows && res.fetchedRows.length > 0) {
      previewRows.value = res.fetchedRows;
    }
  }

  activeEngineInstance = null;
  isSyncing.value = false;
};
</script>

<style scoped>
.app-layout {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}

.browser-banner {
  background: rgba(245, 158, 11, 0.18);
  color: var(--accent-amber);
  border-bottom: 1px solid rgba(245, 158, 11, 0.3);
  padding: 8px 16px;
  font-size: 0.8rem;
  display: flex;
  align-items: center;
  gap: 8px;
  justify-content: center;
}

.browser-banner code {
  background: rgba(0, 0, 0, 0.4);
  padding: 2px 6px;
  border-radius: 4px;
  font-family: 'JetBrains Mono', monospace;
  color: white;
}

.dashboard-body {
  flex: 1;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  overflow-y: auto;
}

.grid-top {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.grid-bottom {
  display: grid;
  grid-template-columns: 1fr;
  gap: 16px;
  flex: 1;
}
</style>
