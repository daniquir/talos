/** OIDC PKCE helper for the extension (Keycloak). */

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

/**
 * Interactive OIDC login; returns id_token from Keycloak token endpoint.
 */
export async function loginOidc({ issuer, clientId }) {
  if (!issuer || !clientId) {
    throw new Error("Configure OIDC issuer and client id in extension options");
  }
  const redirectUri = chrome.identity.getRedirectURL();
  const verifier = randomVerifier();
  const challenge = await challengeS256(verifier);
  const state = randomVerifier().slice(0, 16);
  const authUrl =
    `${issuer.replace(/\/+$/, "")}/protocol/openid-connect/auth` +
    `?client_id=${encodeURIComponent(clientId)}` +
    `&redirect_uri=${encodeURIComponent(redirectUri)}` +
    `&response_type=code` +
    `&scope=${encodeURIComponent("openid profile email")}` +
    `&state=${encodeURIComponent(state)}` +
    `&code_challenge=${encodeURIComponent(challenge)}` +
    `&code_challenge_method=S256`;

  const redirected = await chrome.identity.launchWebAuthFlow({
    url: authUrl,
    interactive: true,
  });
  if (!redirected) throw new Error("OIDC login cancelled");
  const u = new URL(redirected);
  const err = u.searchParams.get("error");
  if (err) throw new Error(err);
  const code = u.searchParams.get("code");
  const st = u.searchParams.get("state");
  if (!code) throw new Error("Missing OIDC code");
  if (st !== state) throw new Error("OIDC state mismatch");

  const tokenUrl = `${issuer.replace(/\/+$/, "")}/protocol/openid-connect/token`;
  const body = new URLSearchParams({
    grant_type: "authorization_code",
    code,
    redirect_uri: redirectUri,
    client_id: clientId,
    code_verifier: verifier,
  });
  const res = await fetch(tokenUrl, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new Error(data.error_description || data.error || `OIDC token HTTP ${res.status}`);
  }
  if (!data.id_token) throw new Error("Missing id_token");
  return {
    idToken: data.id_token,
    accessToken: data.access_token || null,
  };
}
