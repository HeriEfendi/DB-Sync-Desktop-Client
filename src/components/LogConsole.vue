<template>
  <div class="glass-panel log-console-card">
    <!-- Log Console Toolbar -->
    <div class="console-toolbar">
      <div class="toolbar-left">
        <span class="console-title">
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>
          Console Log Sinkronisasi
        </span>
        <span class="log-count">({{ filteredLogs.length }} entri)</span>
      </div>

      <!-- Filter Buttons & Search -->
      <div class="toolbar-right">
        <div class="filter-pills">
          <button
            v-for="lvl in ['all', 'info', 'success', 'warning', 'error']"
            :key="lvl"
            class="filter-pill"
            :class="{ active: currentFilter === lvl, [lvl]: true }"
            @click="currentFilter = lvl"
          >
            {{ lvl.toUpperCase() }}
          </button>
        </div>

        <input
          v-model="searchQuery"
          type="text"
          class="form-input search-input"
          placeholder="Cari log..."
        />

        <button class="btn btn-secondary btn-icon" title="Salin Log" @click="copyLogs">
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
        </button>

        <button class="btn btn-danger btn-icon" title="Bersihkan Log" @click="$emit('clear-logs')">
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
        </button>
      </div>
    </div>

    <!-- Log Display Window -->
    <div ref="logContainer" class="log-window font-mono">
      <div v-if="filteredLogs.length === 0" class="empty-log">
        <span>Belum ada log aktivitas sinkronisasi. Tekan "Mulai Sinkronisasi Data" untuk melihat alur kerja.</span>
      </div>

      <div
        v-for="(log, idx) in filteredLogs"
        :key="idx"
        class="log-row"
        :class="`log-${log.type}`"
      >
        <span class="log-time">[{{ log.timestamp }}]</span>
        <span class="log-level-badge">{{ log.type.toUpperCase() }}</span>
        <span class="log-msg">{{ log.message }}</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, watch, nextTick } from 'vue';

const props = defineProps({
  logs: { type: Array, default: () => [] },
});

defineEmits(['clear-logs']);

const logContainer = ref(null);
const currentFilter = ref('all');
const searchQuery = ref('');

const filteredLogs = computed(() => {
  const recentLogs = props.logs.length > 300 ? props.logs.slice(-300) : props.logs;
  const list = recentLogs.filter((log) => {
    const matchesLevel = currentFilter.value === 'all' || log.type === currentFilter.value;
    const matchesSearch = !searchQuery.value || log.message.toLowerCase().includes(searchQuery.value.toLowerCase());
    return matchesLevel && matchesSearch;
  });
  return list.slice().reverse();
});

// Auto-scroll to top on new log entry (since newest logs are at the top)
watch(
  () => props.logs.length,
  async () => {
    await nextTick();
    if (logContainer.value) {
      logContainer.value.scrollTop = 0;
    }
  }
);

const copyLogs = () => {
  const text = props.logs.map((l) => `[${l.timestamp}] [${l.type.toUpperCase()}] ${l.message}`).join('\n');
  navigator.clipboard.writeText(text);
  alert('Seluruh log berhasil disalin ke clipboard!');
};
</script>

<style scoped>
.log-console-card {
  display: flex;
  flex-direction: column;
  min-height: 290px;
  max-height: 48vh;
  overflow: hidden;
}

.console-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 16px;
  background: rgba(15, 23, 42, 0.7);
  border-bottom: 1px solid var(--border-color);
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.console-title {
  font-size: 0.85rem;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-main);
}

.log-count {
  font-size: 0.75rem;
  color: var(--text-dim);
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.filter-pills {
  display: flex;
  gap: 4px;
}

.filter-pill {
  padding: 2px 8px;
  font-size: 0.7rem;
  font-weight: 600;
  border-radius: 4px;
  border: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
}

.filter-pill.active {
  background: var(--bg-card-hover);
  color: var(--text-main);
  border-color: var(--primary);
}

.search-input {
  padding: 4px 8px;
  font-size: 0.76rem;
  width: 140px;
}

.btn-icon {
  padding: 6px;
}

.log-window {
  min-height: 220px;
  flex: 1;
  padding: 12px 16px;
  overflow-y: auto;
  font-size: 0.81rem;
  line-height: 1.6;
  background: #090d16;
}

.empty-log {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-dim);
  font-style: italic;
  font-size: 0.82rem;
}

.log-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-bottom: 4px;
  word-break: break-all;
}

.log-time {
  color: var(--text-dim);
  white-space: nowrap;
}

.log-level-badge {
  font-size: 0.68rem;
  font-weight: 700;
  padding: 1px 5px;
  border-radius: 3px;
  white-space: nowrap;
}

.log-info .log-level-badge { background: rgba(99, 102, 241, 0.2); color: #818cf8; }
.log-info .log-msg { color: #cbd5e1; }

.log-success .log-level-badge { background: rgba(16, 185, 129, 0.2); color: #34d399; }
.log-success .log-msg { color: #a7f3d0; }

.log-warning .log-level-badge { background: rgba(245, 158, 11, 0.2); color: #fbbf24; }
.log-warning .log-msg { color: #fde68a; }

.log-error .log-level-badge { background: rgba(244, 63, 94, 0.2); color: #f87171; }
.log-error .log-msg { color: #fca5a5; }
</style>
