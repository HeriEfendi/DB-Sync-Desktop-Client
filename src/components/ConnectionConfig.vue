<template>
  <div class="glass-panel config-card">
    <div class="card-header">
      <div class="title-wrap">
        <h3>Konfigurasi Database & Tabel</h3>
        <p class="subtitle">Atur kredensial, pilihan tabel yang akan disinkronkan, mode sync & limit row</p>
      </div>

      <!-- Presets Selector -->
      <div class="preset-controls">
        <select v-model="selectedPresetName" class="form-select preset-select" @change="loadPreset">
          <option value="">-- Pilih Profil Preset --</option>
          <option v-for="(p, key) in presets" :key="key" :value="key">{{ key }}</option>
        </select>
        <!-- Simpan: update preset yang sedang aktif -->
        <button
          class="btn btn-secondary btn-sm"
          title="Simpan perubahan ke preset yang dipilih"
          :disabled="!selectedPresetName"
          @click="updatePreset"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"></path><polyline points="17 21 17 13 7 13 7 21"></polyline><polyline points="7 3 7 8 15 8"></polyline></svg>
          Simpan
        </button>
        <!-- Buat: create preset baru dengan nama baru -->
        <button
          class="btn btn-primary btn-sm"
          title="Buat preset baru dari konfigurasi saat ini"
          @click="createPreset"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
          Buat
        </button>
        <!-- Hapus: hapus preset yang sedang aktif -->
        <button
          class="btn btn-danger btn-sm"
          title="Hapus preset yang dipilih"
          :disabled="!selectedPresetName"
          @click="deletePreset"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6l-1 14H6L5 6"></path><path d="M10 11v6"></path><path d="M14 11v6"></path><path d="M9 6V4h6v2"></path></svg>
          Hapus
        </button>
      </div>
    </div>

    <!-- Navigation Tabs -->
    <div class="tab-header">
      <button
        data-tab="remote"
        class="tab-btn"
        :class="{ active: activeTab === 'remote' }"
        @click="activeTab = 'remote'"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"></circle><line x1="2" y1="12" x2="22" y2="12"></line><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path></svg>
        <span>1. Remote PMA</span>
      </button>

      <button
        data-tab="local"
        class="tab-btn"
        :class="{ active: activeTab === 'local' }"
        @click="activeTab = 'local'"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="2" width="20" height="8" rx="2" ry="2"></rect><rect x="2" y="14" width="20" height="8" rx="2" ry="2"></rect><line x1="6" y1="6" x2="6.01" y2="6"></line><line x1="6" y1="18" x2="6.01" y2="18"></line></svg>
        <span>2. MySQL Lokal</span>
      </button>

      <button
        data-tab="tables"
        class="tab-btn"
        :class="{ active: activeTab === 'tables' }"
        @click="activeTab = 'tables'"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2H2v10h10V2zM22 2h-8v6h8V2zM22 12h-8v10h8V12zM12 16H2v6h10v-6z"></path></svg>
        <span>3. Tabel & Mode Sync</span>
        <span class="badge-tab" v-if="selectedTables.length > 0">{{ selectedTables.length }}</span>
      </button>
    </div>

    <!-- Tab 1: Remote PMA Credentials -->
    <div v-show="activeTab === 'remote'" class="tab-content">
      <div class="form-row">
        <div class="form-group flex-2">
          <label class="form-label">URL PhpMyAdmin Remote</label>
          <input
            v-model="pmaConfig.url"
            type="url"
            class="form-input"
            placeholder="https://server.domain.com/phpmyadmin"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>

        <div class="form-group flex-1">
          <label class="form-label">PMA Username</label>
          <input
            v-model="pmaConfig.username"
            type="text"
            class="form-input"
            placeholder="root / admin"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>

        <div class="form-group flex-1">
          <label class="form-label">PMA Password</label>
          <input
            v-model="pmaConfig.password"
            type="password"
            class="form-input"
            placeholder="••••••••"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>
      </div>

      <div class="form-row">
        <div class="form-group flex-2">
          <label class="form-label">Database Remote</label>
          <input
            v-model="pmaConfig.database"
            type="text"
            class="form-input"
            placeholder="my_remote_db"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>

        <div class="form-group flex-1">
          <label class="form-label">Default Primary Key</label>
          <input
            v-model="pmaConfig.primaryKey"
            type="text"
            class="form-input"
            placeholder="id"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>
      </div>

      <div class="action-bar">
        <button
          class="btn btn-secondary btn-sm"
          :disabled="testingPma"
          @click="$emit('test-pma')"
        >
          <span v-if="testingPma" class="spin-sm"></span>
          <span>Tes Koneksi PMA</span>
        </button>
      </div>
    </div>

    <!-- Tab 2: Local MySQL Credentials -->
    <div v-show="activeTab === 'local'" class="tab-content">
      <div class="form-row">
        <div class="form-group flex-2">
          <label class="form-label">Host Server MySQL</label>
          <input
            v-model="localConfig.host"
            type="text"
            class="form-input"
            placeholder="localhost / 127.0.0.1"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>

        <div class="form-group flex-1">
          <label class="form-label">Port TCP</label>
          <input
            v-model.number="localConfig.port"
            type="number"
            class="form-input"
            placeholder="3306"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>
      </div>

      <div class="form-row">
        <div class="form-group flex-1">
          <label class="form-label">Username DB Lokal</label>
          <input
            v-model="localConfig.username"
            type="text"
            class="form-input"
            placeholder="root"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>

        <div class="form-group flex-1">
          <label class="form-label">Password DB Lokal</label>
          <input
            v-model="localConfig.password"
            type="password"
            class="form-input"
            placeholder="Kosongkan jika tanpa password"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>

        <div class="form-group flex-1">
          <label class="form-label">Database Target Lokal</label>
          <input
            v-model="localConfig.database"
            type="text"
            class="form-input"
            placeholder="my_local_db"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>
      </div>

      <div class="action-bar">
        <button
          class="btn btn-secondary btn-sm"
          :disabled="testingLocal"
          @click="$emit('test-local')"
        >
          <span v-if="testingLocal" class="spin-sm"></span>
          <span>Tes Koneksi MySQL Rust</span>
        </button>
      </div>
    </div>

    <!-- Tab 3: Multi-Table Selection, Sync Mode & Row Limit -->
    <div v-show="activeTab === 'tables'" class="tab-content">
      <!-- Template Tabel Selector + Tools: 1 baris compact -->
      <div class="table-tools-bar">
        <!-- Fetch + Search -->
        <button class="btn btn-primary btn-sm" :disabled="fetchingTables" @click="$emit('fetch-tables')">
          <span v-if="fetchingTables" class="spin-sm"></span>
          <svg v-else xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67"/></svg>
          <span>Ambil Tabel</span>
        </button>

        <div class="table-search-box">
          <input v-model="tableSearchQuery" type="text" class="form-input form-input-sm" placeholder="Cari tabel..." />
        </div>

        <!-- Bulk actions -->
        <button class="btn btn-ghost btn-xs" @click="selectAllTables">Centang Semua</button>
        <button class="btn btn-ghost btn-xs" @click="deselectAllTables">Hapus Centang</button>

        <div class="tools-divider"></div>
        <!-- Template selector & actions -->
        <select v-model="selectedTemplateName" class="form-select preset-select" @change="loadTableTemplate" style="max-width:160px">
          <option value="">-- Template Tabel --</option>
          <option v-for="(t, key) in tableTemplates" :key="key" :value="key">{{ key }}</option>
        </select>
        <button class="btn btn-secondary btn-xs" title="Simpan centangan ke template aktif" :disabled="!selectedTemplateName" @click="updateTableTemplate">
          <svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"></path><polyline points="17 21 17 13 7 13 7 21"></polyline><polyline points="7 3 7 8 15 8"></polyline></svg>
          Simpan
        </button>
        <button class="btn btn-primary btn-xs" title="Buat template baru dari centangan saat ini" @click="createTableTemplate">
          <svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
          Buat
        </button>
        <button class="btn btn-danger btn-xs" title="Hapus template yang dipilih" :disabled="!selectedTemplateName" @click="deleteTableTemplate">
          <svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6l-1 14H6L5 6"></path><path d="M10 11v6"></path><path d="M14 11v6"></path><path d="M9 6V4h6v2"></path></svg>
          Hapus
        </button>

      </div>

      <!-- Table Selection Grid -->
      <div class="table-select-container">
        <div v-if="availableTables.length === 0" class="empty-tables-hint">
          <span>Belum ada daftar tabel. Klik <strong>"Ambil List Tabel Server"</strong> untuk mendeteksi seluruh tabel dari database remote, atau ketik nama tabel secara manual.</span>
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

      <!-- Sync Options Bar (Sync Mode & Limit Rows) -->
      <div class="sync-options-card">
        <div class="options-row">
          <div class="option-inline">
            <span class="option-title">Mode:</span>
            <label class="radio-label-inline" title="Tarik ID baru yang belum ada di lokal, dan update baris jika updated_at di server lebih baru">
              <input type="radio" name="syncMode" value="incremental" :checked="syncMode === 'incremental'" @change="$emit('update:syncMode', 'incremental')" />
              <span>Sync (New &amp; Update)</span>
            </label>
            <label class="radio-label-inline warning-radio">
              <input type="radio" name="syncMode" value="fresh" :checked="syncMode === 'fresh'" @change="$emit('update:syncMode', 'fresh')" />
              <span>Fresh Sync</span>
            </label>
          </div>

          <div class="option-inline">
            <span class="option-title">Limit:</span>
            <select
              :value="rowLimit"
              class="form-select limit-select"
              @change="$emit('update:rowLimit', parseInt($event.target.value, 10))"
            >
              <option :value="0">Semua Row (Default)</option>
              <option :value="1000">1.000 Row</option>
              <option :value="10000">10.000 Row</option>
              <option :value="100000">100.000 Row</option>
              <option :value="500000">500.000 Row</option>
              <option :value="2000000">2.000.000 Row</option>
            </select>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue';

