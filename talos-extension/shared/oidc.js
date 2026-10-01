/** OIDC PKCE helper for the extension (Keycloak).
 *
 * Browser identity flow only; the authorization *code* is exchanged by talos-web
 * so Keycloak never sees chrome-extension:// / moz-extension:// Origins (no Web Origins *).
 */

function b64url(buf) {
  const bytes = buf instanceof ArrayBuffer ? new Uint8Array(buf) : buf;
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function randomVerifier() {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return b64url(bytes);
}

async function challengeS256(verifier) {
  const data = new TextEncoder().encode(verifier);
  const digest = await crypto.subtle.digest("SHA-256", data);
  return b64url(digest);
}

function isGecko() {
  try {
    return String(chrome.runtime.getURL("")).startsWith("moz-extension:");
  } catch {
    return false;
  }
}

/**
 * Redirect URI for identity.launchWebAuthFlow.
 * Firefox AMO: prefer loopback mozoauth2 — allizom.org intercept often fails with
 * a bare `not_found` after Keycloak login (DNS/404 on the dummy host).
 * Chrome/Edge: use the chromiumapp.org URL from getRedirectURL().
 */
export function getOidcRedirectUri() {
  const identityUrl = chrome.identity.getRedirectURL();
  if (!isGecko()) return identityUrl;
  try {
    const host = new URL(identityUrl).hostname;
    // https://<hash>.extensions.allizom.org/ → http://127.0.0.1/mozoauth2/<hash>/
    const m = host.match(
      /^([a-f0-9]+)\.extensions\.(allizom|mozilla)\.org$/i
    );
    if (m) {
      return `http://127.0.0.1/mozoauth2/${m[1]}/`;
    }
  } catch {
    /* fall through */
  }
  return identityUrl;
}

function explainOidcFailure(raw, { issuer, clientId, redirectUri }) {
  const msg = String(raw || "unknown").trim();
  const lower = msg.toLowerCase();
  if (lower === "not_found" || lower.includes("not_found")) {
    return (
      `OIDC redirect failed (not_found). This is NOT the vault passphrase. ` +
      `Keycloak client "${clientId}" must allow this redirect URI exactly:\n${redirectUri}\n` +
      `(Firefox uses http://127.0.0.1/mozoauth2/<hash>/ — covered by http://127.0.0.1/* if present.) ` +
      `Issuer: ${issuer}`
    );
  }
  if (lower.includes("redirect") || lower.includes("invalid_request")) {
    return (
      `OIDC redirect_uri rejected. Register this exact URI on client "${clientId}":\n${redirectUri}`
    );
  }
  if (lower.includes("invalid origin") || lower === "invalid_origin") {
    return (
      `OIDC failed (Invalid origin). Update Talos server (≥ code-exchange unlock) ` +
      `so the extension does not POST to Keycloak from chrome-extension:// / moz-extension://. ` +
      `Do not set Keycloak Web Origins to *. Issuer: ${issuer}`
    );
  }
  if (lower.includes("failed to fetch") || lower.includes("networkerror")) {
    return (
      `Cannot reach Talos or Keycloak (${issuer}). ` +
      `Grant the extension host permission and check server / issuer URLs.`
    );
  }
  return `OIDC login failed: ${msg}`;
}

/**
 * Interactive OIDC authorize; returns authorization code + PKCE verifier.
 * Token exchange is done by talos-web (`POST /api/auth/token/oidc`).
 */
export async function loginOidc({ issuer, clientId }) {
  if (!issuer || !clientId) {
    throw new Error("Configure OIDC issuer and client id in extension options");
  }
  const redirectUri = getOidcRedirectUri();
  const verifier = randomVerifier();
  const challenge = await challengeS256(verifier);
  const state = randomVerifier().slice(0, 16);
  const issuerBase = issuer.replace(/\/+$/, "");
  const authUrl =
    `${issuerBase}/protocol/openid-connect/auth` +
    `?client_id=${encodeURIComponent(clientId)}` +
    `&redirect_uri=${encodeURIComponent(redirectUri)}` +
    `&response_type=code` +
    `&scope=${encodeURIComponent("openid profile email")}` +
    `&state=${encodeURIComponent(state)}` +
    `&code_challenge=${encodeURIComponent(challenge)}` +
    `&code_challenge_method=S256`;

  let redirected;
  try {
    redirected = await chrome.identity.launchWebAuthFlow({
      url: authUrl,
      interactive: true,
    });
  } catch (e) {
    throw new Error(
      explainOidcFailure(e?.message || e, { issuer: issuerBase, clientId, redirectUri })
    );
  }
  if (!redirected) throw new Error("OIDC login cancelled");
  const u = new URL(redirected);
  const err = u.searchParams.get("error");
  if (err) {
    const desc = u.searchParams.get("error_description") || err;
    throw new Error(
      explainOidcFailure(desc, { issuer: issuerBase, clientId, redirectUri })
    );
  }
  const code = u.searchParams.get("code");
  const st = u.searchParams.get("state");
  if (!code) throw new Error("Missing OIDC code");
  if (st !== state) throw new Error("OIDC state mismatch");

  return {
    code,
    codeVerifier: verifier,
    redirectUri,
    clientId,
  };
}
