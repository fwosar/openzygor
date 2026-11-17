const root = document.querySelector("#app");
const tauri = window.__TAURI__;
const games = [
  {
    id: "wow",
    title: "World of Warcraft",
    subtitle: "RETAIL",
    folder: "_retail_",
    symbol: "sun",
  },
  {
    id: "wow-classic",
    title: "Classic Era",
    subtitle: "CLASSIC",
    folder: "_classic_era_",
    symbol: "shield",
  },
  {
    id: "wow-classic-tbc",
    title: "Mists of Pandaria",
    subtitle: "CLASSIC PROGRESSION",
    folder: "_classic_",
    symbol: "leaf",
  },
  {
    id: "wow-classic-tbc-anniv",
    title: "TBC Anniversary",
    subtitle: "ANNIVERSARY",
    folder: "_anniversary_",
    symbol: "diamond",
  },
  {
    id: "wow-forever",
    title: "WoW Forever",
    subtitle: "FOREVER",
    folder: "_forever_",
    symbol: "infinity",
  },
];
const paths = {
  guides:
    "M4 5.5A2.5 2.5 0 0 1 6.5 3H20v18H6.5A2.5 2.5 0 0 1 4 18.5zm0 13A2.5 2.5 0 0 1 6.5 16H20M8 7h8M8 10h5",
  folder:
    "M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
  settings:
    "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8M12 2v3m0 14v3M2 12h3m14 0h3M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2",
  activity: "M3 12h4l3-8 4 16 3-8h4",
  user: "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8M4 21v-2a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v2",
  arrow: "M12 4v16m-6-6 6 6 6-6",
  refresh: "M20 7V3m0 4h-4M4 17v4m0-4h4M5 7a8 8 0 0 1 15 0M4 17a8 8 0 0 0 15 0",
  check: "m5 12 4 4L19 6",
  plus: "M12 5v14M5 12h14",
  external: "M14 3h7v7m0-7L10 14M10 3H4v17h17v-6",
  logout: "M9 4H4v16h5m5-4 4-4-4-4m-6 4h13",
  sun: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8M12 1v3m0 16v3M1 12h3m16 0h3M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2",
  shield: "m12 2 8 4v6c0 5-8 10-8 10S4 17 4 12V6zM8 12l3 3 5-6",
  leaf: "M20 3C5 1 0 13 8 18s15-3 12-15ZM5 21 17 7",
  diamond: "m12 2 8 10-8 10-8-10zM4 12h16M12 2v20",
  infinity: "M12 12C4-1-4 21 6 17c5-2 7-12 12-10 10 4 2 16-6 5",
};
const icon = (name, cls = "") =>
  `<svg class="icon ${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${paths[name] || paths.guides}"/></svg>`;
const escape = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (char) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        char
      ],
  );
let state = {
  config: { region: "us", installations: [] },
  account: null,
  packages: [],
  installations: [],
  warning: null,
};
let page = "guides",
  busy = false,
  busyLabel = "",
  error = "",
  notice = "",
  progress = null;
let selectedGame = "wow",
  username = "",
  remember = true,
  directory = "",
  logs = [];
let lastChecked = null;
let activityWarning = "";
let activityQueue = Promise.resolve();
const disabled = () => (busy ? "disabled" : "");
const packageFor = (game) => state.packages.find((p) => p.game === game);
const invoke = (command, args) => tauri.core.invoke(command, args);
const log = (message, failed = false) =>
  updateActivity("append_activity", { message, failed });

function updateActivity(command, args) {
  // Keep writes and clears in order even when a user navigates during an operation.
  activityQueue = activityQueue.then(async () => {
    try {
      logs = await invoke(command, args);
      activityWarning = "";
    } catch (e) {
      const action =
        command === "load_activity"
          ? "load"
          : command === "clear_activity"
            ? "clear"
            : "save";
      activityWarning = `Could not ${action} activity: ${String(e)}`;
      if (command === "append_activity") {
        logs.unshift({ ...args, timestamp_ms: Date.now() });
        logs = logs.slice(0, 100);
      }
    }
  });
  return activityQueue;
}

