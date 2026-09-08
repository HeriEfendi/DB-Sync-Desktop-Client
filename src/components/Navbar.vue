<template>
  <header class="navbar">
    <div class="nav-brand">
      <div class="logo-icon">
        <svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <ellipse cx="12" cy="5" rx="9" ry="3"></ellipse>
          <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"></path>
          <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"></path>
        </svg>
      </div>
      <div class="brand-text">
        <h1 class="brand-title">DB-Sync <span class="badge-tag">v{{ displayVersion }}</span></h1>
        <p class="brand-subtitle">PMA Remote &rarr; Local MySQL Port</p>
      </div>
    </div>

    <div class="nav-status">
      <!-- Remote PMA Badge -->
      <div class="status-pill" :class="pmaStatus.connected ? 'pill-success' : 'pill-muted'">
        <span class="dot" :class="{ pulse: pmaStatus.connected }"></span>
        <span class="label">PMA:</span>
        <span class="value">{{ pmaStatus.connected ? 'Terhubung' : 'Terputus' }}</span>
      </div>

      <!-- Local MySQL Badge -->
      <div class="status-pill" :class="localStatus.connected ? 'pill-success' : 'pill-muted'">
        <span class="dot" :class="{ pulse: localStatus.connected }"></span>
        <span class="label">MySQL Lokal:</span>
        <span class="value">{{ localStatus.connected ? 'Terhubung' : 'Terputus' }}</span>
      </div>
    </div>
  </header>
</template>

<script setup>
import { computed } from 'vue';
import packageJson from '../../package.json';

const props = defineProps({
  pmaStatus: {
    type: Object,
    default: () => ({ connected: false }),
  },
  localStatus: {
    type: Object,
    default: () => ({ connected: false }),
  },
  isSyncing: {
    type: Boolean,
    default: false,
  },
  appVersion: {
    type: String,
    default: '',
  },
});

const displayVersion = computed(() => props.appVersion || packageJson.version);

defineEmits(['trigger-sync']);
</script>

<style scoped>
.navbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 24px;
  background: #11141b;
  border-bottom: 1px solid var(--border-color);
  transform: translateZ(0);
  z-index: 10;
}

.nav-brand {
  display: flex;
  align-items: center;
  gap: 12px;
}

.logo-icon {
  width: 40px;
  height: 40px;
  border-radius: 12px;
  background: linear-gradient(135deg, #6b74ff 0%, #42c3ec 100%);
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  box-shadow: 0 4px 14px rgba(107, 116, 255, 0.3);
}

.brand-title {
  font-size: 1.1rem;
  font-weight: 700;
  color: var(--text-main);
  display: flex;
  align-items: center;
  gap: 8px;
}

.badge-tag {
  font-size: 0.7rem;
  padding: 2px 6px;
  background: rgba(99, 102, 241, 0.2);
  color: var(--primary);
  border-radius: 4px;
}

.brand-subtitle {
  font-size: 0.76rem;
  color: var(--text-muted);
}

.nav-status {
  display: flex;
  align-items: center;
  gap: 12px;
}

.status-pill {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border-radius: 20px;
  font-size: 0.78rem;
  background: rgba(30, 41, 59, 0.8);
  border: 1px solid var(--border-color);
}

.pill-success {
  border-color: rgba(16, 185, 129, 0.4);
  color: var(--accent-emerald);
}

.pill-muted {
  color: var(--text-dim);
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
}

.dot.pulse {
  animation: pulse-dot 1.5s infinite ease-in-out;
}

.label {
  color: var(--text-muted);
}

.value {
  font-weight: 600;
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  100% { transform: rotate(360deg); }
}
</style>
