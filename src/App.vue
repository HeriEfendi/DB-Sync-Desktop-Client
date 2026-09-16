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
              v-model:selected-preset="currentPresetName"
              :testing-pma="testingPma"
              :testing-local="testingLocal"
              @test-pma="testPmaConnection"
              @test-local="testLocalConnection"
              @preset-changed="handlePresetChanged"
            />

            <TableConfig
              :server-key="currentServerKey"
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
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import Navbar from './components/Navbar.vue';
import ConnectionConfigSection from './components/ConnectionConfigSection.vue';
import TableConfig from './components/TableConfig.vue';
import SyncControl from './components/SyncControl.vue';
import LogConsole from './components/LogConsole.vue';
import { PmaClient } from './services/pmaClient.js';
import { SyncEngine } from './services/syncEngine.js';
import { isTauriEnvironment, safeInvoke } from './services/tauriHelper.js';
import { getAllTableStates, clearTableState, clearAllTableStates } from './services/syncStateStore.js';
import packageJson from '../package.json';
import { getVersion } from '@tauri-apps/api/app';

const isTauri = ref(isTauriEnvironment());
const appVersion = ref(packageJson.version);
const tableStates = ref([]);

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
  use_docker: false,
  docker_container: '',
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
  const tableName = tableMatch ? tableMatch[1] : (entry.tableName || null);

  const isTransientProgress =
    msg.includes('Mengunduh stream') ||
    msg.includes('Menunggu server remote PMA') ||
    msg.includes('Mengalirkan ke MySQL:') ||
    msg.includes('Mengunduh dalam cicilan') ||
    msg.includes('selesai diunduh: ~');

  const isTableFinished =
    entry.type === 'success' ||
    msg.includes('Selesai!') ||
    msg.includes('Struktur tabel berhasil dibuat') ||
    msg.includes('Auto-fallback Fresh Sync') ||
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
    // Hapus log sementara (transient) dan semua log INFO perantara untuk tabel ini
    // agar histori log konsol tetap bersih dan ringkas (hanya menyisakan status akhir)
    logs.value = logs.value.filter(
      (l) =>
        !(
          (l.tableName === tableName ||
            (l.message && l.message.includes(`[Tabel '${tableName}']`))) &&
          (l.isTransient || l.type === 'info')
        )
    );
  }

  // Push permanent clean log
  logs.value.push({
    ...entry,
    tableName: tableName || undefined,
  });
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

const currentPresetName = ref(localStorage.getItem('db_sync_last_preset') || '');