function activityTime(timestamp) {
  const date = new Date(timestamp);
  return `<time datetime="${date.toISOString()}">${escape(date.toLocaleDateString([], { year: "numeric", month: "short", day: "numeric" }))}<span>${escape(date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }))}</span></time>`;
}

function brand() {
  return `<span class="brand-mark" aria-hidden="true"><svg viewBox="0 0 32 32"><path d="m16 3 12 7v12l-12 7-12-7V10z"/><path class="route" d="m10 12 12 0-12 9h12"/></svg></span><span>Open<span class="brand-light">Zygor</span></span>`;
}

function render() {
  root.innerHTML = `<aside class="sidebar"><div class="brand">${brand()}</div>
    <nav aria-label="Main navigation">${[
      ["guides", "guides", "My guides"],
      ["directories", "folder", "Game folders"],
      ["activity", "activity", "Activity"],
      ["settings", "settings", "Preferences"],
    ]
      .map(
        ([id, symbol, label]) =>
          `<button data-page="${id}" class="nav-item ${page === id ? "active" : ""}" ${page === id ? 'aria-current="page"' : ""}>${icon(symbol)}${label}${id === "guides" && state.installations.length ? `<span class="nav-count">${state.installations.length}</span>` : ""}</button>`,
      )
      .join("")}</nav>
    <div class="sidebar-bottom"><div class="native-badge"><span class="status-dot"></span>Native on Linux</div><button class="text-button support" data-link="support">Zygor support ${icon("external")}</button><div class="app-version">OPENZYGOR <span>v0.1.0</span></div></div></aside>
    <div class="app-content"><header class="topbar">
      <div class="account-chip">${icon("user")}<div>${escape(state.account?.display_name || "Zygor account")}<small>${escape(state.account ? state.account.access.level || "Signed in" : "Not signed in")}</small></div>${state.account ? `<button title="Sign out" aria-label="Sign out" class="icon-button" data-action="logout" ${disabled()}>${icon("logout")}</button>` : ""}</div></header>
    <main>${error ? `<div class="banner error" role="alert"><span>${escape(error)}</span><button class="icon-button" data-action="dismiss-error" aria-label="Dismiss error">×</button></div>` : ""}
      ${activityWarning ? `<div class="banner warning" role="status">${escape(activityWarning)}</div>` : ""}
      ${state.warning ? `<div class="banner warning" role="status">${escape(state.warning)}</div>` : ""}
      ${notice ? `<div class="banner success" role="status">${icon("check")}${escape(notice)}</div>` : ""}
      ${page === "guides" ? (state.account ? guidesPage() : loginPage()) : page === "directories" ? directoriesPage() : page === "activity" ? activityPage() : settingsPage()}
    </main><footer><span><span class="status-dot ${busy ? "working" : ""}"></span>${escape(busy ? busyLabel : "Ready")}</span><span>OpenZygor is not affiliated with Zygor.</span></footer></div>
    <div id="download-status" class="download-status" ${progress ? "" : "hidden"} aria-live="polite"></div>`;
  renderProgress();
}

function loginPage() {
  return `<section class="welcome">
    <section class="login-card" aria-labelledby="login-title"><div class="card-symbol">${icon("guides")}</div><h1 id="login-title">Sign in</h1><p>Sign in with your Zygor Guides account.</p>
      <form id="login-form"><label for="username">Username</label><input id="username" name="username" autocomplete="username" placeholder="Your Zygor username" value="${escape(username || state.config.username)}" required ${disabled()}>
      <div class="label-row"><label for="password">Password</label><button type="button" class="text-button" data-link="password">Forgot password?</button></div><input id="password" name="password" type="password" autocomplete="current-password" placeholder="Enter your password" required ${disabled()}>
      <label class="checkbox"><input name="remember" type="checkbox" ${remember ? "checked" : ""} ${disabled()}>Remember me</label>
      <button class="primary full-width" type="submit" ${disabled()}>${busy ? "Signing in…" : "Sign in to Zygor"}${icon("external")}</button></form>
      <div class="login-note">Use your Zygor username, not your Battle.net login. Your password is never saved.</div>
      <div class="register"><button class="text-button" data-link="register">Create an account ${icon("external")}</button></div></section>
    <div class="welcome-bottom"><button class="text-button" data-page="directories">Game folders ${icon("folder")}</button></div></section>`;
}

