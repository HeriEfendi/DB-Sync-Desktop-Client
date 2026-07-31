<template>
  <div class="glass-panel sync-control-card">
    <div class="control-grid">
      <!-- Main Trigger Action -->
      <div class="main-action">
        <button
          v-if="!isSyncing"
          class="btn btn-primary btn-lg"
          @click="$emit('start-sync')"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <polyline points="23 4 23 10 17 10"></polyline>
            <polyline points="1 20 1 14 7 14"></polyline>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
          </svg>
          <span class="btn-text">Mulai Sinkronisasi Data</span>
        </button>

        <button
          v-else
          class="btn btn-danger btn-lg btn-stop"
          title="Klik untuk menghentikan sinkronisasi saat ini. Data yang sudah masuk tetap tersimpan."
          @click="$emit('stop-sync')"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
            <rect x="6" y="6" width="12" height="12" rx="2"></rect>
          </svg>
          <span class="btn-text">Hentikan Sinkronisasi</span>
        </button>

        <!-- Auto Sync Timer Select -->
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

      <!-- Live Sync Statistics -->
      <div class="stats-grid">
        <div class="stat-card" :class="{ 'stat-highlight': isSyncing }">
          <span class="stat-value text-emerald">
            {{ isSyncing ? formatNumber(syncProgress.totalSyncedAllTables) : formatNumber(stats.totalSynced) }}
          </span>
          <span class="stat-label">{{ isSyncing ? 'Baris Masuk (Live)' : 'Total Data Disinkron' }}</span>
        </div>

        <div v-if="isSyncing" class="stat-card stat-live">
          <span class="stat-value text-cyan">
            {{ formatNumber(syncProgress.currentTableIndex) }} / {{ formatNumber(syncProgress.totalTables) }}
          </span>
          <span class="stat-label">Tabel Process: {{ syncProgress.currentTableName || '...' }}</span>
        </div>

        <div v-else class="stat-card">
          <span class="stat-value text-cyan">{{ formatDuration(stats.lastDuration) }}</span>
          <span class="stat-label">Durasi Terakhir</span>
        </div>

        <div class="stat-card">
          <span class="stat-value text-amber">{{ isSyncing ? `${formatNumber(syncProgress.rowsSyncedCurrentTable)} baris` : (stats.lastSyncTime || '-') }}</span>
          <span class="stat-label">{{ isSyncing ? 'Row Tabel Saat Ini' : 'Waktu Sinkron Terakhir' }}</span>
        </div>
      </div>
    </div>

    <!-- Active Sync Live Progress Status Info -->
    <div v-if="isSyncing" class="sync-live-banner">
      <div class="live-indicator">
        <span class="live-dot"></span>
        <span class="live-text">
          Menyinkronkan {{ syncProgress.currentTableIndex }}/{{ syncProgress.totalTables }} tabel
          <strong v-if="syncProgress.currentTableName">('{{ syncProgress.currentTableName }}')</strong>:
          <span class="text-emerald">{{ formatNumber(syncProgress.rowsSyncedCurrentTable) }} baris</span> tabel ini dimasukkan (Total Session: <strong>{{ formatNumber(syncProgress.totalSyncedAllTables) }}</strong> baris).
        </span>
      </div>
    </div>

    <!-- Active Sync Progress Bar -->
    <div v-if="isSyncing" class="progress-bar-container">
      <div
        class="progress-bar-fill"
        :style="{ width: syncProgress.totalTables > 0 ? `${Math.min(100, (syncProgress.currentTableIndex / syncProgress.totalTables) * 100)}%` : '0%' }"
      ></div>
    </div>
  </div>
</template>

<script setup>
defineProps({
  isSyncing: { type: Boolean, default: false },
  autoSyncInterval: { type: Number, default: 0 },
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
      status: 'idle',
    }),
  },
});

defineEmits(['start-sync', 'stop-sync', 'update:autoSyncInterval']);

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
  padding: 18px 24px;
  position: relative;
  overflow: hidden;
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
}

.main-action {
  display: flex;
  align-items: center;
  gap: 16px;
}

.btn-lg {
  padding: 12px 24px;
  font-size: 0.95rem;
  border-radius: var(--radius-md);
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
  padding: 4px 8px;
  font-size: 0.8rem;
  width: auto;
  border: none;
  background: transparent;
  color: #111827;
}

.interval-select option {
  color: #111827;
}

.stats-grid {
  display: flex;
  gap: 16px;
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

.spin-lg {
  width: 18px;
  height: 18px;
  border: 3px solid rgba(255, 255, 255, 0.3);
  border-top-color: white;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.sync-live-banner {
  margin-top: 12px;
  padding: 8px 12px;
  background: rgba(15, 23, 42, 0.7);
  border-radius: var(--radius-md);
  border: 1px solid rgba(6, 182, 212, 0.3);
}

.live-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.82rem;
  color: var(--text-main);
}

.live-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #10b981;
  box-shadow: 0 0 8px #10b981;
  animation: pulse 1.5s infinite;
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

@keyframes pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.4; transform: scale(0.85); }
}

@keyframes spin {
  100% { transform: rotate(360deg); }
}
</style>

