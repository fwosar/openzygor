const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { setImmediate: nextTurn } = require("node:timers/promises");
const { test } = require("node:test");
const vm = require("node:vm");

const source = readFileSync(join(__dirname, "../ui/app.js"), "utf8");
const editions = [
  "wow",
  "wow-classic",
  "wow-classic-tbc",
  "wow-classic-tbc-anniv",
  "wow-forever",
];

function installation(id, game, version = "1", exists = true) {
  return {
    id,
    game,
    installed_version: version,
    exists,
    path: `/fixture/${id}`,
  };
}

function fixture() {
  const installations = [
    installation("retail", "wow"),
    installation("classic", "wow-classic"),
  ];
  return {
    config: { region: "eu", update_automatically: true, installations },
    account: { display_name: "Fixture", access: { level: "elite" } },
    packages: editions.map((game) => ({
      game,
      ver: "2",
      status: { available: true, hidden: false },
    })),
    installations,
  };
}

// Run the actual frontend and its event handlers without a display or service.
async function client(
  initial = fixture(),
  beforeCommand = async () => {},
  history = { entries: [] },
) {
  const data = structuredClone(initial);
  const calls = [];
  const activityCalls = [];
  const handlers = {};
  const timers = [];
  const root = {
    innerHTML: "",
    addEventListener: (event, handler) => (handlers[event] = handler),
  };
  const progress = { innerHTML: "", hidden: true };
  const context = vm.createContext({
    document: {
      querySelector: (selector) =>
        selector === "#app"
          ? root
          : selector === "#download-status"
            ? progress
            : null,
    },
    window: {
      __TAURI__: {
        event: { listen: async () => {} },
        core: {
          invoke: async (command, args) => {
            const activityCommand = command.endsWith("_activity");
            (activityCommand ? activityCalls : calls).push({
              command,
              args: structuredClone(args),
            });
            await beforeCommand(command, args, data);
            if (activityCommand) {
              if (command === "append_activity") {
                history.entries.unshift({
                  ...structuredClone(args),
                  timestamp_ms: Date.now(),
                });
                history.entries = history.entries.slice(0, 100);
              } else if (command === "clear_activity") {
                history.entries = [];
              }
              return structuredClone(history.entries);
            }
            if (command === "install") {
              const item = data.installations.find((i) => i.id === args.id);
              item.installed_version = data.packages.find(
                (p) => p.game === item.game,
              ).ver;
            } else if (command === "settings") {
              data.config.region = args.region;
              data.config.update_automatically = args.updateAutomatically;
            }
            return structuredClone(data);
          },
        },
      },
    },
    setInterval: (callback, delay) => timers.push({ callback, delay }),
    FormData: class {
      constructor(form) {
        this.values = form.values;
      }
      get(key) {
        return this.values[key];
      }
      has(key) {
        return Object.hasOwn(this.values, key);
      }
    },
  });
  vm.runInContext(source, context, { filename: "ui/app.js" });
  await nextTurn();
  assert.equal(
    timers.length,
    1,
    "Frontend initialized and scheduled its timer",
  );
  assert.equal(timers[0].delay, 30 * 60 * 1000);
  calls.length = 0;
  return {
    calls,
    activityCalls,
    history,
    data,
    get html() {
      return root.innerHTML;
    },
    tick: () => timers[0].callback(),
    click: (dataset) =>
      handlers.click({
        target: { closest: () => ({ dataset, disabled: false }) },
      }),
    save: (values) =>
      handlers.submit({
        preventDefault() {},
        target: { id: "settings-form", values },
      }),
  };
}

const installedIds = (app) =>
  app.calls.filter((c) => c.command === "install").map((c) => c.args.id);
const titles = (app) =>
  [...app.html.matchAll(/class="guide-title"><h2>([^<]+)/g)].map((m) => m[1]);

