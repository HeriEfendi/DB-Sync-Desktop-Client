<template>
  <div class="glass-panel preview-card">
    <div class="preview-header">
      <div class="header-left">
        <h4>Preview & Data Inspector</h4>
        <span class="count-tag" v-if="rows.length > 0">({{ rows.length }} baris)</span>
      </div>

      <div class="header-right">
        <div class="view-toggle">
          <button
            class="toggle-btn"
            :class="{ active: viewMode === 'table' }"
            @click="viewMode = 'table'"
          >
            Tabel
          </button>
          <button
            class="toggle-btn"
            :class="{ active: viewMode === 'json' }"
            @click="viewMode = 'json'"
          >
            Raw JSON
          </button>
        </div>

        <button class="btn btn-secondary btn-sm" @click="$emit('refresh-local-preview')">
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21.5 2v6h-6M2.13 15.57a10 10 0 1 0 0-7.14l-1.63-1.63"></path></svg>
          <span>Preview DB Lokal</span>
        </button>
      </div>
    </div>

    <!-- Table View -->
    <div v-if="viewMode === 'table'" class="table-wrapper">
      <table v-if="rows.length > 0" class="data-table">
        <thead>
          <tr>
            <th v-for="col in columns" :key="col">{{ col }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(row, idx) in rows" :key="idx">
            <td v-for="col in columns" :key="col">
              <span v-if="row[col] === null || row[col] === undefined" class="null-val">NULL</span>
              <span v-else>{{ row[col] }}</span>
            </td>
          </tr>
        </tbody>
      </table>

      <div v-else class="empty-state">
        <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect><line x1="3" y1="9" x2="21" y2="9"></line><line x1="9" y1="21" x2="9" y2="9"></line></svg>
        <p>Belum ada data untuk dipreview. Jalankan sinkronisasi atau klik "Preview DB Lokal".</p>
      </div>
    </div>

    <!-- JSON View -->
    <div v-else class="json-wrapper font-mono">
      <pre v-if="rows.length > 0">{{ JSON.stringify(rows, null, 2) }}</pre>
      <div v-else class="empty-state">
        <p>Data kosong.</p>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue';

const props = defineProps({
  rows: { type: Array, default: () => [] },
});

defineEmits(['refresh-local-preview']);

const viewMode = ref('table');

const columns = computed(() => {
  if (!props.rows || props.rows.length === 0) return [];
  return Object.keys(props.rows[0]);
});
</script>

<style scoped>
.preview-card {
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
  height: 280px;
  overflow: hidden;
}

.preview-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.header-left h4 {
  font-size: 0.92rem;
  font-weight: 600;
}

.count-tag {
  font-size: 0.76rem;
  color: var(--text-muted);
}

.header-right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.view-toggle {
  display: flex;
  background: rgba(15, 23, 42, 0.6);
  border-radius: var(--radius-sm);
  padding: 2px;
}

.toggle-btn {
  padding: 4px 10px;
  font-size: 0.75rem;
  background: transparent;
  border: none;
  color: var(--text-muted);
  border-radius: 4px;
  cursor: pointer;
}

.toggle-btn.active {
  background: var(--bg-card);
  color: var(--text-main);
}

.table-wrapper {
  flex: 1;
  overflow: auto;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
}

.data-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.8rem;
  text-align: left;
}

.data-table th {
  position: sticky;
  top: 0;
  background: #1e293b;
  padding: 8px 12px;
  font-weight: 600;
  color: var(--text-muted);
  border-bottom: 1px solid var(--border-color);
  white-space: nowrap;
}

.data-table td {
  padding: 6px 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  color: var(--text-main);
  white-space: nowrap;
}

.data-table tr:hover td {
  background: rgba(255, 255, 255, 0.03);
}

.null-val {
  color: var(--text-dim);
  font-style: italic;
  font-size: 0.75rem;
}

.json-wrapper {
  flex: 1;
  overflow: auto;
  padding: 12px;
  background: #090d16;
  border-radius: var(--radius-sm);
  font-size: 0.78rem;
  color: var(--accent-cyan);
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  gap: 8px;
  color: var(--text-dim);
  font-size: 0.82rem;
}
</style>
