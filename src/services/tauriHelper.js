import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { fetch as tauriFetch } from '@tauri-apps/plugin-http';

/**
 * Helper utility to detect Tauri environment and execute safe IPC invokes and HTTP fetches
 */

export function isTauriEnvironment() {
  return (
    typeof window !== 'undefined' &&
    (window.__TAURI_INTERNALS__ !== undefined ||
      window.__TAURI_METADATA__ !== undefined ||
      window.__TAURI__ !== undefined)
  );
}

export async function safeInvoke(cmd, args = {}) {
  if (!isTauriEnvironment()) {
    console.warn(`[Browser Mode] Tauri invoke command '${cmd}' dipanggil di browser biasa.`);

    if (cmd === 'test_local_connection') {
      throw new Error(
        'Koneksi port 3306 MySQL lokal memerlukan backend Rust Tauri. Harap jalankan aplikasi dalam mode desktop menggunakan perintah: npm run tauri dev'
      );
    }
    if (cmd === 'get_last_local_id') {
      return null;
    }
    if (cmd === 'get_all_tables_last_local_ids') {
      return {};
    }
    if (cmd === 'batch_cleanup_incremental') {
      return {};
    }
    throw new Error('Fitur ini memerlukan runtime desktop Tauri.');
  }

  return await invoke(cmd, args);
}

export async function safeFetch(url, options = {}) {
  if (isTauriEnvironment()) {
    try {
      return await tauriFetch(url, options);
    } catch (e) {
      console.warn('Tauri HTTP fetch error:', e);
      throw e;
    }
  }
  return await fetch(url, options);
}

export async function safeListen(eventName, handler) {
  if (!isTauriEnvironment()) {
    return () => {};
  }
  try {
    return await listen(eventName, handler);
  } catch (e) {
    console.warn(`[Tauri Event] Gagal mendaftarkan listener '${eventName}':`, e);
    return () => {};
  }
}

