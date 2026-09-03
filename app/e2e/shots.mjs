// Capture screenshots of the real app, for looking at the design rather than
// asserting on it. Writes PNGs next to this file.
//
//   node e2e/shots.mjs [outdir]
//
// Like run.mjs it stops well before anything is launched: it only walks the two
// screens that were designed.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { execFileSync } from "node:child_process";
import { newSession, sleep, waitForDriver } from "./webdriver.mjs";

const APP = join(process.cwd(), "..", "target", "release", "herdup-app.exe");
const TAURI_DRIVER = join(process.env.USERPROFILE ?? "", ".cargo", "bin", "tauri-driver.exe");
const EDGE_DRIVER = join(
  process.env.LOCALAPPDATA ?? "",
  "Programs",
  "msedgedriver",
  "msedgedriver.exe",
);
const OUT = process.argv[2] ?? join(process.cwd(), "e2e", "shots");

let driver = null;
let session = null;

/// Where the demo project lives.
///
/// Deliberately NOT a temp directory: the path is visible in every screenshot,
/// and `%TEMP%` on Windows contains the account name — publishing that on a
/// landing page leaks the username and reads like a scratch folder. A sibling
/// of the repo gives a short, ordinary-looking path with nothing personal in
/// it. Never reuses an existing folder, and only removes what it created.
function demoPath() {
  const parent = resolve(process.cwd(), '..', '..'); // the folder holding the repo, e.g. D:\work
  for (const name of ["my-app", "my-app-demo", "herdup-demo-project"]) {
    const candidate = join(parent, name);
    if (!existsSync(candidate)) return candidate;
  }
  throw new Error(`no free demo folder under ${parent}; move or rename my-app*`);
}

function makeCleanRepo() {
  const dir = demoPath();
  mkdirSync(dir, { recursive: true });
  const git = (...args) => execFileSync("git", args, { cwd: dir, stdio: "ignore" });
  git("init", "-q");
  git("config", "user.email", "shots@example.com");
  git("config", "user.name", "shots");
  writeFileSync(join(dir, "README.md"), "# demo\n");
  git("add", ".");
  git("commit", "-qm", "initial");
  return dir;
}

async function shot(name) {
  const b64 = await session.screenshot();
  const file = join(OUT, `${name}.png`);
  writeFileSync(file, Buffer.from(b64, "base64"));
  console.log(`  wrote ${file}`);
}

async function main() {
  if (!existsSync(APP)) throw new Error(`build first: cargo tauri build`);
  mkdirSync(OUT, { recursive: true });

  // Run against an isolated herdr session. A named session gets its own socket
  // AND its own state dir, so the "Already running" list stays empty and these
  // published images can never contain somebody's real project.
  process.env.HERDUP_SESSION = "herdup-shots";

  driver = spawn(TAURI_DRIVER, ["--native-driver", EDGE_DRIVER], { stdio: "ignore" });
  await waitForDriver();
  session = await newSession(APP);
  await sleep(2500);

  const repo = makeCleanRepo();

  // The app remembers recent projects per machine. Anything left by an earlier
  // run would show up in the list — and these images get published, so start
  // from an empty list every time.
  try {
    await session.execute("localStorage.clear(); location.reload();");
    await sleep(2500);
  } catch {
    /* older driver without execute — the list is usually empty anyway */
  }

  await shot("1-project-empty");

  const input = await session.waitFor('[data-testid="project-input"]');
  await input.clear();
  await input.sendKeys(repo);
  await sleep(1200);
  await shot("2-project-chosen");

  await (await session.waitFor('[data-testid="project-next"]')).click();
  await session.waitFor('[data-testid="step-team"]');
  await sleep(1200);
  await shot("3-team-squad");

  await (await session.waitFor('[data-testid="template-full-team"]')).click();
  await sleep(1000);
  await shot("4-team-full");

  await (await session.waitFor('[data-testid="drop-1"]')).click();
  await sleep(1000);
  await shot("5-team-after-removal");

  rmSync(repo, { recursive: true, force: true });
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
