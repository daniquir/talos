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

function explainOidcFailure(raw, { issuer, clientId, redirectUri }) {
  const msg = String(raw || "unknown").trim();
  const lower = msg.toLowerCase();
  if (lower === "not_found" || lower.includes("not_found")) {
    return (
      `OIDC redirect failed (not_found). This is not the vault passphrase. ` +
      `In Keycloak client "${clientId}", Valid redirect URIs must include exactly:\n${redirectUri}\n` +
      `Also allow https://*.extensions.allizom.org/* (Firefox AMO) and https://*.chromiumapp.org/* (Chrome). ` +
      `Issuer in options must be ${issuer}`
    );
  }
  if (lower.includes("redirect") || lower.includes("invalid_request")) {
    return (
      `OIDC redirect_uri rejected. Register this exact URI on client "${clientId}":\n${redirectUri}`
    );
  }
  return `OIDC login failed: ${msg}`;
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

  const tokenUrl = `${issuerBase}/protocol/openid-connect/token`;
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
    throw new Error(
      explainOidcFailure(
        data.error_description || data.error || `OIDC token HTTP ${res.status}`,
        { issuer: issuerBase, clientId, redirectUri }
      )
    );
  }
  if (!data.id_token) throw new Error("Missing id_token");
  return {
    idToken: data.id_token,
    accessToken: data.access_token || null,
  };
}