function heading(title, actions = "") {
  return `<div class="page-heading"><h1>${title}</h1>${actions ? `<div class="heading-actions">${actions}</div>` : ""}</div>`;
}
function availableUpdates() {
  return state.installations.filter((i) => {
    const p = packageFor(i.game);
    return (
      i.exists &&
      p?.status.available &&
      !p.status.hidden &&
      p.ver &&
      i.installed_version &&
      i.installed_version !== p.ver
    );
  });
}
function guidesPage() {
  const updates = availableUpdates();
  const installedGames = new Set(
    state.installations.filter((i) => i.installed_version).map((i) => i.game),
  );
  const visibleGames = games
    .filter(
      (g) =>
        !packageFor(g.id)?.status.hidden &&
        (packageFor(g.id) || state.installations.some((i) => i.game === g.id)),
    )
    .sort(
      (a, b) =>
        Number(installedGames.has(b.id)) - Number(installedGames.has(a.id)),
    );
  return (
    heading(
      "My guides",
      `<button class="secondary" data-action="refresh" ${disabled()}>${icon("refresh")}Check for updates</button><button class="primary" data-action="update-all" ${busy || !updates.length ? "disabled" : ""}>${icon("arrow")}Update all${updates.length ? ` (${updates.length})` : ""}</button>`,
    ) +
    `<div class="library-summary"><div><span class="status-dot"></span>${updates.length ? `${updates.length} update${updates.length === 1 ? "" : "s"} available` : "Guide library"}</div><div>${lastChecked ? `Checked ${escape(lastChecked)}` : ""}<span class="region-tag">${escape(state.config.region.toUpperCase())}</span></div></div>
    ${
      visibleGames.length
        ? `<div class="guide-grid">${visibleGames
            .map((g) => {
              const p = packageFor(g.id),
                installations = state.installations.filter(
                  (i) => i.game === g.id,
                );
              return `<article class="guide-card"><div class="guide-cover ${g.id}"><div class="cover-grid"></div><span class="edition-label">${g.subtitle}</span><span class="edition-symbol">${icon(g.symbol)}</span></div><div class="guide-content"><div class="guide-title"><h2>${g.title}</h2><span class="version-label">${p?.ver ? `v${escape(p.ver)}` : "—"}</span></div><p class="guide-description">${p?.status.available ? "Zygor Guides Viewer" : escape(p?.status.minimessage || "Check availability with your Zygor account")}</p>
      ${
        installations.length
          ? installations
              .map((i) => {
                const available = p?.status.available && i.exists;
                const current = Boolean(
                  i.installed_version && i.installed_version === p?.ver,
                );
                const label = !i.exists
                  ? "Folder missing"
                  : current
                    ? "Up to date"
                    : i.installed_version
                      ? "Update available"
                      : "Not installed";
                return `<div class="installation"><div class="install-meta"><span class="pill ${current ? "current" : i.installed_version ? "update" : ""}">${current ? icon("check") : ""}${label}</span><span>${i.installed_version ? `Installed ${escape(i.installed_version)}` : "Ready to install"}</span></div><div class="install-path" title="${escape(i.path)}">${icon("folder")}<span>${escape(i.path)}</span></div>
        <div class="card-actions"><button class="${current ? "secondary" : "primary"}" data-action="install" data-id="${escape(i.id)}" ${busy || !available ? "disabled" : ""}>${icon(current ? "refresh" : "arrow")}${current ? "Repair guides" : i.installed_version ? "Update guides" : "Install guides"}</button>${i.installed_version ? `<button class="text-button muted" data-action="uninstall" data-id="${escape(i.id)}" ${disabled()}>Uninstall</button>` : ""}</div></div>`;
              })
              .join("")
          : `<div class="no-folder"><p>${icon("folder")}Choose where this edition is installed.</p><button class="secondary" data-configure="${g.id}">Set up game folder</button></div>`
      }</div></article>`;
            })
            .join("")}</div>`
        : `<div class="empty-state">${icon("guides")}<h2>No guides loaded yet</h2><p>Check for updates to see which guides your account can download.</p></div>`
    }
    <p class="section-note">After an update, type <code>/reload</code> in WoW or restart the game to load your guides.</p>`
  );
}

