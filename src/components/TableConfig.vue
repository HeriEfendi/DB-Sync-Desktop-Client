<template>
  <div class="glass-panel config-card">
    <div class="card-header">
      <div class="title-wrap">
        <div class="title-row">
          <h3>Daftar Tabel</h3>
          <!-- <span class="selection-badge">
            <span class="badge-dot"></span>
            <strong>{{ selectedTables.length }}</strong> dari {{ availableTables.length }} tabel dipilih
          </span> -->
        </div>
        <p class="subtitle">Pilih tabel yang akan disinkronkan dan simpan template centangan untuk reuse.</p>
      </div>
    </div>
    <div class="table-tools-bar">
      <button class="btn btn-primary btn-sm" :disabled="fetchingTables" @click="$emit('fetch-tables')">
        <span v-if="fetchingTables" class="spin-sm"></span>
        <span>Ambil Tabel</span>
      </button>
      <div class="table-search-box"><input v-model="tableSearchQuery" type="text" class="form-input form-input-sm" placeholder="Cari tabel..." /></div>
      <button class="btn btn-ghost btn-xs" @click="selectAllTables">Centang Semua</button>
      <button class="btn btn-ghost btn-xs" @click="deselectAllTables">Hapus Centang</button>
      <div class="selection-info-pill">
        <span>Terpilih: <strong>{{ selectedTables.length }}</strong> / {{ availableTables.length }}</span>
      </div>
      <div class="tools-divider"></div>
      <select v-model="selectedTemplateName" class="form-select preset-select" @change="loadTableTemplate"><option value="">-- Template Tabel --</option><option v-for="(t, key) in tableTemplates" :key="key" :value="key">{{ key }}</option></select>
      <button class="btn btn-secondary btn-xs" :disabled="!selectedTemplateName" @click="updateTableTemplate">Simpan</button>
      <button class="btn btn-primary btn-xs" @click="createTableTemplate">Buat</button>
      <button class="btn btn-danger btn-xs" :disabled="!selectedTemplateName" @click="deleteTableTemplate">Hapus</button>
    </div>

    <div class="table-select-container">
      <div v-if="availableTables.length === 0" class="empty-tables-hint">
        <span>Belum ada daftar tabel. Klik <strong>"Ambil Tabel"</strong> untuk mendeteksi tabel dari database remote, atau ketik nama tabel secara manual.</span>
        <div class="manual-table-input">
          <input v-model="manualTableName" type="text" class="form-input form-input-sm" placeholder="Nama tabel manual (misal: users)" @keyup.enter="addManualTable" />
          <button class="btn btn-secondary btn-xs" @click="addManualTable">+ Tambah</button>
        </div>
      </div>

      <div v-else class="table-checkbox-grid">
        <label
          v-for="tableName in filteredTables"
          :key="tableName"
          class="table-checkbox-card"
          :class="{ selected: selectedTables.includes(tableName) }"
        >
          <input
            type="checkbox"
            :value="tableName"
            :checked="selectedTables.includes(tableName)"
            @change="toggleTableSelection(tableName)"
          />
          <div class="table-card-body">
            <span class="table-name-text" :title="tableName">{{ tableName }}</span>
            <div class="table-card-meta">
              <template v-if="getTableState(tableName)">
                <span class="meta-item">
                  <span class="meta-label">Last ID:</span>
                  <strong class="text-amber">{{ getTableState(tableName).lastSyncedId !== null && getTableState(tableName).lastSyncedId !== undefined ? getTableState(tableName).lastSyncedId : '-' }}</strong>
                </span>
                <span class="meta-item">
                  <span class="meta-label">Sync:</span>
                  <span class="text-muted">{{ formatDate(getTableState(tableName).lastSyncTime) }}</span>
                </span>
              </template>
              <span v-else class="meta-item text-dim">Belum pernah sync</span>
            </div>
          </div>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue';

const props = defineProps({
  selectedTables: { type: Array, default: () => [] },
  availableTables: { type: Array, default: () => [] },
  fetchingTables: { type: Boolean, default: false },
  syncTableStates: { type: Array, default: () => [] },
});

const emit = defineEmits([
  'update:selectedTables',
  'update:availableTables',
  'fetch-tables',
]);

const getTableState = (tableName) => {
  if (!props.syncTableStates || !Array.isArray(props.syncTableStates)) return null;
  return props.syncTableStates.find(
    (st) => st._table === tableName || st.table === tableName
  ) || null;
};

const formatDate = (isoString) => {
  if (!isoString) return '-';
  try {
    const d = new Date(isoString);
    return d.toLocaleString('id-ID', { dateStyle: 'short', timeStyle: 'short' });
  } catch {
    return isoString;
  }
};

const tableSearchQuery = ref('');
const manualTableName = ref('');
const selectedTemplateName = ref('');
const tableTemplates = ref({});

onMounted(() => {
  loadTableTemplateList();
});

const filteredTables = computed(() => {
  if (!tableSearchQuery.value) return props.availableTables;
  const q = tableSearchQuery.value.toLowerCase().trim();
  return props.availableTables.filter((t) => t.toLowerCase().includes(q));
});

const toggleTableSelection = (tableName) => {
  const current = [...props.selectedTables];
  const idx = current.indexOf(tableName);
  if (idx > -1) current.splice(idx, 1);
  else current.push(tableName);
  emit('update:selectedTables', current);
};

const selectAllTables = () => {
  const current = new Set(props.selectedTables);
  filteredTables.value.forEach((t) => current.add(t));
  emit('update:selectedTables', [...current]);
};

const deselectAllTables = () => {
  const filtered = new Set(filteredTables.value);
  emit('update:selectedTables', props.selectedTables.filter((t) => !filtered.has(t)));
};

