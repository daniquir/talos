/** Shared Talos API client (extension). */

export function normalizeBaseUrl(url) {
  return (url || "").trim().replace(/\/+$/, "");
}

/** Loopback only — plain HTTP allowed nowhere else. */
export function isLoopbackHostname(hostname) {
  const h = String(hostname || "")
    .trim()
    .toLowerCase()
    .replace(/^\[|\]$/g, "");
  return h === "localhost" || h === "127.0.0.1" || h === "::1";
}

/**
 * Military posture: HTTPS required except loopback (local docker/dev).
 * Rejects http://LAN-IP and http://hostname.
 */
export function assertSecureServerUrl(url) {
  const base = normalizeBaseUrl(url);
  if (!base) throw new Error("Server URL is required");
  let parsed;
  try {
    parsed = new URL(base);
  } catch {
    throw new Error("Invalid server URL");
  }
  if (parsed.protocol === "https:") return base;
  if (parsed.protocol === "http:" && isLoopbackHostname(parsed.hostname)) return base;
  throw new Error("HTTPS is required for non-localhost Talos servers");
}

/** Auto-lock idle minutes: 0 = off. Allowed: 0, 5, 15, 30. Default 15. */
export function normalizeAutoLockMinutes(value) {
  if (value === undefined || value === null || value === "") return 15;
  const n = Number(value);
  if (n === 0 || n === 5 || n === 15 || n === 30) return n;
  return 15;
}

export async function getConfig() {
  const data = await chrome.storage.local.get([
    "serverUrl",
    "persistSessionToken",
    "autoLockMinutes",
    "oidcIssuer",
    "oidcClientId",
  ]);
  const rawUrl = normalizeBaseUrl(data.serverUrl || "http://localhost:3000");
  let serverUrl = rawUrl;
  try {
    serverUrl = assertSecureServerUrl(rawUrl);
  } catch {
    /* keep raw for options UI so the user can fix it */
  }
  return {
    serverUrl,
    persistSessionToken: Boolean(data.persistSessionToken),
    autoLockMinutes: normalizeAutoLockMinutes(data.autoLockMinutes),
    oidcIssuer: normalizeBaseUrl(data.oidcIssuer || "http://localhost:8080/realms/talos"),
    oidcClientId: (data.oidcClientId || "talos-extension").trim(),
  };
}

export async function setConfig({
  serverUrl,
  persistSessionToken,
  autoLockMinutes,
  oidcIssuer,
  oidcClientId,
}) {
  const patch = {};
  if (serverUrl !== undefined) {
    patch.serverUrl = assertSecureServerUrl(serverUrl);
  }
  if (persistSessionToken !== undefined) {
    patch.persistSessionToken = Boolean(persistSessionToken);
  }
  if (autoLockMinutes !== undefined) {
    patch.autoLockMinutes = normalizeAutoLockMinutes(autoLockMinutes);
  }
  if (oidcIssuer !== undefined) {
    patch.oidcIssuer = normalizeBaseUrl(oidcIssuer);
  }
  if (oidcClientId !== undefined) {
    patch.oidcClientId = String(oidcClientId || "").trim();
  }
  await chrome.storage.local.set(patch);
}

/** Session-scoped store (cleared when the browser process exits). */
export function getSessionStore() {
  return chrome.storage?.session || null;
}

export async function saveSessionToken(token, expiresAt) {
  const store = getSessionStore();
  if (!store) return false;
  await store.set({
    accessToken: token,
    expiresAt: expiresAt || null,
  });
  return true;
}

export async function loadSessionToken() {
  const store = getSessionStore();
  if (!store) return null;
  const data = await store.get(["accessToken", "expiresAt"]);
  if (!data.accessToken) return null;
  return {
    token: data.accessToken,
    expiresAt: data.expiresAt || null,
  };
}

export async function clearSessionToken() {
  const store = getSessionStore();
  if (!store) return;
  await store.remove(["accessToken", "expiresAt"]);
}

export async function ensureHostPermission(serverUrl) {
  const base = assertSecureServerUrl(serverUrl);
  let origin;
  try {
    origin = new URL(base).origin + "/*";
  } catch {
    throw new Error("Invalid server URL");
  }
  const has = await chrome.permissions.contains({ origins: [origin] });
  if (has) return true;
  return chrome.permissions.request({ origins: [origin] });
}

