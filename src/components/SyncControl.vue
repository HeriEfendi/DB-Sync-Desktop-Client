<template>
  <div class="glass-panel sync-control-card">
    <div class="control-grid">
      <div class="main-action">
        <button
          v-if="!isSyncing"
          class="btn btn-primary btn-sm"
          @click="$emit('start-sync')"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <polyline points="23 4 23 10 17 10"></polyline>
            <polyline points="1 20 1 14 7 14"></polyline>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
          </svg>
          <span class="btn-text">Mulai Sinkronisasi Data</span>
        </button>

        <button
          v-else
          class="btn btn-danger btn-sm btn-stop"
          title="Klik untuk menghentikan sinkronisasi saat ini. Data yang sudah masuk tetap tersimpan."
          @click="$emit('stop-sync')"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
            <rect x="6" y="6" width="12" height="12" rx="2"></rect>
          </svg>
          <span class="btn-text">Hentikan Sinkronisasi</span>
        </button>

        <div class="auto-sync-box">
          <label class="auto-sync-label">
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"></circle><polyline points="12 6 12 12 16 14"></polyline></svg>
            <span>Otomatisasi:</span>
          </label>
          <select
            :value="autoSyncInterval"
            class="form-select interval-select"
            @change="$emit('update:autoSyncInterval', parseInt($event.target.value, 10))"
          > 
            <option :value="0">Non-Aktif (Manual)</option>
            <option :value="5">Setiap 5 Detik</option>
            <option :value="10">Setiap 10 Detik</option>
            <option :value="30">Setiap 30 Detik</option>
            <option :value="60">Setiap 1 Menit</option>
            <option :value="300">Setiap 5 Menit</option>
          </select>
        </div>
      </div>

      <div class="stats-grid">
        <div class="stat-card" :class="{ 'stat-highlight': isSyncing }">
          <span class="stat-value text-emerald">
            {{ isSyncing ? formatNumber(syncProgress.totalSyncedAllTables) : formatNumber(stats.totalSynced) }}
          </span>
          <span class="stat-label">{{ isSyncing ? 'Baris Masuk (Live)' : 'Total Data Disinkron' }}</span>
        </div>

        <div v-if="isSyncing" class="stat-card stat-live" style="width: 300px;">
          <span class="stat-value text-cyan">
            {{ formatNumber(syncProgress.currentTableIndex) }} / {{ formatNumber(syncProgress.totalTables) }}
          </span>
           <span class="stat-label">Tabel Process: <span class="table-name-truncated" :title="syncProgress.currentTableName">{{ syncProgress.currentTableName || '...' }}</span></span>
        </div>

        <div v-else class="stat-card">
          <span class="stat-value text-cyan">{{ formatDuration(stats.lastDuration) }}</span>
          <span class="stat-label">Durasi Terakhir</span>
        </div>

        <div v-if="!isSyncing" class="stat-card">
          <span class="stat-value text-amber">{{ (stats.lastSyncTime || '-') }}</span>
          <span class="stat-label">Waktu Sinkron Terakhir</span>
        </div>
        <div v-else class="stat-card stat-live">
          <span class="stat-value text-amber">{{ elapsedDisplay }}</span>
          <span class="stat-label">Waktu Berjalan</span>
        </div>
      </div>
    </div>
    <div class="sync-options-card">
      <div class="options-row">
        <div class="option-inline">
          <span class="option-title">Mode:</span>
          <label class="radio-label-inline" title="Server sumber data utama. Menghapus data lokal di atas Last ID, lalu mengambil ID baru dan data updated_at terbaru dari server."><input type="radio" name="syncMode" value="incremental" :checked="syncMode === 'incremental'" @change="$emit('update:sync-mode', 'incremental')" /> <span>Sync Server (New &amp; Update)</span></label>
          <label class="radio-label-inline warning-radio"><input type="radio" name="syncMode" value="fresh" :checked="syncMode === 'fresh'" @change="$emit('update:sync-mode', 'fresh')" /> <span>Fresh Sync</span></label>
        </div>
        <div class="option-inline">
          <span class="option-title">Limit:</span>
          <select
            :value="Number.isFinite(Number(rowLimit)) ? Number(rowLimit) : 0"
            class="form-select limit-select"
            @change="$emit('update:row-limit', Number($event.target.value) || 0)"
          >
            <option :value="0">Semua Row (Default)</option><option :value="1000">1.000 Row</option><option :value="10000">10.000 Row</option><option :value="100000">100.000 Row</option><option :value="500000">500.000 Row</option><option :value="2000000">2.000.000 Row</option>
          </select>
        </div>
      </div>
    </div>

    <div v-if="isSyncing" class="sync-live-banner">
      <div class="sync-live-content">
        <span class="pulse-dot"></span>
        <span class="live-status-text">
          Menyinkronkan <strong class="badge-accent">{{ syncProgress.currentTableIndex }}/{{ syncProgress.totalTables }}</strong> tabel
          <strong v-if="syncProgress.currentTableName" class="badge-tableName">'{{ syncProgress.currentTableName }}'</strong>:
          <span class="text-emerald font-bold">{{ formatNumber(syncProgress.rowsSyncedCurrentTable) }} baris</span> dimasukkan
          (Total Session: <strong class="text-white">{{ formatNumber(syncProgress.totalSyncedAllTables) }}</strong> baris).
        </span>
      </div>
    </div>

    <div v-if="isSyncing" class="progress-bar-container">
      <div
        class="progress-bar-fill"
        :style="{ width: syncProgress.totalTables > 0 ? `${Math.min(100, (syncProgress.currentTableIndex / syncProgress.totalTables) * 100)}%` : '0%' }"
      ></div>
    </div>
    <div v-if="syncTableStates && syncTableStates.length > 0" class="sync-states-card">
      <div class="states-header">
        <button class="states-toggle" type="button" :aria-expanded="showSyncStates" aria-controls="sync-states-table" @click="showSyncStates = !showSyncStates">
          <span class="states-title-wrap">
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 8v4l3 3"></path><circle cx="12" cy="12" r="9"></circle></svg>
            <span class="states-title">State &amp; ID Sync Terakhir per Tabel</span>
            <span class="state-count">{{ syncTableStates.length }}</span>
          </span>
          <svg class="toggle-chevron" :class="{ open: showSyncStates }" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 9 6 6 6-6"></path></svg>
        </button>
        <button v-if="showSyncStates" class="btn btn-ghost btn-xs text-rose" title="Reset semua riwayat sync state" @click="$emit('reset-all-table-states')">
          Reset Semua State
        </button>
      </div>
      <div v-show="showSyncStates" id="sync-states-table" class="states-table-wrapper">
        <table class="states-table">
          <thead>
            <tr>
              <th>Server / DB</th>
              <th>Tabel</th>
              <th>Last Synced ID</th>
              <th>Waktu Sync Terakhir</th>
              <th>Aksi</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="st in syncTableStates" :key="st.key">
              <td class="font-mono text-dim">{{ st.server }} / {{ st.database }}</td>
              <td class="font-mono text-emerald"><strong>{{ st.table }}</strong></td>
              <td class="font-mono text-amber">
                <strong>{{ st.lastSyncedId !== null && st.lastSyncedId !== undefined ? st.lastSyncedId : '-' }}</strong>
                <small v-if="st.primaryKey" class="text-dim"> ({{ st.primaryKey }})</small>
              </td>
              <td class="text-muted">{{ formatDate(st.lastSyncTime) }}</td>
              <td>
                <button class="btn btn-danger btn-xs" title="Reset state tabel ini" @click="$emit('reset-table-state', st)">
                  Reset
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, watch, computed, onUnmounted } from 'vue';
const props = defineProps({
  isSyncing: { type: Boolean, default: false },
  autoSyncInterval: { type: Number, default: 0 },
  syncMode: { type: String, default: 'incremental' },
  rowLimit: { type: Number, default: 0 },
  stats: {
    type: Object,
    default: () => ({
      totalSynced: 0,
      lastDuration: 0,
      lastSyncTime: '',
    }),
  },
  syncProgress: {
    type: Object,
    default: () => ({
      currentTableIndex: 0,
      totalTables: 0,
      currentTableName: '',
      rowsSyncedCurrentTable: 0,
      totalSyncedAllTables: 0,
    }),
  },
  syncTableStates: {
    type: Array,
    default: () => [],
  },
});

