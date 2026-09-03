// Measure how tall each screen actually is, so the window can be sized to the
// content instead of guessed at.
//
//   node e2e/measure.mjs [width]
//
// Reports the content height of every state at the given webview width,
// including the tall ones (warnings, six teammates, running teams).

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { newSession, sleep, waitForDriver } from "./webdriver.mjs";

const APP = join(process.cwd(), "..", "target", "release", "herdup-app.exe");
const TAURI_DRIVER = join(process.env.USERPROFILE ?? "", ".cargo", "bin", "tauri-driver.exe");
const EDGE_DRIVER = join(
  process.env.LOCALAPPDATA ?? "",
  "Programs",
  "msedgedriver",
  "msedgedriver.exe",
);
const WIDTH = Number(process.argv[2] ?? 700);

let driver = null;
let session = null;

function makeRepo({ git: withGit = true, dirty = false } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "herdup-measure-"));
  mkdirSync(join(dir, "src"), { recursive: true });
  writeFileSync(join(dir, "README.md"), "# demo\n");
  if (withGit) {
    const g = (...a) => execFileSync("git", a, { cwd: dir, stdio: "ignore" });
    g("init", "-q");
    g("config", "user.email", "m@example.com");
    g("config", "user.name", "m");
    g("add", ".");
    g("commit", "-qm", "initial");
    if (dirty) writeFileSync(join(dir, "src", "a.txt"), "changed\n");
  }
  return dir;
}

/// Content height = how tall the window's webview would need to be to avoid
/// scrolling. `.app` is the only block, so its height plus its own margin box.
const MEASURE = `
  const app = document.querySelector('.app');
  const r = app.getBoundingClientRect();
  return Math.ceil(r.height) + '|' + Math.ceil(app.scrollWidth) + '|' + window.innerHeight;
`;

const rows = [];
async function record(label) {
  await sleep(700);
  const [h, w, view] = (await session.execute(MEASURE)).split("|").map(Number);
  rows.push({ label, contentH: h, contentW: w, viewH: view, scrolls: h > view });
}

async function setProject(path) {
  const input = await session.waitFor('[data-testid="project-input"]');
  await input.clear();
  await input.sendKeys(path);
}

async function main() {
  if (!existsSync(APP)) throw new Error("build first: cargo tauri build");
  driver = spawn(TAURI_DRIVER, ["--native-driver", EDGE_DRIVER], { stdio: "ignore" });
  await waitForDriver();
  session = await newSession(APP);
  await sleep(2500);
  await session.setWindowRect(WIDTH, 900); // tall on purpose so nothing clips
  await sleep(700);

  const clean = makeRepo();
  const nogit = makeRepo({ git: false });
  const dirty = makeRepo({ dirty: true });

  await record("1 project — nothing chosen");

  await setProject(clean);
  await record("1 project — clean folder (quiet)");

  await setProject(nogit);
  await record("1 project — no version history (warns)");

  await setProject(dirty);
  await record("1 project — uncommitted changes (warns)");

  await setProject(clean);
  await (await session.waitFor('[data-testid="project-next"]')).click();
  await session.waitFor('[data-testid="step-team"]');
  await record("2 team — 4 teammates");

  await (await session.waitFor('[data-testid="template-full-team"]')).click();
  await record("2 team — 6 teammates");

  await (await session.waitFor('[data-testid="template-solo"]')).click();
  await record("2 team — 1 teammate");

  await (await session.waitFor('[data-testid="template-full-team"]')).click();
  await (await session.waitFor('[data-testid="team-next"]')).click();
  await session.waitFor('[data-testid="step-preflight"]');
  await record("3 check — clean");

  console.log(`\nwebview width ${WIDTH}px\n`);
  const pad = (s, n) => String(s).padEnd(n);
  console.log(`${pad("state", 42)} ${pad("content h", 10)} widest  scrolls?`);
  for (const r of rows) {
    console.log(
      `${pad(r.label, 42)} ${pad(r.contentH, 10)} ${pad(r.contentW, 7)} ${r.scrolls ? "YES" : "no"}`,
    );
  }
  const tallest = rows.reduce((a, b) => (b.contentH > a.contentH ? b : a));
  console.log(`\ntallest: ${tallest.contentH}px — "${tallest.label}"`);

  for (const d of [clean, nogit, dirty]) rmSync(d, { recursive: true, force: true });
}

main()
  .catch((e) => {
    console.error(`\nfailed: ${e.message}`);
    process.exitCode = 1;
  })
  .finally(async () => {
    if (session) await session.deleteSession();
    if (driver?.pid) {
      try {
        execFileSync("taskkill", ["/PID", String(driver.pid), "/T", "/F"], { stdio: "ignore" });
      } catch {
        /* already gone */
      }
    }
  });
