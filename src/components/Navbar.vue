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
        <h1 class="brand-title">DB-Sync <span class="badge-tag">v1.0</span></h1>
        <p class="brand-subtitle">PMA Remote &rarr; Local MySQL Port 3306</p>
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

    <div class="nav-actions">
      <button class="btn btn-primary" :disabled="isSyncing" @click="$emit('trigger-sync')">
        <svg v-if="!isSyncing" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21.5 2v6h-6M2.13 15.57a10 10 0 1 0 0-7.14l-1.63-1.63"></path>
        </svg>
        <svg v-else class="spin" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="12" y1="2" x2="12" y2="6"></line>
          <line x1="12" y1="18" x2="12" y2="22"></line>
          <line x1="4.93" y1="4.93" x2="7.76" y2="7.76"></line>
          <line x1="16.24" y1="16.24" x2="19.07" y2="19.07"></line>
          <line x1="2" y1="12" x2="6" y2="12"></line>
          <line x1="18" y1="12" x2="22" y2="12"></line>
          <line x1="4.93" y1="19.07" x2="7.76" y2="16.24"></line>
          <line x1="16.24" y1="7.76" x2="19.07" y2="4.93"></line>
        </svg>
        <span>{{ isSyncing ? 'Menyinkronkan...' : 'Sinkronkan Sekarang' }}</span>
      </button>
    </div>
  </header>
</template>

<script setup>
defineProps({
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
});

defineEmits(['trigger-sync']);
</script>

<style scoped>
.navbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 24px;
  background: rgba(15, 23, 42, 0.85);
  backdrop-filter: blur(12px);
  border-bottom: 1px solid var(--border-color);
}

.nav-brand {
  display: flex;
  align-items: center;
  gap: 12px;
}

.logo-icon {
  width: 40px;
  height: 40px;
  border-radius: var(--radius-md);
  background: linear-gradient(135deg, var(--primary) 0%, var(--accent-cyan) 100%);
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  box-shadow: var(--shadow-glow);
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