const props = defineProps({
  pmaConfig: { type: Object, required: true },
  localConfig: { type: Object, required: true },
  selectedTables: { type: Array, default: () => [] },
  availableTables: { type: Array, default: () => [] },
  syncMode: { type: String, default: 'incremental' },
  rowLimit: { type: Number, default: 0 },
  testingPma: { type: Boolean, default: false },
  testingLocal: { type: Boolean, default: false },
  fetchingTables: { type: Boolean, default: false },
});

const emit = defineEmits([
  'update:pmaConfig',
  'update:localConfig',
  'update:selectedTables',
  'update:availableTables',
  'update:syncMode',
  'update:rowLimit',
  'test-pma',
  'test-local',
  'fetch-tables',
  'preset-changed',
]);

const activeTab = ref('remote');
const selectedPresetName = ref('');
const presets = ref({});
const tableSearchQuery = ref('');
const manualTableName = ref('');
const selectedTemplateName = ref('');
const tableTemplates = ref({});

onMounted(() => {
  loadPresetList();
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
  if (idx > -1) {
    current.splice(idx, 1);
  } else {
    current.push(tableName);
  }
  emit('update:selectedTables', current);
};

const selectAllTables = () => {
  // Hanya tambahkan tabel yang sedang tampil (filteredTables) ke seleksi,
  // tabel yang tidak masuk filter tetap tidak berubah.
  const current = new Set(props.selectedTables);
  filteredTables.value.forEach((t) => current.add(t));
  emit('update:selectedTables', [...current]);
};

const deselectAllTables = () => {
  // Hanya hapus tabel yang sedang tampil (filteredTables) dari seleksi,
  // tabel yang tidak masuk filter tetap terjaga.
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

const loadPresetList = () => {
  try {
    const raw = localStorage.getItem('db_sync_presets');
    if (raw) presets.value = JSON.parse(raw);
  } catch (e) {
    console.error('Failed loading presets:', e);
  }
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
  const name = prompt('Nama template tabel baru (misal: "Core Tables" atau "HR Module"):');
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

const createPreset = () => {
  const name = prompt('Nama preset baru (misal: "Production Core Sync"):');
  if (!name || !name.trim()) return;
  const trimmed = name.trim();

  presets.value[trimmed] = {
    pmaConfig: { ...props.pmaConfig },
    localConfig: { ...props.localConfig },
    selectedTables: [...props.selectedTables],
    availableTables: [...props.availableTables],
    syncMode: props.syncMode,
    rowLimit: props.rowLimit,
  };

  localStorage.setItem('db_sync_presets', JSON.stringify(presets.value));
  selectedPresetName.value = trimmed;
};

const updatePreset = () => {
  const name = selectedPresetName.value;
  if (!name || !presets.value[name]) return;

  presets.value[name] = {
    pmaConfig: { ...props.pmaConfig },
    localConfig: { ...props.localConfig },
    selectedTables: [...props.selectedTables],
    availableTables: [...props.availableTables],
    syncMode: props.syncMode,
    rowLimit: props.rowLimit,
  };

  localStorage.setItem('db_sync_presets', JSON.stringify(presets.value));
};

const deletePreset = () => {
  const name = selectedPresetName.value;
  if (!name || !presets.value[name]) return;
  if (!confirm(`Hapus preset "${name}"? Tindakan ini tidak bisa dibatalkan.`)) return;

  delete presets.value[name];
  localStorage.setItem('db_sync_presets', JSON.stringify(presets.value));
  selectedPresetName.value = '';
  // Reset semua field setelah hapus
  emit('preset-changed');
  emit('update:pmaConfig', { url: '', username: '', password: '', database: '' });
  emit('update:localConfig', { host: '', port: 3306, username: '', password: '', database: '' });
  emit('update:availableTables', []);
  emit('update:selectedTables', []);
  emit('update:syncMode', 'incremental');
  emit('update:rowLimit', 0);
};

const loadPreset = () => {
  const name = selectedPresetName.value;

  // Reset status koneksi setiap kali preset berganti
  emit('preset-changed');

  // Jika kembali ke pilihan kosong — reset semua field
  if (!name) {
    emit('update:pmaConfig', { url: '', username: '', password: '', database: '' });
    emit('update:localConfig', { host: '', port: 3306, username: '', password: '', database: '' });
    emit('update:availableTables', []);
    emit('update:selectedTables', []);
    emit('update:syncMode', 'incremental');
    emit('update:rowLimit', 0);
    return;
  }

  if (!presets.value[name]) return;

  const p = presets.value[name];
  if (p.pmaConfig) {
    emit('update:pmaConfig', { ...p.pmaConfig });
  }
  if (p.localConfig) {
    emit('update:localConfig', { ...p.localConfig });
  }
  if (p.availableTables) {
    emit('update:availableTables', p.availableTables);
  }
  if (p.selectedTables) {
    emit('update:selectedTables', p.selectedTables);
  }
  if (p.syncMode) {
    emit('update:syncMode', p.syncMode);
  }
  if (p.rowLimit !== undefined) {
    emit('update:rowLimit', p.rowLimit);
  }
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

.form-label {
  color: #ffffff !important;
  font-weight: 600;
}

.config-card input.form-input,
.config-card select.form-select {
  color: #ffffff !important;
  -webkit-text-fill-color: #ffffff !important;
  background: #111827 !important;
  padding: 8px 10px;
  font-size: 0.78rem;
}

.config-card input.form-input::placeholder {
  color: #cbd5e1 !important;
  opacity: 1 !important;
}

.config-card input.form-input:-webkit-autofill,
.config-card input.form-input:-webkit-autofill:hover,
.config-card input.form-input:-webkit-autofill:focus {
  -webkit-text-fill-color: #ffffff !important;
  box-shadow: 0 0 0 1000px #111827 inset !important;
}

.form-select option {
  color: #ffffff;
  background: #111827;
}

.subtitle {
  font-size: 0.78rem;
  color: var(--text-muted);
}

.preset-controls {
  display: flex;
  align-items: center;
  gap: 8px;
}

select.preset-select {
  box-sizing: border-box;
  width: 100%;
  max-width: 180px;
  min-height: 34px;
  padding: 8px 10px;
  background: var(--bg-input) !important;
  border: 1px solid var(--border-color) !important;
  border-radius: var(--radius-sm);
  color: var(--text-main) !important;
  -webkit-text-fill-color: var(--text-main) !important;
  font-family: inherit;
  font-size: 0.78rem;
  font-weight: 400;
  line-height: normal;
  color-scheme: dark;
}

select.preset-select option {
  color: var(--text-main) !important;
  background: var(--bg-input) !important;
  font: inherit;
}

.btn-sm {
  padding: 6px 12px;
  font-size: 0.78rem;
}

.btn-xs {
  padding: 4px 8px;
  font-size: 0.72rem;
}

.badge-tab {
  background: var(--accent-cyan);
  color: #0d1117;
  font-weight: 700;
  font-size: 0.7rem;
  padding: 1px 6px;
  border-radius: 10px;
}

.form-row {
  display: flex;
  gap: 12px;
}

.flex-1 { flex: 1; }
.flex-2 { flex: 2; }

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

.table-bulk-actions {
  display: flex;
  gap: 6px;
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
  max-height: 200px;
  width: 100%;
  box-sizing: border-box;
  overflow-x: hidden;
  overflow-y: auto;
  margin-bottom: 16px;
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

.sync-options-card {
  background: rgba(0, 0, 0, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  padding: 10px 14px;
}

.options-row {
  display: flex;
  align-items: center;
  gap: 20px;
  flex-wrap: wrap;
}

.option-inline {
  display: flex;
  align-items: center;
  gap: 8px;
}

.radio-label-inline {
  display: flex;
  align-items: center;
  gap: 5px;
  cursor: pointer;
  font-size: 0.82rem;
  color: var(--text-main);
  padding: 4px 8px;
  border-radius: 5px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(255, 255, 255, 0.03);
  white-space: nowrap;
}

.radio-label-inline.warning-radio {
  border-color: rgba(245, 158, 11, 0.2);
}

.option-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.option-title {
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.radio-group {
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
}

.radio-label {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  cursor: pointer;
  background: rgba(255, 255, 255, 0.03);
  padding: 8px 12px;
  border-radius: 6px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  flex: 1;
  min-width: 240px;
}

.warning-radio {
  border-color: rgba(245, 158, 11, 0.2);
}

.radio-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.radio-text strong {
  font-size: 0.82rem;
  color: var(--text-main);
}

.radio-text small {
  font-size: 0.73rem;
  color: var(--text-muted);
  line-height: 1.3;
}

.limit-controls {
  display: flex;
  align-items: center;
  gap: 12px;
}

.limit-select {
  padding: 6px 12px;
  font-size: 0.8rem;
  width: auto;
  max-width: 260px;
  color: #111827;
}

.limit-select option {
  color: #111827;
}

.limit-hint {
  font-size: 0.78rem;
  color: var(--text-muted);
}

.action-bar {
  display: flex;
  justify-content: flex-end;
  margin-top: 12px;
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
