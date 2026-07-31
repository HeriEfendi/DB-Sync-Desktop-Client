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
    if (cmd === 'get_local_table_preview') {
      return [];
    }
    if (cmd === 'sync_to_local_db') {
      throw new Error(
        'Sinkronisasi ke MySQL lokal port 3306 memerlukan runtime Tauri. Jalankan `npm run tauri dev` pada terminal.'
      );
    }
    throw new Error('Fitur ini memerlukan runtime desktop Tauri.');
  }

  const { invoke } = await import('@tauri-apps/api/core');
  return await invoke(cmd, args);
}

export async function safeFetch(url, options = {}) {
  if (isTauriEnvironment()) {
    try {
      const { fetch: tauriFetch } = await import('@tauri-apps/plugin-http');
      return await tauriFetch(url, options);
    } catch (e) {
      console.warn('Tauri HTTP fetch error:', e);
      throw e;
    }
  }
  return await fetch(url, options);
}
