<template>
  <div class="app-layout">
    <!-- Environment Mode Warning Banner if opened in Web Browser -->
    <div v-if="!isTauri" class="browser-banner">
      <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>
      <span>Mode Pratinjau Web Browser terdeteksi. Untuk mengaktifkan koneksi port native MySQL (3306), buka aplikasi desktop Tauri dengan perintah: <code>npm run tauri dev</code></span>
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
          :testing-pma="testingPma"
          :testing-local="testingLocal"
          @test-pma="testPmaConnection"
          @test-local="testLocalConnection"
        />

        <SyncControl
          :is-syncing="isSyncing"
          v-model:auto-sync-interval="autoSyncInterval"
          :stats="stats"
          @start-sync="handleStartSync"
        />
      </div>

      <!-- Bottom Grid: Log Console & Data Inspector -->
      <div class="grid-bottom">
        <LogConsole
          :logs="logs"
          @clear-logs="logs = []"
        />

        <DataPreview
          :rows="previewRows"
          @refresh-local-preview="fetchLocalPreview"
        />
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
import DataPreview from './components/DataPreview.vue';
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

const pmaStatus = ref({ connected: false });
const localStatus = ref({ connected: false });
const testingPma = ref(false);
const testingLocal = ref(false);

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
    if (savedPma) Object.assign(pmaConfig.value, JSON.parse(savedPma));
    if (savedLocal) Object.assign(localConfig.value, JSON.parse(savedLocal));
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
      message: 'Aplikasi berjalan di Web Browser. Untuk sinkronisasi ke port 3306 MySQL lokal, jalankan: npm run tauri dev',
      timestamp: new Date().toLocaleTimeString(),
    });
  }

  addLog({
    type: 'info',
    message: 'DB-Sync Desktop Client siap. Atur konfigurasi remote & lokal lalu tekan "Mulai Sinkronisasi Data".',
    timestamp: new Date().toLocaleTimeString(),
  });
});

watch(
  pmaConfig,
  (val) => {
    localStorage.setItem('db_sync_pma_config', JSON.stringify(val));
  },
  { deep: true }
);

watch(
  localConfig,
  (val) => {
    localStorage.setItem('db_sync_local_config', JSON.stringify(val));
  },
  { deep: true }
);

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
  addLog({
    type: 'info',
    message: `Mengambil data preview dari tabel MySQL lokal: ${localConfig.value.table || pmaConfig.value.table}...`,
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
      tableName: localConfig.value.table || pmaConfig.value.table,
      limit: 20,
    });

    previewRows.value = rows;
    addLog({
      type: 'success',
      message: `Berhasil mengambil ${rows.length} baris preview dari tabel lokal.`,
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

// Execute full sync cycle
const handleStartSync = async () => {
  if (isSyncing.value) return;

  isSyncing.value = true;
  const engine = new SyncEngine(pmaConfig.value, localConfig.value, {
    onLog: addLog,
  });

  const res = await engine.runIncrementalSync(500);

  if (res.success) {
    pmaStatus.value.connected = true;
    localStatus.value.connected = true;

    if (res.count > 0) {
      stats.value.totalSynced += res.count;
      stats.value.lastDuration = res.durationMs;
      stats.value.lastSyncTime = new Date().toLocaleTimeString();

      if (res.fetchedRows) {
        previewRows.value = res.fetchedRows;
      }
    }
  }

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
  grid-template-columns: 1fr 1fr;
  gap: 16px;
  flex: 1;
}

@media (max-width: 1024px) {
  .grid-bottom {
    grid-template-columns: 1fr;
  }
}
</style>
