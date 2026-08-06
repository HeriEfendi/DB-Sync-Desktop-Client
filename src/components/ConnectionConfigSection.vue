<template>
  <div class="glass-panel config-card">
    <div class="card-header">
      <div class="title-wrap">
        <h3>Konfigurasi Database</h3>
        <p class="subtitle">Atur kredensial remote PMA dan MySQL lokal untuk sinkronisasi.</p>
      </div>

      <div class="preset-controls">
        <select v-model="selectedPresetName" class="form-select preset-select" @change="loadPreset" style="background-color: #111827 !important; color: #ffffff !important; color-scheme: dark;">
          <option value="">-- Pilih Profil Preset --</option>
          <option v-for="(p, key) in presets" :key="key" :value="key">{{ key }}</option>
        </select>

        <button class="btn btn-secondary btn-sm" title="Simpan perubahan ke preset yang dipilih" :disabled="!selectedPresetName" @click="updatePreset">
          Simpan
        </button>
        <button class="btn btn-primary btn-sm" title="Buat preset baru dari konfigurasi saat ini" @click="createPreset">
          Buat
        </button>
        <button class="btn btn-danger btn-sm" title="Hapus preset yang dipilih" :disabled="!selectedPresetName" @click="deletePreset">
          Hapus
        </button>
      </div>
    </div>

    <div class="tab-header">
      <button data-tab="remote" class="tab-btn" :class="{ active: activeTab === 'remote' }" @click="activeTab = 'remote'">
        <span>1. Remote PMA</span>
      </button>
      <button data-tab="local" class="tab-btn" :class="{ active: activeTab === 'local' }" @click="activeTab = 'local'">
        <span>2. MySQL Lokal</span>
      </button>
    </div>

    <div v-show="activeTab === 'remote'" class="tab-content">
      <div class="form-row">
        <div class="form-group flex-2">
          <label class="form-label">URL PhpMyAdmin Remote</label>
          <input v-model="pmaConfig.url" type="url" class="form-input" placeholder="https://server.domain.com/phpmyadmin" @change="$emit('update:pma-config', pmaConfig)" />
        </div>
        <div class="form-group flex-1">
          <label class="form-label">PMA Username</label>
          <input v-model="pmaConfig.username" type="text" class="form-input" placeholder="root / admin" @change="$emit('update:pma-config', pmaConfig)" />
        </div>
        <div class="form-group flex-1">
          <label class="form-label">PMA Password</label>
          <input v-model="pmaConfig.password" type="password" class="form-input" placeholder="••••••••" @change="$emit('update:pma-config', pmaConfig)" />
        </div>
      </div>

      <div class="form-row">
        <div class="form-group flex-2">
          <label class="form-label">Database Remote</label>
          <input v-model="pmaConfig.database" type="text" class="form-input" placeholder="my_remote_db" @change="$emit('update:pma-config', pmaConfig)" />
        </div>
        <div class="form-group flex-1">
          <label class="form-label">Default Primary Key</label>
          <input v-model="pmaConfig.primaryKey" type="text" class="form-input" placeholder="id" @change="$emit('update:pma-config', pmaConfig)" />
        </div>
      </div>

      <div class="action-bar">
        <button class="btn btn-secondary btn-sm" :disabled="testingPma" @click="$emit('test-pma')">
          <span v-if="testingPma" class="spin-sm"></span>
          <span>Tes Koneksi PMA</span>
        </button>
      </div>
    </div>

    <div v-show="activeTab === 'local'" class="tab-content">
      <div class="form-row">
        <div class="form-group flex-2">
          <label class="form-label">Host Server MySQL</label>
          <input v-model="localConfig.host" type="text" class="form-input" placeholder="localhost / 127.0.0.1" @change="$emit('update:local-config', localConfig)" />
        </div>
        <div class="form-group flex-1">
          <label class="form-label">Port TCP</label>
          <input v-model.number="localConfig.port" type="number" class="form-input" placeholder="3306" @change="$emit('update:local-config', localConfig)" />
        </div>
      </div>

      <div class="form-row">
        <div class="form-group flex-1">
          <label class="form-label">Username DB Lokal</label>
          <input v-model="localConfig.username" type="text" class="form-input" placeholder="root" @change="$emit('update:local-config', localConfig)" />
        </div>
        <div class="form-group flex-1">
          <label class="form-label">Password DB Lokal</label>
          <input v-model="localConfig.password" type="password" class="form-input" placeholder="Kosongkan jika tanpa password" @change="$emit('update:local-config', localConfig)" />
        </div>
        <div class="form-group flex-1">
          <label class="form-label">Database Target Lokal</label>
          <input v-model="localConfig.database" type="text" class="form-input" placeholder="my_local_db" @change="$emit('update:local-config', localConfig)" />
        </div>
      </div>

      <div class="action-bar">
        <button class="btn btn-secondary btn-sm" :disabled="testingLocal" @click="$emit('test-local')">
          <span v-if="testingLocal" class="spin-sm"></span>
          <span>Tes Koneksi MySQL Rust</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, watch, nextTick } from 'vue';