function directoriesPage() {
  return (
    heading(
      "Game folders",
      `<button class="secondary" data-action="detect" ${disabled()}>${icon("refresh")}Detect installations</button>`,
    ) +
    `<div class="directory-layout"><section class="panel"><div class="panel-heading"><h2>Connected folders</h2><span class="count">${state.installations.length}</span></div>${state.installations.length ? state.installations.map((i) => `<div class="directory-row"><div class="directory-icon">${icon("folder")}</div><div><h3>${escape(i.title)} ${!i.exists ? '<span class="pill update">Folder missing</span>' : ""}</h3><p>${escape(i.path)}</p></div><button class="text-button muted" data-action="remove-folder" data-id="${escape(i.id)}" ${disabled()}>Remove</button></div>`).join("") : `<div class="empty-state compact">${icon("folder")}<h3>No game folders connected</h3><p>Detect Bottles, Wine, Lutris, and Proton installations, or add a folder manually.</p></div>`}<div class="panel-footnote">Removing a folder here leaves its game and addon files in place.</div></section>
    <section class="panel add-folder"><h2>Add a game folder</h2><p>Select the edition and its installation folder.</p><form id="folder-form"><label for="game">Game edition</label><select id="game" name="game" ${disabled()}>${games.map((g) => `<option value="${g.id}" ${g.id === selectedGame ? "selected" : ""}>${g.title}</option>`).join("")}</select><label for="directory">Installation folder</label><div class="path-input"><input id="directory" name="path" value="${escape(directory)}" placeholder="/path/to/World of Warcraft" required ${disabled()}><button type="button" class="secondary" data-action="browse" aria-label="Browse for a folder" ${disabled()}>${icon("folder")}</button></div><p class="input-hint">Choose the World of Warcraft folder, an edition folder such as <code>_retail_</code>, or its <code>Interface/AddOns</code> folder.</p><button class="primary full-width" type="submit" ${disabled()}>${icon("plus")}Connect folder</button></form></section></div>`
  );
}

function settingsPage() {
  return (
    heading("Preferences") +
    `<section class="panel preferences"><h2>Guide downloads</h2><form id="settings-form"><div class="setting-row"><div><label for="region">Game region</label><p>Guide releases can differ between regions.</p></div><select id="region" name="region" ${disabled()}>${[
      ["us", "Americas & Oceania"],
      ["eu", "Europe"],
      ["kr", "Korea"],
      ["tw", "Taiwan"],
      ["cn", "China"],
    ]
      .map(
        ([id, title]) =>
          `<option value="${id}" ${id === state.config.region ? "selected" : ""}>${title}</option>`,
      )
      .join(
        "",
      )}</select></div><div class="setting-row"><div><label for="auto-update">Update periodically</label><p>Update installed guides every 30 minutes while OpenZygor is open.</p></div><input id="auto-update" type="checkbox" name="auto" ${state.config.update_automatically ? "checked" : ""} ${disabled()}></div><button class="primary" type="submit" ${disabled()}>Save preferences</button></form></section>
    <section class="panel preferences account-panel"><h2>Your account</h2><div class="setting-row"><div><strong>${escape(state.account?.display_name || "Not signed in")}</strong><p>${state.account ? `${escape(state.account.access.level)} · ${escape(state.account.access.status)}` : "Sign in to see the guides available to you."}</p></div>${state.account ? `<button class="secondary" data-link="account">Manage subscription ${icon("external")}</button>` : `<button class="secondary" data-page="guides">Sign in</button>`}</div>${accountDetails()}<p class="input-hint">Guide access and subscriptions are managed by Zygor.</p></section>`
  );
}

