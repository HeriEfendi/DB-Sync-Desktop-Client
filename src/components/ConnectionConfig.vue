<template>
  <div class="glass-panel config-card">
    <div class="card-header">
      <div class="title-wrap">
        <h3>Konfigurasi Database</h3>
        <p class="subtitle">Atur kredensial PhpMyAdmin Remote dan database MySQL lokal</p>
      </div>

      <!-- Presets Selector -->
      <div class="preset-controls">
        <select v-model="selectedPresetName" class="form-select preset-select" @change="loadPreset">
          <option value="">-- Pilih Profil Preset --</option>
          <option v-for="(p, key) in presets" :key="key" :value="key">{{ key }}</option>
        </select>
        <button class="btn btn-secondary btn-sm" title="Simpan Preset Saat Ini" @click="savePreset">
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"></path><polyline points="17 21 17 13 7 13 7 21"></polyline><polyline points="7 3 7 8 15 8"></polyline></svg>
        </button>
      </div>
    </div>

    <!-- Navigation Tabs -->
    <div class="tab-header">
      <button
        class="tab-btn"
        :class="{ active: activeTab === 'remote' }"
        @click="activeTab = 'remote'"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"></circle><line x1="2" y1="12" x2="22" y2="12"></line><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path></svg>
        <span>Remote PhpMyAdmin</span>
      </button>

      <button
        class="tab-btn"
        :class="{ active: activeTab === 'local' }"
        @click="activeTab = 'local'"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="2" width="20" height="8" rx="2" ry="2"></rect><rect x="2" y="14" width="20" height="8" rx="2" ry="2"></rect><line x1="6" y1="6" x2="6.01" y2="6"></line><line x1="6" y1="18" x2="6.01" y2="18"></line></svg>
        <span>MySQL Lokal (3306)</span>
      </button>
    </div>

    <!-- Tab 1: Remote PMA Credentials -->
    <div v-show="activeTab === 'remote'" class="tab-content">
      <div class="form-group">
        <label class="form-label">URL PhpMyAdmin Remote</label>
        <input
          v-model="pmaConfig.url"
          type="url"
          class="form-input"
          placeholder="https://server.domain.com/phpmyadmin"
          @change="$emit('update:pmaConfig', pmaConfig)"
        />
      </div>

      <div class="form-row">
        <div class="form-group">
          <label class="form-label">PMA Username</label>
          <input
            v-model="pmaConfig.username"
            type="text"
            class="form-input"
            placeholder="root / admin"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>

        <div class="form-group">
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
        <div class="form-group">
          <label class="form-label">Database Remote</label>
          <input
            v-model="pmaConfig.database"
            type="text"
            class="form-input"
            placeholder="my_remote_db"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>

        <div class="form-group">
          <label class="form-label">Tabel Remote</label>
          <input
            v-model="pmaConfig.table"
            type="text"
            class="form-input"
            placeholder="users / orders"
            @change="$emit('update:pmaConfig', pmaConfig)"
          />
        </div>

        <div class="form-group">
          <label class="form-label">Primary Key / Incremental Col</label>
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
        <div class="form-group">
          <label class="form-label">Username DB Lokal</label>
          <input
            v-model="localConfig.username"
            type="text"
            class="form-input"
            placeholder="root"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>

        <div class="form-group">
          <label class="form-label">Password DB Lokal</label>
          <input
            v-model="localConfig.password"
            type="password"
            class="form-input"
            placeholder="Kosongkan jika tanpa password"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>
      </div>

      <div class="form-row">
        <div class="form-group">
          <label class="form-label">Database Target Lokal</label>
          <input
            v-model="localConfig.database"
            type="text"
            class="form-input"
            placeholder="my_local_db"
            @change="$emit('update:localConfig', localConfig)"
          />
        </div>

        <div class="form-group">
          <label class="form-label">Tabel Target Lokal</label>
          <input
            v-model="localConfig.table"
            type="text"
            class="form-input"
            placeholder="Sama dengan tabel remote"
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
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue';

const props = defineProps({
  pmaConfig: { type: Object, required: true },
  localConfig: { type: Object, required: true },
  testingPma: { type: Boolean, default: false },
  testingLocal: { type: Boolean, default: false },
});

const emit = defineEmits(['update:pmaConfig', 'update:localConfig', 'test-pma', 'test-local']);

const activeTab = ref('remote');
const selectedPresetName = ref('');
const presets = ref({});

onMounted(() => {
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

const savePreset = () => {
  const name = prompt('Masukkan nama profil preset (misal: "Production Sync" atau "Dev Sync"):');
  if (!name || !name.trim()) return;

  presets.value[name.trim()] = {
    pmaConfig: { ...props.pmaConfig },
    localConfig: { ...props.localConfig },
  };

  localStorage.setItem('db_sync_presets', JSON.stringify(presets.value));
  selectedPresetName.value = name.trim();
};

const loadPreset = () => {
  const name = selectedPresetName.value;
  if (!name || !presets.value[name]) return;

  const p = presets.value[name];
  if (p.pmaConfig) {
    Object.assign(props.pmaConfig, p.pmaConfig);
    emit('update:pmaConfig', props.pmaConfig);
  }
  if (p.localConfig) {
    Object.assign(props.localConfig, p.localConfig);
    emit('update:localConfig', props.localConfig);
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
  padding: 5px 10px;
  font-size: 0.8rem;
  width: auto;
  max-width: 180px;
}

.btn-sm {
  padding: 6px 12px;
  font-size: 0.78rem;
}

.form-row {
  display: flex;
  gap: 12px;
}

.flex-1 { flex: 1; }
.flex-2 { flex: 2; }

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
