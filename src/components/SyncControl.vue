<template>
  <div class="glass-panel sync-control-card">
    <div class="control-grid">
      <!-- Main Trigger Action -->
      <div class="main-action">
        <button
          class="btn btn-primary btn-lg"
          :disabled="isSyncing"
          @click="$emit('start-sync')"
        >
          <svg v-if="!isSyncing" xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <polyline points="23 4 23 10 17 10"></polyline>
            <polyline points="1 20 1 14 7 14"></polyline>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
          </svg>
          <span v-else class="spin-lg"></span>
          <span class="btn-text">{{ isSyncing ? 'Proses Sinkronisasi...' : 'Mulai Sinkronisasi Data' }}</span>
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
        <div class="stat-card">
          <span class="stat-value text-emerald">{{ stats.totalSynced }}</span>
          <span class="stat-label">Total Data Disinkron</span>
        </div>

        <div class="stat-card">
          <span class="stat-value text-cyan">{{ stats.lastDuration ? `${stats.lastDuration}ms` : '-' }}</span>
          <span class="stat-label">Durasi Terakhir</span>
        </div>

        <div class="stat-card">
          <span class="stat-value text-amber">{{ stats.lastSyncTime || '-' }}</span>
          <span class="stat-label">Waktu Sinkron Terakhir</span>
        </div>
      </div>
    </div>

    <!-- Active Sync Progress Bar -->
    <div v-if="isSyncing" class="progress-bar-container">
      <div class="progress-bar-fill"></div>
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
});

defineEmits(['start-sync', 'update:autoSyncInterval']);
</script>

<style scoped>
.sync-control-card {
  padding: 18px 24px;
  position: relative;
  overflow: hidden;
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
}

.stat-value {
  font-size: 1.15rem;
  font-weight: 700;
  font-family: 'JetBrains Mono', monospace;
}

.stat-label {
  font-size: 0.72rem;
  color: var(--text-muted);
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

.progress-bar-container {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 3px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
}

.progress-bar-fill {
  height: 100%;
  width: 40%;
  background: linear-gradient(90deg, var(--primary), var(--accent-cyan));
  animation: shimmer 1.2s infinite ease-in-out;
}

@keyframes shimmer {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(300%); }
}

@keyframes spin {
  100% { transform: rotate(360deg); }
}
</style>