function accountDetails() {
  if (!state.account) return "";
  const access = state.account.access;
  const rows = [
    ["Plan", access.plan_name],
    ["Access expires", access.elite_expiry_hr],
    ["Next renewal", access.rebill_date_hr],
  ].filter(([, value]) => value);
  return rows.length
    ? `<dl class="account-details">${rows.map(([label, value]) => `<div><dt>${label}</dt><dd>${escape(value)}</dd></div>`).join("")}</dl>`
    : "";
}
function activityPage() {
  return (
    heading(
      "Activity",
      `<button class="secondary" data-action="clear-log" ${disabled()}>Clear activity</button>`,
    ) +
    `<section class="panel activity-panel">${logs.length ? logs.map((item) => `<div class="log-entry ${item.failed ? "failed" : ""}">${activityTime(item.timestamp_ms)}<span>${icon(item.failed ? "activity" : "check")}</span><p>${escape(item.message)}</p></div>`).join("") : `<div class="empty-state">${icon("activity")}<h2>No activity yet</h2></div>`}</section>`
  );
}

function renderProgress() {
  const area = document.querySelector("#download-status");
  if (!area || !progress) return;
  const percent = progress.total
    ? Math.min(100, Math.round((progress.current / progress.total) * 100))
    : null;
  area.innerHTML = `<div class="progress-top"><div><strong>${escape(progress.phase)}</strong><small>${progress.phase === "Downloading guides" ? `${(progress.current / 1048576).toFixed(1)} MB${progress.total ? ` / ${(progress.total / 1048576).toFixed(1)} MB` : ""}` : "Your existing guides stay available until the update is ready."}</small></div><span>${percent === null ? "" : `${percent}%`}</span><button class="secondary" data-action="cancel" ${progress.cancelling ? "disabled" : ""}>${progress.cancelling ? "Cancelling…" : "Cancel"}</button></div><progress max="100" ${percent === null ? "" : `value="${percent}"`} aria-label="Download progress"></progress>`;
}

async function perform(label, command, args, success) {
  if (busy) return false;
  busy = true;
  busyLabel = label;
  error = "";
  notice = "";
  render();
  try {
    state = await invoke(command, args);
    if (success) {
      notice = success;
      await log(success);
    }
    if (["login", "refresh"].includes(command))
      lastChecked = new Date().toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
      });
    return true;
  } catch (e) {
    error = String(e);
    await log(error, true);
    return false;
  } finally {
    busy = false;
    busyLabel = "";
    progress = null;
    render();
  }
}

async function installOne(id) {
  progress = { phase: "Preparing download", current: 0, total: null };
  return perform(
    "Updating guides",
    "install",
    { id },
    "Guides installed. Type /reload in WoW to load the update.",
  );
}
async function updateAll() {
  const ids = availableUpdates().map((i) => i.id);
  for (const id of ids) {
    if (!(await installOne(id))) break;
  }
}
async function updatePeriodically() {
  if (busy || !state.account || !state.config.update_automatically) return;
  if (await perform("Checking for updates", "refresh")) await updateAll();
}
async function confirmUninstall() {
  const dialog = document.querySelector("#confirm-dialog");
  return new Promise((resolve) => {
    const finish = (result) => {
      dialog.close();
      dialog.oncancel = null;
      resolve(result);
    };
    document.querySelector("#confirm-no").onclick = () => finish(false);
    document.querySelector("#confirm-yes").onclick = () => finish(true);
    dialog.oncancel = (e) => {
      e.preventDefault();
      finish(false);
    };
    dialog.showModal();
  });
}