const emit = defineEmits([
  'start-sync',
  'stop-sync',
  'update:autoSyncInterval',
  'update:sync-mode',
  'update:row-limit',
  'reset-table-state',
  'reset-all-table-states',
]);

// --- Realtime elapsed timer ---
const elapsedSeconds = ref(0);
const showSyncStates = ref(false);
let elapsedTimer = null;

watch(() => props.isSyncing, (syncing) => {
  if (syncing) {
    elapsedSeconds.value = 0;
    elapsedTimer = setInterval(() => { elapsedSeconds.value++; }, 1000);
  } else {
    clearInterval(elapsedTimer);
    elapsedTimer = null;
  }
});

onUnmounted(() => { clearInterval(elapsedTimer); });

const elapsedDisplay = computed(() => {
  const s = elapsedSeconds.value;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}j ${m}m ${sec}d`;
  if (m > 0) return `${m}m ${sec}d`;
  return `${sec}d`;
});
// --- End timer ---

const formatDate = (isoString) => {
  if (!isoString) return '-';
  try {
    const d = new Date(isoString);
    return d.toLocaleString('id-ID', { dateStyle: 'short', timeStyle: 'medium' });
  } catch {
    return isoString;
  }
};

const formatNumber = (val) => {
  if (val === null || val === undefined || isNaN(val)) return '0';
  return Number(val).toLocaleString('id-ID');
};

const formatDuration = (ms) => {
  if (!ms || isNaN(ms)) return '-';
  if (ms < 1000) return `${ms}ms`;

  const totalSec = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSec / 60);
  const seconds = totalSec % 60;

  if (minutes === 0) {
    return `${seconds}d`;
  }
  return `${minutes}m ${seconds}d`;
};
</script>

<style scoped>
.sync-control-card {
  padding: 18px 24px 22px;
  position: relative;
  overflow: hidden;
  min-height: auto;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.btn-danger {
  background: linear-gradient(135deg, #ef4444, #dc2626) !important;
  color: white !important;
  border: 1px solid rgba(239, 68, 68, 0.5) !important;
  box-shadow: 0 4px 14px rgba(239, 68, 68, 0.4);
  transition: all 0.2s ease;
}

.btn-danger:hover {
  background: linear-gradient(135deg, #dc2626, #b91c1c) !important;
  box-shadow: 0 6px 18px rgba(239, 68, 68, 0.6);
  transform: translateY(-1px);
}

.control-grid {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 24px;
  flex-wrap: wrap;
}

.main-action {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}

.auto-sync-box {
  display: flex;
  align-items: center;
  gap: 8px;
  background: rgba(15, 23, 42, 0.6);
  padding: 6px 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
}

.auto-sync-label {
  font-size: 0.8rem;
  color: var(--text-muted);
  display: flex;
  align-items: center;
  gap: 4px;
}

.interval-select {
  padding: 0px 2px;
  font-size: 0.8rem;
  width: auto;
  border: none;
  background: transparent;
  color: #111827;
}

.interval-select option {
  color: #111827;
}

.sync-options-card,
.sync-settings-box {
  background: rgba(15, 23, 42, 0.5);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 10px 14px;
  width: 100%;
}

.options-row,
.sync-settings-row {
  display: flex;
  align-items: center;
  gap: 24px;
  flex-wrap: wrap;
}

.option-inline {
  display: flex;
  align-items: center;
  gap: 8px;
}

.stat-label {
  min-width: 0;
}

.table-name-truncated {
  display: inline-block;
  max-width: 12rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: bottom;
}

.option-title {
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.radio-label-inline {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  font-size: 0.82rem;
  color: var(--text-main);
  padding: 5px 10px;
  border-radius: 6px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  white-space: nowrap;
  transition: all 0.15s ease;
}

.radio-label-inline:hover {
  background: rgba(255, 255, 255, 0.08);
}

.radio-label-inline.warning-radio {
  border-color: rgba(245, 158, 11, 0.3);
}

.limit-select {
  padding: 0px 2px;
  font-size: 0.8rem;
  width: auto;
  min-width: 160px;
  max-width: 260px;
  color: #111827;
}

.limit-select option {
  color: #111827;
}

.stats-grid {
  display: flex;
  gap: 14px;
  flex-wrap: wrap;
  justify-content: flex-end;
  align-items: stretch;
}

.stat-card {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  background: rgba(15, 23, 42, 0.4);
  padding: 8px 14px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
  min-width: 130px;
  transition: all 0.2s ease;
}

.stat-highlight {
  border-color: rgba(16, 185, 129, 0.4);
  background: rgba(16, 185, 129, 0.08);
}

.stat-live {
  border-color: rgba(6, 182, 212, 0.4);
  background: rgba(6, 182, 212, 0.08);
}

.stat-value {
  font-size: 1.15rem;
  font-weight: 700;
  font-family: 'JetBrains Mono', monospace;
}

.stat-label {
  font-size: 0.72rem;
  color: var(--text-muted);
  white-space: nowrap;
}

.text-emerald { color: var(--accent-emerald); }
.text-cyan { color: var(--accent-cyan); }
.text-amber { color: var(--accent-amber); }
.text-white { color: #ffffff; }
.font-bold { font-weight: 600; }

.sync-live-banner {
  width: 100%;
  padding: 10px 14px;
  background: rgba(15, 23, 42, 0.75);
  border-radius: var(--radius-md);
  border: 1px solid rgba(6, 182, 212, 0.35);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
}

.sync-live-content {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  min-width: 0;
}

.live-status-text {
  font-size: 0.83rem;
  color: var(--text-main);
  line-height: 1.4;
  word-break: break-word;
  flex: 1;
}

.badge-accent {
  color: var(--accent-cyan);
  background: rgba(6, 182, 212, 0.15);
  padding: 2px 6px;
  border-radius: 4px;
  font-weight: 600;
}

.badge-tableName {
  color: #a7f3d0;
  background: rgba(16, 185, 129, 0.15);
  padding: 2px 6px;
  border-radius: 4px;
  font-family: 'JetBrains Mono', monospace;
}

.pulse-dot {
  width: 9px;
  height: 9px;
  flex: 0 0 9px;
  border-radius: 50%;
  background: #10b981;
  box-shadow: 0 0 10px #10b981;
  animation: pulse-glow 1.5s infinite;
}

.progress-bar-container {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 4px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
}

.progress-bar-fill {
  height: 100%;
  background: linear-gradient(90deg, var(--primary), var(--accent-cyan), var(--accent-emerald));
  transition: width 0.3s ease;
}

@keyframes pulse-glow {
  0%, 100% { opacity: 1; transform: scale(1); box-shadow: 0 0 10px #10b981; }
  50% { opacity: 0.4; transform: scale(0.85); box-shadow: 0 0 3px #10b981; }
}

.sync-states-card {
  margin-top: 8px;
  background: rgba(15, 23, 42, 0.6);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: var(--radius-md);
  padding: 12px 14px;
}

.states-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.states-toggle {
  display: flex;
  flex: 1;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  min-width: 0;
  padding: 2px 0;
  color: var(--text-muted);
  background: transparent;
  border: 0;
  cursor: pointer;
  text-align: left;
}

.states-toggle:focus-visible {
  outline: 2px solid var(--accent-cyan);
  outline-offset: 3px;
  border-radius: 4px;
}

.states-title-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-muted);
}

.state-count {
  padding: 1px 6px;
  border-radius: 999px;
  background: rgba(56, 189, 248, 0.12);
  color: var(--accent-cyan);
  font-size: 0.7rem;
  font-weight: 700;
}

.toggle-chevron {
  flex: 0 0 auto;
  transition: transform 0.2s ease;
}

.toggle-chevron.open {
  transform: rotate(180deg);
}

.states-title {
  font-size: 0.78rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.text-rose { color: var(--accent-rose); }

.states-table-wrapper {
  overflow-x: auto;
  max-height: 200px;
  overflow-y: auto;
}

.states-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.76rem;
  text-align: left;
}

.states-table th,
.states-table td {
  padding: 6px 10px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
}

.states-table th {
  color: var(--text-dim);
  font-weight: 600;
  text-transform: uppercase;
  font-size: 0.68rem;
}

.states-table tr:hover {
  background: rgba(255, 255, 255, 0.03);
}

@media (max-width: 768px) {
  .control-grid {
    flex-direction: column;
    align-items: stretch;
  }
  .stats-grid {
    justify-content: space-between;
  }
  .stat-card {
    flex: 1;
    min-width: 100px;
  }
}
</style>