const props = defineProps({
  pmaConfig: { type: Object, required: true },
  localConfig: { type: Object, required: true },
  testingPma: { type: Boolean, default: false },
  testingLocal: { type: Boolean, default: false },
});

const emit = defineEmits([
  'update:pma-config',
  'update:local-config',
  'test-pma',
  'test-local',
  'preset-changed',
]);

const activeTab = ref('remote');
const selectedPresetName = ref('');
const presets = ref({});

const pmaConfig = ref({ ...props.pmaConfig });
const localConfig = ref({ ...props.localConfig });

watch(() => props.pmaConfig, (val) => {
  pmaConfig.value = { ...val };
}, { deep: true });

watch(() => props.localConfig, (val) => {
  localConfig.value = { ...val };
}, { deep: true });

onMounted(() => {
  selectedPresetName.value = '';
  loadPresetList();
});

const loadPresetList = () => {
  try {
    const raw = localStorage.getItem('db_sync_presets');
    if (raw) presets.value = JSON.parse(raw);
  } catch (e) {
    console.error('Failed loading presets:', e);
  }
};

const createPreset = () => {
  const name = prompt('Nama preset baru (misal: "Production Core Sync")');
  if (!name || !name.trim()) return;
  const trimmed = name.trim();

  presets.value[trimmed] = {
    pmaConfig: { ...props.pmaConfig },
    localConfig: { ...props.localConfig },
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
  emit('preset-changed');
  emit('update:pma-config', { url: '', username: '', password: '', database: '' });
  emit('update:local-config', { host: '', port: 3306, username: '', password: '', database: '' });
};

const loadPreset = async () => {
  const name = selectedPresetName.value;
  emit('preset-changed');

  if (!name) {
    emit('update:pma-config', { url: '', username: '', password: '', database: '' });
    emit('update:local-config', { host: '', port: 3306, username: '', password: '', database: '' });
  } else if (presets.value[name]) {
    const p = presets.value[name];
    if (p.pmaConfig) emit('update:pma-config', { ...p.pmaConfig });
    if (p.localConfig) emit('update:local-config', { ...p.localConfig });
  }

  await nextTick();
  emit('test-pma');
  emit('test-local');
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

.preset-controls {
  display: flex;
  align-items: center;
  gap: 8px;
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

.tab-header {
  display: flex;
  gap: 3px;
  padding: 3px;
  background: #101217;
  border-radius: 6px;
  margin-bottom: 16px;
}

.tab-btn {
  flex: 1;
  padding: 8px 10px;
  background: transparent;
  border: 0;
  border-radius: 4px;
  color: var(--text-muted);
  font-size: 0.72rem;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
}

.tab-btn.active {
  background: #252a34;
  color: var(--text-main);
}

.form-row {
  display: flex;
  gap: 12px;
}

.flex-1 { flex: 1; }
.flex-2 { flex: 2; }

.form-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 14px;
}

.form-label {
  font-size: 0.72rem;
  font-weight: 550;
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