async function request(path, { method = "GET", body, token } = {}) {
  const { serverUrl } = await getConfig();
  if (!serverUrl) {
    throw new Error("Configure the Talos server URL in extension options");
  }
  assertSecureServerUrl(serverUrl);

  const headers = { Accept: "application/json" };
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
  }
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  const res = await fetch(`${serverUrl}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  let data = null;
  const text = await res.text();
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
  }

  if (!res.ok) {
    const msg =
      (data && data.error) ||
      (typeof data === "string" ? data : null) ||
      res.statusText ||
      `HTTP ${res.status}`;
    const err = new Error(msg);
    err.status = res.status;
    throw err;
  }
  return data;
}

export const API = {
  health() {
    return request("/api/health");
  },
  version() {
    return request("/api/version");
  },
  status(token) {
    return request("/api/auth/status", { token });
  },
  issueToken(masterKey) {
    return request("/api/auth/token", {
      method: "POST",
      body: { key: masterKey },
    });
  },
  issueTokenOidc(idToken, masterKey) {
    return request("/api/auth/token/oidc", {
      method: "POST",
      body: {
        id_token: idToken,
        key: masterKey || null,
      },
    });
  },
  authStatus(token) {
    return request("/api/auth/status", { token });
  },
  logout(token) {
    return request("/api/auth/logout", { method: "POST", token });
  },
  match(host, token) {
    if (!token) {
      const err = new Error("Vault is locked");
      err.status = 401;
      throw err;
    }
    return request(`/api/match?host=${encodeURIComponent(host)}`, { token });
  },
  reindex(token) {
    return request("/api/match/reindex", { method: "POST", token });
  },
  tree(token) {
    return request("/api/tree", { token });
  },
  save(path, content, token, originalPath = null) {
    return request("/api/save", {
      method: "POST",
      body: {
        path,
        content,
        original_path: originalPath || null,
      },
      token,
    });
  },
  decrypt(path, token, reveal = true) {
    return request("/api/decrypt", {
      method: "POST",
      body: { path, reveal },
      token,
    });
  },
  settings(token) {
    return request("/api/settings", { token });
  },
  updateSettings(patch, token) {
    return request("/api/settings", {
      method: "PUT",
      body: patch || {},
      token,
    });
  },
};

/**
 * Parse pass-format secret body into password / user / url / notes.
 * Notes = remaining lines after known metadata keys (same as web UI).
 */
export function parseCredential(content) {
  const text = typeof content === "string" ? content : String(content ?? "");
  const lines = text.split(/\r?\n/);
  const password = lines[0] || "";
  let username = "";
  let url = "";
  const notesLines = [];
  for (const line of lines.slice(1)) {
    const idx = line.indexOf(":");
    if (idx < 0) {
      notesLines.push(line);
      continue;
    }
    const key = line.slice(0, idx).trim().toLowerCase();
    const value = line.slice(idx + 1).trim();
    if (key === "user" || key === "username" || key === "login") {
      if (value) username = value;
      continue;
    }
    if (key === "url" || key === "website") {
      if (value) url = value;
      continue;
    }
    notesLines.push(line);
  }
  return {
    password,
    username,
    url,
    notes: notesLines.join("\n").replace(/^\n+|\n+$/g, ""),
  };
}

/** Build pass-format body for save (password + User + URL + freeform notes). */
export function formatPassContent({ password, username, url, notes }) {
  let content = String(password ?? "");
  if (username) content += `\nUser: ${username}`;
  if (url) content += `\nURL: ${url}`;
  const note = String(notes ?? "").replace(/^\n+|\n+$/g, "");
  if (note) content += `\n${note}`;
  return content;
}

export function normalizeUsername(value) {
  return String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[\s()-]/g, "");
}

export async function getCaptureNeverHosts() {
  const data = await chrome.storage.local.get(["captureNeverHosts"]);
  return Array.isArray(data.captureNeverHosts) ? data.captureNeverHosts : [];
}

export async function addCaptureNeverHost(host) {
  const h = String(host || "").trim().toLowerCase();
  if (!h) return;
  const list = await getCaptureNeverHosts();
  if (list.includes(h)) return;
  list.push(h);
  await chrome.storage.local.set({ captureNeverHosts: list });
}

export async function isCaptureNeverHost(host) {
  const h = String(host || "").trim().toLowerCase();
  const list = await getCaptureNeverHosts();
  return list.includes(h);
}
