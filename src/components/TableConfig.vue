<template>
  <div class="glass-panel config-card">
    <div class="card-header">
      <div class="title-wrap">
        <h3>Daftar Tabel</h3>
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
          <span class="table-name-text" :title="tableName">{{ tableName }}</span>
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
});

const emit = defineEmits([
  'update:selectedTables',
  'update:availableTables',
  'fetch-tables',
]);

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

.title-wrap h3 {
  font-size: 1.05rem;
  font-weight: 600;
}

.subtitle {
  font-size: 0.78rem;
  color: var(--text-muted);
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
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}

.table-checkbox-card {
  display: flex;
  align-items: center;
  gap: 8px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  padding: 8px 10px;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s ease;
  user-select: none;
}

.table-checkbox-card:hover {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.15);
}

.table-checkbox-card.selected {
  background: rgba(56, 189, 248, 0.12);
  border-color: rgba(56, 189, 248, 0.35);
}

.table-name-text {
  display: block;
  font-size: 0.82rem;
  font-family: 'JetBrains Mono', monospace;
  color: var(--text-main);
  white-space: normal;
  overflow: visible;
  text-overflow: clip;
  word-break: break-word;
}

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
