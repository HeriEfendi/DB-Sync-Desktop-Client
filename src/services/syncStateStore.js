/**
 * syncStateStore.js
 * 
 * Menyimpan dan membaca state sinkronisasi terakhir per tabel.
 * Key format: db_sync_tstate_{serverHost}_{database}_{tableName}
 * 
 * Value format:
 * {
 *   lastSyncedId: number|string|null,
 *   lastSyncTime: string (ISO),
 *   lastSyncedUpdatedAt: string|null,
 *   rowsSynced: number
 * }
 */

const STORE_PREFIX = 'db_sync_tstate_';

/**
 * Normalize a server URL/host into a safe key segment.
 * e.g. "https://dbm.olshoperp.com/phpmyadmin" → "dbm.olshoperp.com"
 */
function normalizeHost(urlOrHost) {
  if (!urlOrHost) return 'unknown';
  try {
    const u = new URL(urlOrHost);
    return u.hostname.replace(/[^a-zA-Z0-9.-]/g, '_');
  } catch {
    // Not a URL, treat as raw host
    return urlOrHost.replace(/[^a-zA-Z0-9.-]/g, '_');
  }
}

function sanitizeSegment(s) {
  return (s || 'unknown').replace(/[^a-zA-Z0-9_.-]/g, '_');
}

/**
 * Build a unique localStorage key for a specific table sync state.
 */
export function buildKey(serverHost, database, tableName) {
  const host = normalizeHost(serverHost);
  const db = sanitizeSegment(database);
  const tbl = sanitizeSegment(tableName);
  return `${STORE_PREFIX}${host}_${db}_${tbl}`;
}

/**
 * Get the sync state for a specific table.
 * @returns {{ lastSyncedId: any, lastSyncTime: string, lastSyncedUpdatedAt: string|null, rowsSynced: number } | null}
 */
export function getTableState(serverHost, database, tableName) {
  try {
    const key = buildKey(serverHost, database, tableName);
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

/**
 * Save sync state for a specific table.
 */
export function saveTableState(serverHost, database, tableName, state) {
  try {
    const key = buildKey(serverHost, database, tableName);
    const existing = getTableState(serverHost, database, tableName) || {};
    const merged = {
      ...existing,
      ...state,
      // Always persist metadata so getAllTableStates can read them reliably
      _server: normalizeHost(serverHost),
      _database: sanitizeSegment(database),
      _table: tableName,
    };
    localStorage.setItem(key, JSON.stringify(merged));
  } catch (e) {
    console.warn('[syncStateStore] Failed to save state:', e);
  }
}

/**
 * Clear sync state for a specific table.
 */
export function clearTableState(serverHost, database, tableName) {
  try {
    const key = buildKey(serverHost, database, tableName);
    localStorage.removeItem(key);
  } catch (e) {
    console.warn('[syncStateStore] Failed to clear state:', e);
  }
}

/**
 * Clear all table sync states.
 */
export function clearAllTableStates() {
  try {
    const keysToRemove = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && key.startsWith(STORE_PREFIX)) {
        keysToRemove.push(key);
      }
    }
    keysToRemove.forEach((k) => localStorage.removeItem(k));
  } catch (e) {
    console.warn('[syncStateStore] Failed to clear all states:', e);
  }
}

/**
 * Get all table sync states as an array of { server, database, table, ...state }.
 * Optionally filter by serverHost and database.
 */
export function getAllTableStates(filterServer, filterDatabase) {
  const results = [];
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (!key || !key.startsWith(STORE_PREFIX)) continue;

      const suffix = key.slice(STORE_PREFIX.length);
      // Format: host_database_tableName — split on first two underscores
      const parts = suffix.split('_');
      if (parts.length < 3) continue;

      // Host can contain dots, database and table can contain underscores
      // We stored them as host_db_table but all segments are sanitized
      // Use a smarter approach: try to parse the stored JSON and see if it matches
      const raw = localStorage.getItem(key);
      if (!raw) continue;

      try {
        const state = JSON.parse(raw);
        const entry = {
          key,
          server: state._server || parts[0] || '',
          database: state._database || parts[1] || '',
          table: state._table || parts.slice(2).join('_') || '',
          ...state,
        };

        if (filterServer && entry.server !== normalizeHost(filterServer)) continue;
        if (filterDatabase && entry.database !== sanitizeSegment(filterDatabase)) continue;

        results.push(entry);
      } catch {
        // skip malformed entries
      }
    }
  } catch (e) {
    console.warn('[syncStateStore] Failed to read all states:', e);
  }
  return results;
}
