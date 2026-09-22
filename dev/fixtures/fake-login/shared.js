/**
 * Shared fake accounts for extension multi-match tests.
 * Must stay in sync with dev/fixtures/secrets.json (URL http://127.0.0.1:8765).
 */
window.ACME_ACCOUNTS = [
  {
    path: "Personal/AcmeDemo/demo-user",
    user: "demo.user",
    password: "ExtTest-Pass-2026",
    kind: "username",
  },
  {
    path: "Personal/AcmeDemo/alice-email",
    user: "alice@acme.test",
    password: "AliceMail#2026",
    kind: "email",
  },
  {
    path: "Personal/AcmeDemo/mobile",
    user: "+34600111222",
    password: "PhonePin-2026",
    kind: "phone",
  },
  {
    path: "Personal/AcmeDemo/work-ops",
    user: "work.ops",
    password: "WorkOps!2026",
    kind: "username",
  },
];

window.acmeNormalize = function acmeNormalize(value) {
  return String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[\s()-]/g, "");
};

window.acmeValidate = function acmeValidate(user, password) {
  const u = window.acmeNormalize(user);
  const p = String(password || "");
  return window.ACME_ACCOUNTS.find(
    (a) => window.acmeNormalize(a.user) === u && a.password === p
  );
};

window.acmeWireOneStep = function acmeWireOneStep(opts) {
  const form = document.getElementById("login-form");
  const result = document.getElementById("result");
  const idInput = document.getElementById(opts.idField || "identifier");
  const passInput = document.getElementById("password");

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const match = window.acmeValidate(idInput.value, passInput.value);
    result.className = match ? "ok" : "fail";
    result.textContent = match
      ? `Signed in as ${match.user} (${match.path}).`
      : "Wrong credentials — pick another Talos match or check the seed list.";
  });
};

window.acmeWireTwoStep = function acmeWireTwoStep(opts) {
  const form = document.getElementById("login-form");
  const result = document.getElementById("result");
  const stepId = document.getElementById("step-id");
  const stepPass = document.getElementById("step-pass");
  const idInput = document.getElementById(opts.idField || "identifier");
  const passInput = document.getElementById("password");

  document.getElementById("btn-next").addEventListener("click", () => {
    if (!idInput.value.trim()) {
      result.className = "fail";
      result.textContent = "Enter an identity first (or autofill from Talos).";
      return;
    }
    result.textContent = "";
    stepId.hidden = true;
    stepPass.hidden = false;
    passInput.required = true;
    passInput.focus();
  });

  document.getElementById("btn-back").addEventListener("click", () => {
    stepPass.hidden = true;
    stepId.hidden = false;
    passInput.required = false;
    passInput.value = "";
    idInput.focus();
  });

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const match = window.acmeValidate(idInput.value, passInput.value);
    result.className = match ? "ok" : "fail";
    result.textContent = match
      ? `Signed in as ${match.user} (${match.path}).`
      : "Wrong credentials — pick another Talos match or check the seed list.";
  });
};

window.acmeGoSuccess = function acmeGoSuccess(query) {
  const q = query ? `?${query}` : "";
  location.href = `success.html${q}`;
};

window.acmeWireRegister = function acmeWireRegister(opts) {
  const form = document.getElementById("register-form");
  const result = document.getElementById("result");
  const idInput = document.getElementById(opts.idField || "identifier");
  const passInput = document.getElementById("password");
  const confirmInput = document.getElementById("password-confirm");

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const user = idInput.value.trim();
    const pass = passInput.value;
    const confirm = confirmInput.value;
    if (!user || pass.length < 2) {
      result.className = "fail";
      result.textContent = "Need identity and password.";
      return;
    }
    if (pass !== confirm) {
      result.className = "fail";
      result.textContent = "Passwords do not match.";
      return;
    }
    if (window.acmeValidate(user, pass) || window.ACME_ACCOUNTS.some((a) => window.acmeNormalize(a.user) === window.acmeNormalize(user))) {
      result.className = "fail";
      result.textContent = "That identity already exists in the seed vault — use a new user for Save tests.";
      return;
    }
    result.className = "ok";
    result.textContent = `Registered ${user} (local fake). Talos should offer Save.`;
    if (opts.navigate) {
      setTimeout(() => window.acmeGoSuccess(`mode=register&user=${encodeURIComponent(user)}`), 400);
    }
  });
};

window.acmeWireChangePassword = function acmeWireChangePassword(opts) {
  const form = document.getElementById("change-form");
  const result = document.getElementById("result");
  const idInput = document.getElementById(opts.idField || "identifier");
  const currentInput = document.getElementById("current-password");
  const passInput = document.getElementById("password");
  const confirmInput = document.getElementById("password-confirm");

  if (opts.defaultUser && idInput && !idInput.value) {
    idInput.value = opts.defaultUser;
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const user = idInput.value.trim();
    const neu = passInput.value;
    const confirm = confirmInput ? confirmInput.value : neu;
    if (confirmInput && neu !== confirm) {
      result.className = "fail";
      result.textContent = "New passwords do not match.";
      return;
    }
    if (neu.length < 2) {
      result.className = "fail";
      result.textContent = "New password too short.";
      return;
    }
    if (currentInput) {
      const ok = window.acmeValidate(user, currentInput.value);
      if (!ok) {
        result.className = "fail";
        result.textContent = "Current password does not match a seed account.";
        return;
      }
    } else if (!window.ACME_ACCOUNTS.some((a) => window.acmeNormalize(a.user) === window.acmeNormalize(user))) {
      result.className = "fail";
      result.textContent = "Unknown seed user — use demo.user / alice@acme.test / etc.";
      return;
    }
    result.className = "ok";
    result.textContent = `Password changed for ${user} (local fake). Talos should offer Update.`;
    if (opts.navigate) {
      setTimeout(() => window.acmeGoSuccess(`mode=change&user=${encodeURIComponent(user)}`), 400);
    }
  });
};