root.addEventListener("click", async (event) => {
  const button = event.target.closest("button");
  if (!button || button.disabled) return;
  if (button.dataset.page) {
    page = button.dataset.page;
    error = "";
    notice = "";
    render();
    return;
  }
  if (button.dataset.configure) {
    selectedGame = button.dataset.configure;
    page = "directories";
    render();
    return;
  }
  if (button.dataset.link) {
    try {
      await invoke("open_link", { link: button.dataset.link });
    } catch (e) {
      error = String(e);
      render();
    }
    return;
  }
  const id = button.dataset.id;
  switch (button.dataset.action) {
    case "dismiss-error":
      error = "";
      render();
      break;
    case "logout":
      await perform("Signing out", "logout", {}, "Signed out of Zygor.");
      break;
    case "refresh":
      await perform(
        "Checking for updates",
        "refresh",
        {},
        "Guide library refreshed.",
      );
      break;
    case "detect": {
      const before = state.installations.length;
      if (
        await perform("Looking for WoW installations", "detect_installations")
      ) {
        notice =
          state.installations.length > before
            ? `Connected ${state.installations.length - before} installation(s).`
            : "No additional installations found. You can add a folder manually.";
        await log(notice);
        render();
      }
      break;
    }
    case "browse": {
      try {
        const path = await invoke("choose_directory");
        if (path) {
          directory = path;
          render();
        }
      } catch (e) {
        error = String(e);
        render();
      }
      break;
    }
    case "remove-folder":
      await perform(
        "Removing folder",
        "remove_installation",
        { id },
        "Folder disconnected. Game files were left in place.",
      );
      break;
    case "install":
      await installOne(id);
      break;
    case "update-all": {
      await updateAll();
      break;
    }
    case "uninstall":
      if (await confirmUninstall())
        await perform(
          "Uninstalling guides",
          "uninstall",
          { id },
          "Guides uninstalled. A recovery copy was retained.",
        );
      break;
    case "cancel":
      if (progress) {
        progress.cancelling = true;
        renderProgress();
      }
      await invoke("cancel_download");
      break;
    case "clear-log":
      if (busy) break;
      busy = true;
      busyLabel = "Clearing activity";
      render();
      await updateActivity("clear_activity");
      busy = false;
      busyLabel = "";
      render();
      break;
  }
});

root.addEventListener("input", (event) => {
  if (event.target.id === "username") username = event.target.value;
  if (event.target.name === "remember") remember = event.target.checked;
  if (event.target.id === "directory") directory = event.target.value;
  if (event.target.id === "game") selectedGame = event.target.value;
});
root.addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.target,
    values = new FormData(form);
  if (form.id === "login-form") {
    username = String(values.get("username")).trim();
    const password = String(values.get("password"));
    form.elements.password.value = "";
    await perform("Signing in", "login", {
      username,
      password,
      remember: values.has("remember"),
    });
  } else if (form.id === "folder-form") {
    selectedGame = String(values.get("game"));
    directory = String(values.get("path"));
    await perform(
      "Connecting game folder",
      "add_installation",
      { game: selectedGame, path: directory },
      "Game folder connected.",
    );
  } else if (form.id === "settings-form") {
    const saved = await perform(
      "Saving preferences",
      "settings",
      {
        region: String(values.get("region")),
        updateAutomatically: values.has("auto"),
      },
      "Preferences saved.",
    );
    if (saved && state.account)
      await perform(
        "Refreshing regional guide releases",
        "refresh",
        {},
        "Preferences saved and guide library refreshed.",
      );
  }
});

async function start() {
  if (!tauri) {
    root.innerHTML =
      '<div class="boot">OpenZygor needs its native desktop backend. Start it with <code>cargo run</code> from the project folder.</div>';
    return;
  }
  await updateActivity("load_activity");
  try {
    await tauri.event.listen("download-progress", (event) => {
      progress = event.payload;
      const area = document.querySelector("#download-status");
      if (area) area.hidden = false;
      renderProgress();
    });
    state = await invoke("initialize");
    if (!state.config.installations.length)
      state = await invoke("detect_installations");
    render();
  } catch (e) {
    error = String(e);
    render();
  }
  setInterval(updatePeriodically, 30 * 60 * 1000);
}
start();