const addManualTable = () => {
  const tName = manualTableName.value.trim();
  if (!tName) return;

  const currentAvailable = [...props.availableTables];
  if (!currentAvailable.includes(tName)) {
    currentAvailable.push(tName);
    emit('update:availableTables', currentAvailable);
  }

  const currentSelected = [...props.selectedTables];
  if (!currentSelected.includes(tName)) {
    currentSelected.push(tName);
    emit('update:selectedTables', currentSelected);
  }

  manualTableName.value = '';
};

const loadTableTemplateList = () => {
  try {
    const raw = localStorage.getItem('db_sync_table_templates');
    if (raw) tableTemplates.value = JSON.parse(raw);
  } catch (e) {
    console.error('Failed loading table templates:', e);
  }
};

const loadTableTemplate = () => {
  const name = selectedTemplateName.value;
  if (!name || !tableTemplates.value[name]) return;
  emit('update:selectedTables', [...tableTemplates.value[name]]);
};

const createTableTemplate = () => {
  const name = prompt('Nama template tabel baru (misal: "Core Tables" atau "HR Module")');
  if (!name || !name.trim()) return;

  const trimmed = name.trim();
  tableTemplates.value[trimmed] = [...props.selectedTables];
  localStorage.setItem('db_sync_table_templates', JSON.stringify(tableTemplates.value));
  selectedTemplateName.value = trimmed;
};

const updateTableTemplate = () => {
  const name = selectedTemplateName.value;
  if (!name) return;
  tableTemplates.value[name] = [...props.selectedTables];
  localStorage.setItem('db_sync_table_templates', JSON.stringify(tableTemplates.value));
};

const deleteTableTemplate = () => {
  const name = selectedTemplateName.value;
  if (!name || !tableTemplates.value[name]) return;
  if (!confirm(`Hapus template "${name}"? Tindakan ini tidak bisa dibatalkan.`)) return;
  delete tableTemplates.value[name];
  localStorage.setItem('db_sync_table_templates', JSON.stringify(tableTemplates.value));
  selectedTemplateName.value = '';
};
</script>

<style scoped>
.config-card {
  padding: 20px;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.title-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.title-wrap h3 {
  font-size: 1.05rem;
  font-weight: 600;
  margin: 0;
}

.selection-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  background: rgba(6, 182, 212, 0.12);
  border: 1px solid rgba(6, 182, 212, 0.3);
  color: #38bdf8;
  font-size: 0.78rem;
  font-weight: 500;
  padding: 2px 10px;
  border-radius: 12px;
}

.selection-badge strong {
  color: #38bdf8;
  font-weight: 700;
}

.badge-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #38bdf8;
  box-shadow: 0 0 6px #38bdf8;
}

.selection-info-pill {
  font-size: 0.76rem;
  color: var(--text-muted);
  background: rgba(15, 23, 42, 0.5);
  border: 1px solid rgba(255, 255, 255, 0.08);
  padding: 3px 8px;
  border-radius: 6px;
  white-space: nowrap;
}

.selection-info-pill strong {
  color: var(--accent-cyan);
}

.subtitle {
  font-size: 0.78rem;
  color: var(--text-muted);
  margin-top: 4px;
}

.table-tools-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
  flex-wrap: wrap;
}

.table-search-box {
  flex: 1;
  min-width: 160px;
}

.form-input-sm {
  padding: 6px 10px;
  font-size: 0.8rem;
}

.preset-select {
  padding: 0px 2px;
  font-size: 0.8rem;
  width: auto;
  max-width: 180px;
  color: #111827;
}

.preset-select option {
  color: #111827;
}

.btn-sm {
  padding: 6px 12px;
  font-size: 0.78rem;
}

.btn-xs {
  padding: 4px 8px;
  font-size: 0.72rem;
}

.btn-ghost {
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-main);
  border: 1px solid rgba(255, 255, 255, 0.08);
}

.tools-divider {
  width: 1px;
  height: 20px;
  background: rgba(255, 255, 255, 0.12);
  align-self: center;
  flex-shrink: 0;
}

.table-select-container {
  background: rgba(0, 0, 0, 0.25);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  padding: 12px;
  max-height: 280px;
  width: 100%;
  box-sizing: border-box;
  overflow-x: hidden;
  overflow-y: auto;
}

.empty-tables-hint {
  font-size: 0.8rem;
  color: var(--text-muted);
  text-align: center;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-items: center;
}

.manual-table-input {
  display: flex;
  gap: 8px;
  max-width: 320px;
  width: 100%;
}

.table-checkbox-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 8px;
}

.table-checkbox-card {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  padding: 10px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s ease;
  user-select: none;
}

.table-checkbox-card input[type="checkbox"] {
  margin-top: 3px;
  flex-shrink: 0;
}

.table-checkbox-card:hover {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.15);
}

.table-checkbox-card.selected {
  background: rgba(56, 189, 248, 0.12);
  border-color: rgba(56, 189, 248, 0.35);
}

.table-card-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  flex: 1;
}

.table-name-text {
  display: block;
  font-size: 0.82rem;
  font-weight: 600;
  font-family: 'JetBrains Mono', monospace;
  color: var(--text-main);
  word-break: break-word;
}

.table-card-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 0.69rem;
  flex-wrap: wrap;
}

.meta-item {
  display: flex;
  align-items: center;
  gap: 3px;
}

.meta-label {
  color: var(--text-dim);
  font-size: 0.65rem;
}

.text-amber { color: var(--accent-amber); }
.text-muted { color: var(--text-muted); }
.text-dim { color: var(--text-dim); }

.spin-sm {
  width: 12px;
  height: 12px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: white;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  100% { transform: rotate(360deg); }
}
</style>