const computeServerKey = (pma, preset = '') => {
  const p = (preset || '').trim();
  if (p) {
    return `preset_${p.replace(/[^a-zA-Z0-9_\-]/g, '_')}`;
  }
  const u = (pma?.url || '').trim().toLowerCase().replace(/https?:\/\//, '').replace(/[^a-zA-Z0-9_\-]/g, '_');
  const d = (pma?.database || '').trim().toLowerCase().replace(/[^a-zA-Z0-9_\-]/g, '_');
  if (!u && !d) return 'default';
  return `server_${u}___${d}`;
};

const currentServerKey = computed(() => computeServerKey(pmaConfig.value, currentPresetName.value));

const saveTablesForServerKey = (key, available, selected) => {
  if (!key) return;
  try {
    const data = {
      availableTables: Array.isArray(available) ? available : [],
      selectedTables: Array.isArray(selected) ? selected : [],
    };
    localStorage.setItem(`db_sync_tables_${key}`, JSON.stringify(data));
  } catch (e) {
    console.error('Failed saving tables for server key:', key, e);
  }
};

const loadTablesForServerKey = (key) => {
  if (!key) return;
  try {
    const raw = localStorage.getItem(`db_sync_tables_${key}`);
    if (raw) {
      const data = JSON.parse(raw);
      availableTables.value = Array.isArray(data.availableTables) ? data.availableTables : [];
      selectedTables.value = Array.isArray(data.selectedTables) ? data.selectedTables : [];
      return;
    }

    // Fallback migrasi jika belum ada penyimpanan per serverKey tapi ada data lama di global
    const globalAvail = localStorage.getItem('db_sync_available_tables');
    const globalSel = localStorage.getItem('db_sync_selected_tables');
    if (globalAvail) {
      try {
        availableTables.value = JSON.parse(globalAvail);
        selectedTables.value = globalSel ? JSON.parse(globalSel) : [];
        saveTablesForServerKey(key, availableTables.value, selectedTables.value);
        return;
      } catch (e) {}
    }

    // Server baru: kosongkan daftar tabel
    availableTables.value = [];
    selectedTables.value = [];
  } catch (e) {
    console.error('Failed loading tables for server key:', key, e);
    availableTables.value = [];
    selectedTables.value = [];
  }
};

onMounted(async () => {
  try {
    const savedPma = localStorage.getItem('db_sync_pma_config');
    const savedLocal = localStorage.getItem('db_sync_local_config');
    const savedMode = localStorage.getItem('db_sync_mode');
    const savedLimit = localStorage.getItem('db_sync_row_limit');
    const savedStats = localStorage.getItem('db_sync_stats');
    const savedLogs = localStorage.getItem('db_sync_logs');

    if (savedPma) Object.assign(pmaConfig.value, JSON.parse(savedPma));
    if (savedLocal) Object.assign(localConfig.value, JSON.parse(savedLocal));
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

  loadTablesForServerKey(currentServerKey.value);
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

watch(currentServerKey, (newKey, oldKey) => {
  if (oldKey && oldKey !== newKey) {
    saveTablesForServerKey(oldKey, availableTables.value, selectedTables.value);
  }
  if (newKey) {
    loadTablesForServerKey(newKey);
  }
  refreshTableStates();
});

watch(pmaConfig, (val) => {
  debounceStorageSave('db_sync_pma_config', val);
  refreshTableStates();
}, { deep: true });
watch(localConfig, (val) => debounceStorageSave('db_sync_local_config', val), { deep: true });
watch(availableTables, (val) => {
  saveTablesForServerKey(currentServerKey.value, val, selectedTables.value);
}, { deep: true });
watch(selectedTables, (val) => {
  saveTablesForServerKey(currentServerKey.value, availableTables.value, val);
}, { deep: true });
watch(syncMode, (val) => debounceStorageSave('db_sync_mode', val));
watch(rowLimit, (val) => debounceStorageSave('db_sync_row_limit', String(val)));
watch(stats, (val) => debounceStorageSave('db_sync_stats', val), { deep: true });

const handlePresetChanged = (presetName) => {
  pmaStatus.value.connected = false;
  localStatus.value.connected = false;
  if (presetName !== undefined && presetName !== null) {
    currentPresetName.value = presetName;
  }
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
      pmaStatus.value.connected = true;

      addLog({
        type: 'success',
        message: `✅ Berhasil mengekstrak ${tables.length} tabel dari PMA: ${tables.slice(0, 10).join(', ')}${tables.length > 10 ? ` ... +${tables.length - 10} lainnya` : ''}`,
        timestamp: new Date().toLocaleTimeString(),
      });

      // Bandingkan dengan tabel di database MySQL lokal
      if (isTauri.value && localConfig.value.database) {
        try {
          addLog({
            type: 'info',
            message: `Memeriksa perbedaan skema tabel dengan database MySQL lokal (${localConfig.value.database})...`,
            timestamp: new Date().toLocaleTimeString(),
          });

          const localTables = await safeInvoke('get_local_tables', {
            config: {
              host: localConfig.value.host || '127.0.0.1',
              port: parseInt(localConfig.value.port || 3306, 10),
              username: localConfig.value.username || 'root',
              password: localConfig.value.password || '',
              database: localConfig.value.database || '',
              use_docker: Boolean(localConfig.value.use_docker),
              docker_container: localConfig.value.docker_container || '',
            },
          });

          if (Array.isArray(localTables)) {
            const pmaTableSet = new Set(tables);
            const localTableSet = new Set(localTables);

            // Daftar tabel yang sebelumnya pernah terdaftar pada server PMA ini
            const prevServerTables = new Set(availableTables.value || []);

            // Drop: HANYA tabel yang sebelumnya memang pernah terdaftar di server PMA ini,
            // dan ada di MySQL lokal, namun sekarang sudah tidak ada lagi di PMA server ini.
            // Ini mencegah terhapusnya tabel lokal yang berasal dari server PMA lain!
            const droppedTables = localTables.filter((t) => prevServerTables.has(t) && !pmaTableSet.has(t));
            const newTables = tables.filter((t) => !localTableSet.has(t));

            // 1. Drop tabel lokal yang sudah tidak ada di PMA
            if (droppedTables.length > 0) {
              addLog({
                type: 'warning',
                message: `🗑️ Menghapus ${droppedTables.length} tabel di MySQL lokal yang sudah tidak ada di PMA: ${droppedTables.join(', ')}...`,
                timestamp: new Date().toLocaleTimeString(),
              });

              await safeInvoke('drop_local_tables', {
                config: {
                  host: localConfig.value.host || '127.0.0.1',
                  port: parseInt(localConfig.value.port || 3306, 10),
                  username: localConfig.value.username || 'root',
                  password: localConfig.value.password || '',
                  database: localConfig.value.database || '',
                  use_docker: Boolean(localConfig.value.use_docker),
                  docker_container: localConfig.value.docker_container || '',
                },
                tables: droppedTables,
              });

              droppedTables.forEach((t) => {
                clearTableState(pmaConfig.value.url, pmaConfig.value.database, t);
              });

              addLog({
                type: 'success',
                message: `✅ Selesai menghapus ${droppedTables.length} tabel usang dari MySQL lokal.`,
                timestamp: new Date().toLocaleTimeString(),
              });
            }

            // 2. Tambahkan tabel baru dari PMA yang belum ada di local
            // 2. Tambahkan tabel baru dari PMA yang belum ada di local (Hanya struktur tabel / DDL, penarikan data dilakukan pada proses sync)
            if (newTables.length > 0) {
              addLog({
                type: 'info',
                message: `📥 Ditemukan ${newTables.length} tabel baru di PMA yang belum ada di lokal: ${newTables.join(', ')}. Membuat struktur tabel di MySQL lokal...`,
                timestamp: new Date().toLocaleTimeString(),
              });

              const syncEngine = new SyncEngine(pmaConfig.value, localConfig.value, {
                onLog: addLog,
                onTableSynced: refreshTableStates,
                onProgress: () => {},
              });

              await syncEngine.runMultiTableSync({
                tables: newTables,
                syncMode: 'structure_only',
                rowLimit: 0,
              });

              addLog({
                type: 'success',
                message: `✅ Selesai membuat struktur ${newTables.length} tabel baru di MySQL lokal (data akan disinkronkan saat proses Sync).`,
                timestamp: new Date().toLocaleTimeString(),
              });
            }

            if (droppedTables.length === 0 && newTables.length === 0) {
              addLog({
                type: 'info',
                message: `✨ Semua ${tables.length} tabel di PMA dan MySQL lokal sudah sinkron dan selaras.`,
                timestamp: new Date().toLocaleTimeString(),
              });
            }

            // Perbarui selectedTables
            const currentSelected = new Set(selectedTables.value || []);
            droppedTables.forEach((t) => currentSelected.delete(t));
            newTables.forEach((t) => currentSelected.add(t));
            if (currentSelected.size === 0) {
              selectedTables.value = [...tables];
            } else {
              selectedTables.value = tables.filter((t) => currentSelected.has(t));
            }
          } else {
            selectedTables.value = [...tables];
          }
        } catch (localErr) {
          addLog({
            type: 'warning',
            message: `Pengecekan database MySQL lokal dilewati: ${localErr.message || localErr}`,
            timestamp: new Date().toLocaleTimeString(),
          });
          selectedTables.value = [...tables];
        }
      } else {
        selectedTables.value = [...tables];
      }

      availableTables.value = tables;
      saveTablesForServerKey(currentServerKey.value, tables, selectedTables.value);
      refreshTableStates();
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
  const targetDesc = localConfig.value.use_docker && localConfig.value.docker_container
    ? `Docker [${localConfig.value.docker_container}] & ${localConfig.value.host}:${localConfig.value.port || 3306}`
    : `${localConfig.value.host}:${localConfig.value.port || 3306}`;

  addLog({
    type: 'info',
    message: `Menguji koneksi port native MySQL lokal (${targetDesc})...`,
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
        use_docker: Boolean(localConfig.value.use_docker),
        docker_container: localConfig.value.docker_container || '',
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

  const expectedTotal = tablesToSync.length;

  isSyncing.value = true;
  syncProgress.value = {
    currentTableIndex: 0,
    totalTables: expectedTotal,
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
      const rawTableIndex = p.currentTableIndex || 0;
      // Pastikan index tabel tidak pernah melebihi total dan tidak melompat mundur karena out-of-order event multi-worker
      const safeTableIndex = Math.min(
        Math.max(rawTableIndex, syncProgress.value.currentTableIndex || 0),
        expectedTotal
      );
      const safeTotalRows = Math.max(
        p.totalSyncedAllTables || 0,
        syncProgress.value.totalSyncedAllTables || 0
      );

      syncProgress.value = {
        currentTableIndex: safeTableIndex,
        totalTables: expectedTotal, // Terkunci presisi ke jumlah tabel yang dipilih
        currentTableName: p.currentTableName || syncProgress.value.currentTableName || '',
        rowsSyncedCurrentTable: rowsCount,
        rowsSyncedForCurrentTable: rowsCount,
        totalSyncedAllTables: safeTotalRows,
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

<style>
.app-layout { display:flex; flex-direction:column; width:100%; height:100vh; min-width:0; overflow:hidden; }
.desktop-frame { flex:1 1 auto; width:100%; min-width:0; }
.desktop-content { width:100%; min-width:0; }
.browser-banner { flex:0 0 auto; }
</style>