test("installed editions come first, with stable ordering within each group", async () => {
  const data = fixture();
  data.installations = [
    installation("retail", "wow", null),
    installation("classic", "wow-classic"),
    installation("anniversary-empty", "wow-classic-tbc-anniv", null),
    installation("anniversary", "wow-classic-tbc-anniv"),
  ];
  const app = await client(data);
  assert.deepEqual(titles(app), [
    "Classic Era",
    "TBC Anniversary",
    "World of Warcraft",
    "Mists of Pandaria",
    "WoW Forever",
  ]);
  await app.click({ action: "install", id: "retail" });
  assert.deepEqual(titles(app), [
    "World of Warcraft",
    "Classic Era",
    "TBC Anniversary",
    "Mists of Pandaria",
    "WoW Forever",
  ]);
});

test("periodic updates use fresh releases and only update eligible installed copies", async () => {
  const data = fixture();
  data.packages.forEach((p) => (p.ver = "1"));
  data.packages[1].status.available = false;
  data.packages[2].status.hidden = true;
  data.installations = [
    installation("retail", "wow"),
    installation("retail-copy", "wow"),
    installation("fresh", "wow", null),
    installation("missing", "wow", "1", false),
    installation("current", "wow", "2"),
    installation("unavailable", "wow-classic"),
    installation("hidden", "wow-classic-tbc"),
    installation("anniversary", "wow-classic-tbc-anniv"),
    installation("unknown-version", "wow-forever"),
  ];
  const app = await client(data, async (command, args, data) => {
    if (command === "refresh") {
      data.packages.forEach((p) => (p.ver = "2"));
      data.packages[4].ver = null;
    }
  });
  await app.tick();
  assert.equal(app.calls[0].command, "refresh");
  assert.deepEqual(installedIds(app), ["retail", "retail-copy", "anniversary"]);
  assert.equal(
    app.data.installations.find((i) => i.id === "fresh").installed_version,
    null,
  );
  assert.ok(!titles(app).includes("Mists of Pandaria"));
  app.calls.length = 0;
  await app.tick();
  assert.deepEqual(
    app.calls.map((c) => c.command),
    ["refresh"],
  );
});

test("manual checks do not install; Update all uses the same eligibility rules", async () => {
  const data = fixture();
  data.packages[1].status.hidden = true;
  const app = await client(data);
  await app.click({ action: "refresh" });
  assert.deepEqual(
    app.calls.map((c) => c.command),
    ["refresh"],
  );
  await app.click({ action: "update-all" });
  assert.deepEqual(installedIds(app), ["retail"]);
});

test("periodic updates skip disabled preferences and signed-out sessions", async () => {
  for (const change of [
    (data) => (data.config.update_automatically = false),
    (data) => (data.account = null),
  ]) {
    const data = fixture();
    change(data);
    const app = await client(data);
    await app.tick();
    assert.deepEqual(app.calls, []);
  }
});

test("saving preferences controls subsequent automatic updates", async () => {
  const data = fixture();
  data.config.update_automatically = false;
  const app = await client(data);
  await app.click({ page: "settings" });
  await app.save({ region: "eu", auto: "on" });
  assert.deepEqual(app.calls[0], {
    command: "settings",
    args: { region: "eu", updateAutomatically: true },
  });
  assert.match(app.html, /id="auto-update"[^>]*checked/);
  await app.tick();
  assert.deepEqual(installedIds(app), ["retail", "classic"]);
  await app.save({ region: "eu" });
  assert.doesNotMatch(app.html, /id="auto-update"[^>]*checked/);
  app.calls.length = 0;
  await app.tick();
  assert.deepEqual(app.calls, []);
});

test("a failed refresh prevents installation and allows a later retry", async () => {
  let fail = true;
  const app = await client(fixture(), async (command) => {
    if (command === "refresh" && fail) throw new Error("Refresh failed");
  });
  await app.tick();
  assert.deepEqual(installedIds(app), []);
  assert.match(app.html, /role="alert".*Refresh failed/);
  fail = false;
  await app.tick();
  assert.deepEqual(installedIds(app), ["retail", "classic"]);
});

test("automatic batches do not overlap and cancellation stops remaining updates", async () => {
  let rejectDownload;
  const downloading = new Promise((_, reject) => (rejectDownload = reject));
  let started;
  const ready = new Promise((resolve) => (started = resolve));
  const app = await client(fixture(), async (command) => {
    if (command === "install") {
      started();
      await downloading;
    }
    if (command === "cancel_download")
      rejectDownload(new Error("Download cancelled"));
  });
  const batch = app.tick();
  await ready;
  await app.tick();
  assert.deepEqual(
    app.calls.map((c) => c.command),
    ["refresh", "install"],
  );
  await app.click({ action: "cancel" });
  await batch;
  assert.deepEqual(installedIds(app), ["retail"]);
  assert.match(app.html, /role="alert".*Download cancelled/);
  assert.equal(app.data.installations[0].installed_version, "1");
});

test("activity loads on restart and clearing removes persisted history", async () => {
  const app = await client(fixture(), async (command) => {
    if (command === "install") throw new Error("Download failed");
  });
  await app.click({ action: "refresh" });
  await app.click({ action: "install", id: "retail" });
  assert.equal(app.history.entries.length, 2);
  assert.equal(app.history.entries[0].failed, true);
  assert.equal(app.history.entries[1].failed, false);
  const restarted = await client(fixture(), undefined, app.history);
  await restarted.click({ page: "activity" });
  assert.match(restarted.html, /Download failed/);
  assert.match(restarted.html, /Guide library refreshed/);
  assert.match(restarted.html, /<time datetime="\d{4}-\d{2}-\d{2}T/);
  await restarted.click({ action: "clear-log" });
  const cleared = await client(fixture(), undefined, app.history);
  await cleared.click({ page: "activity" });
  assert.match(cleared.html, /No activity yet/);
  assert.deepEqual(app.history.entries, []);
});

test("activity write errors preserve operation results and show unsaved entries", async () => {
  const app = await client(fixture(), async (command) => {
    if (command === "append_activity") throw new Error("Disk full");
    if (command === "install") throw new Error("Download failed");
  });
  await app.click({ action: "refresh" });
  assert.match(app.html, /banner success/);
  assert.match(app.html, /Could not save activity: Error: Disk full/);
  await app.click({ action: "install", id: "retail" });
  assert.match(app.html, /role="alert".*Download failed/);
  assert.match(app.html, /Could not save activity: Error: Disk full/);
  await app.click({ page: "activity" });
  assert.match(app.html, /log-entry failed/);
  assert.match(app.html, /Guide library refreshed/);
  assert.deepEqual(app.history.entries, []);
});

test("failed clears retain history and can be retried", async () => {
  let fail = true;
  const app = await client(fixture(), async (command) => {
    if (command === "clear_activity" && fail)
      throw new Error("Read only filesystem");
  });
  await app.click({ action: "refresh" });
  await app.click({ page: "activity" });
  await app.click({ action: "clear-log" });
  assert.match(app.html, /Could not clear activity/);
  assert.equal(app.history.entries.length, 1);
  assert.match(app.html, /Guide library refreshed/);
  fail = false;
  await app.click({ action: "clear-log" });
  assert.deepEqual(app.history.entries, []);
  assert.doesNotMatch(app.html, /Could not clear activity/);
});

test("unreadable history does not prevent launching the sign-in screen", async () => {
  const data = fixture();
  data.account = null;
  const app = await client(data, async (command) => {
    if (command === "load_activity")
      throw new Error("Activity history is damaged");
  });
  assert.match(app.html, /id="login-form"/);
  assert.match(
    app.html,
    /Could not load activity: Error: Activity history is damaged/,
  );
});
