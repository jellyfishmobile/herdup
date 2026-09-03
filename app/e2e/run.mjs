// herdup GUI end-to-end tests.
//
// Runs the real app under tauri-driver and drives it by `data-testid` selector.
//
// SAFETY: these tests deliberately never complete a launch. They exercise the
// wiring — screen navigation, IPC, plan preview, preflight — and then assert the
// GUARDRAILS hold, which by definition creates nothing. That is not a
// limitation; launching real agents is Phase 6's job and is verified there.
//
// Run with:  npm run test:e2e

import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { newSession, waitForDriver, sleep } from "./webdriver.mjs";

// Prefer the release binary: a debug Tauri build points at the Vite dev server,
// while release embeds `dist/`, so release is the artifact users actually get.
const RELEASE = resolve(process.cwd(), "../target/release/herdup-app.exe");
const DEBUG = resolve(process.cwd(), "../target/debug/herdup-app.exe");
const APP = existsSync(RELEASE) ? RELEASE : DEBUG;

const EDGE_DRIVER =
  process.env.EDGE_DRIVER ??
  join(
    process.env.LOCALAPPDATA ?? "",
    "Programs",
    "msedgedriver",
    "msedgedriver.exe",
  );

const TAURI_DRIVER =
  process.env.TAURI_DRIVER ??
  join(process.env.USERPROFILE ?? "", ".cargo", "bin", "tauri-driver.exe");

/// Kill a process *and its children*.
///
/// tauri-driver starts msedgedriver itself, and killing only the parent leaves
/// it running. On Windows `taskkill /T` is the reliable way to take the tree.
function killTree(pid) {
  if (!pid) return;
  try {
    if (process.platform === "win32") {
      execFileSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
    } else {
      process.kill(-pid, "SIGKILL");
    }
  } catch {
    /* already gone */
  }
}
const results = [];
let driver;
let session;

function check(name, condition, detail = "") {
  results.push({ name, ok: !!condition, detail });
  console.log(`  ${condition ? "ok  " : "FAIL"} ${name}${detail ? `  — ${detail}` : ""}`);
}

/// A throwaway git repo, so the "no version control" warning does not fire and
/// the project-exists check passes.
function makeCleanRepo() {
  const dir = mkdtempSync(join(tmpdir(), "herdup-e2e-clean-"));
  const git = (...args) =>
    execFileSync("git", args, { cwd: dir, stdio: "ignore" });
  git("init", "-q");
  git("config", "user.email", "e2e@example.com");
  git("config", "user.name", "e2e");
  writeFileSync(join(dir, "README.md"), "# e2e\n");
  git("add", "-A");
  git("commit", "-q", "-m", "initial");
  return dir;
}

function makeUnversionedFolder() {
  const dir = mkdtempSync(join(tmpdir(), "herdup-e2e-nogit-"));
  mkdirSync(join(dir, "src"), { recursive: true });
  return dir;
}

async function setProject(path) {
  const input = await session.waitFor('[data-testid="project-input"]');
  await input.clear();
  await input.sendKeys(path);
  return input;
}

/// herdr's vocabulary must never reach the screen. `<code>` is exempt: the last
/// screen deliberately offers the raw `herdr --session …` command as an escape
/// hatch for people who do want it.
const BANNED = [
  [/\bpanes?\b/i, "pane"],
  [/\bw\d+:[pt]\d+\b/i, "a raw pane id like w1:p1"],
  [/\bbriefings?\b/i, "briefing"],
  [/\bworkspaces?\b/i, "workspace"],
  [/\btemplates?\b/i, "template"],
];

async function checkVocabulary(where) {
  const text = await session.execute(`
    const main = document.querySelector('main');
    if (!main) return '';
    const clone = main.cloneNode(true);
    for (const c of clone.querySelectorAll('code')) c.remove();
    return clone.innerText || '';
  `);
  // "Your workspace" is the caption over the picture of the team; it is the one
  // survivor, and it means the ordinary English word.
  const cleaned = String(text).replace(/your workspace/gi, "");
  const hit = BANNED.find(([re]) => re.test(cleaned));
  check(
    `no herdr vocabulary on the ${where}`,
    !hit,
    hit ? `found ${hit[1]}` : "",
  );
}

async function main() {
  console.log(`app:    ${APP}`);
  console.log(`driver: ${EDGE_DRIVER}`);
  if (!existsSync(APP)) throw new Error(`build the app first: cargo build --release -p herdup-app`);
  if (!existsSync(EDGE_DRIVER)) throw new Error(`msedgedriver not found at ${EDGE_DRIVER}`);

  console.log("starting tauri-driver…");
  // No `shell: true`. It wraps the spawn in cmd.exe, so killing the returned
  // handle leaves the real tauri-driver — and the msedgedriver it starts —
  // running after the test. It also triggers Node's DEP0190 warning.
  driver = spawn(TAURI_DRIVER, ["--native-driver", EDGE_DRIVER], { stdio: "ignore" });
  await waitForDriver();

  console.log("launching herdup…");
  session = await newSession(APP);
  await sleep(2500); // let the webview finish its first render

  // -- 1. the app starts on the project step ------------------------------
  console.log("\n[1] first screen");
  const onProject = await session.waitFor('[data-testid="step-project"]');
  check("app opens on the project step", !!onProject);
  const next = await session.waitFor('[data-testid="project-next"]');
  check("Next is disabled with no project chosen", !(await next.enabled()));

  // -- 2. a path that does not exist is blocked ---------------------------
  console.log("\n[2] a project path that does not exist");
  await setProject("D:\\definitely\\not\\here");
  await (await session.waitFor('[data-testid="project-next"]')).click();
  await session.waitFor('[data-testid="step-team"]');
  await (await session.waitFor('[data-testid="team-next"]')).click();

  const blocking = await session.waitFor('[data-testid="blocking"]');
  const blockingText = await blocking.text();
  check(
    "preflight blocks a non-existent project",
    blockingText.includes("does not exist"),
    blockingText.replace(/\s+/g, " ").slice(0, 90),
  );
  const blockedButton = await session.waitFor('[data-testid="preflight-next"]');
  check("launch button is disabled while blocked", !(await blockedButton.enabled()));

  // -- 3. an unversioned folder warns and gates ---------------------------
  console.log("\n[3] a folder with no version control");
  const nogit = makeUnversionedFolder();
  await (await session.waitFor('[data-testid="back"]')).click(); // Back
  await session.waitFor('[data-testid="step-team"]');
  // Back again to the project step.
  await (await session.waitFor('[data-testid="back"]')).click();
  await setProject(nogit);
  await (await session.waitFor('[data-testid="project-next"]')).click();
  await (await session.waitFor('[data-testid="team-next"]')).click();

  const warning = await session.waitFor('[data-testid="warning-0"]');
  const warningText = await warning.text();
  check(
    "warns that nothing can be undone without git",
    warningText.includes("undo"),
    warningText.replace(/\s+/g, " ").slice(0, 90),
  );

  const gated = await session.waitFor('[data-testid="preflight-next"]');
  check("launch is gated until the warning is acknowledged", !(await gated.enabled()));

  // The launcher is for people who have never heard of herdr, so its own
  // vocabulary must never reach the screen (DESIGN.md). This caught raw pane
  // ids like "w1:p1" and the phrase "pane ready" being dumped into the UI.
  await checkVocabulary("check screen");

  const ack = await session.waitFor('[data-testid="ack-0"]');
  await ack.click();
  await sleep(300);
  const ungated = await session.waitFor('[data-testid="preflight-next"]');
  check("acknowledging the warning enables launch", await ungated.enabled());

  // -- 4. a clean repo produces no warnings -------------------------------
  console.log("\n[4] a clean git repository");
  const clean = makeCleanRepo();
  await (await session.waitFor('[data-testid="back"]')).click();
  await session.waitFor('[data-testid="step-team"]');
  await (await session.waitFor('[data-testid="back"]')).click();
  await setProject(clean);
  await (await session.waitFor('[data-testid="project-next"]')).click();

  // -- 5. template selection changes the plan ------------------------------
  console.log("\n[5] template selection");
  await (await session.waitFor('[data-testid="template-solo"]')).click();
  await sleep(600);
  const teamBody = await (await session.waitFor('[data-testid="step-team"]')).text();
  check("choosing Solo shows a single role", teamBody.includes("Dev"), "");
  await (await session.waitFor('[data-testid="template-full-team"]')).click();
  await sleep(600);
  const fullBody = await (await session.waitFor('[data-testid="step-team"]')).text();
  check(
    "choosing Full team shows the coordinator and all six roles",
    ["PM", "Coder 1", "Coder 2", "QA", "Builds", "Research"].every((r) =>
      fullBody.includes(r),
    ),
  );
  // The word "coordinator" is deliberately gone from the UI — it is herdr
  // vocabulary. The lead is now marked by its tinted lane instead, so assert
  // the thing the user can actually see.
  const leadLane = await session.find("main .lane.lead");
  check("the lead is distinguished from the rest of the team", leadLane !== null);

  await checkVocabulary("team screen");

  // Removing a teammate must drop the one that was pointed at. The compacted
  // index shifts after the first removal, so this is the case that used to
  // silently drop the wrong role.
  const beforeDrop = (await (await session.waitFor('[data-testid="step-team"]')).text()) ?? "";
  await (await session.waitFor('[data-testid="drop-1"]')).click(); // Coder 1
  await sleep(600);
  await (await session.waitFor('[data-testid="drop-2"]')).click(); // QA, after the shift
  await sleep(600);
  const afterDrop = await (await session.waitFor('[data-testid="step-team"]')).text();
  check(
    "removing two teammates removes the two that were pointed at",
    beforeDrop.includes("Coder 1") &&
      !afterDrop.includes("Coder 1") &&
      !afterDrop.includes("QA") &&
      afterDrop.includes("Coder 2") &&
      afterDrop.includes("Builds"),
    afterDrop.replace(/\s+/g, " ").slice(0, 160),
  );

  // Put the team back before the preflight assertions below.
  await (await session.waitFor('[data-testid="template-full-team"]')).click();
  await sleep(600);

  await (await session.waitFor('[data-testid="team-next"]')).click();
  await session.waitFor('[data-testid="step-preflight"]');
  await sleep(400);
  const noWarning = await session.find('[data-testid="warning-0"]');
  check("a clean repo raises no warnings", noWarning === null);
  const ready = await session.waitFor('[data-testid="preflight-next"]');
  check("launch is enabled for a clean, valid project", await ready.enabled());

  // Deliberately stop here. Clicking that button would start real agents.
  console.log("\n  (stopping before launch on purpose — no agents are started)");

  rmSync(nogit, { recursive: true, force: true });
  rmSync(clean, { recursive: true, force: true });
}

/// On failure, say what the app was actually showing. A timeout on a selector
/// is far less useful than the error banner that explains why it never appeared.
async function dumpState() {
  if (!session) return;
  for (const step of ["project", "team", "preflight", "firstrun", "launching", "done"]) {
    if (await session.find(`[data-testid="step-${step}"]`)) {
      console.error(`  current step: ${step}`);
      break;
    }
  }
  const err = await session.find('[data-testid="error"]');
  if (err) {
    console.error(`  app error banner: ${(await err.text()).replace(/\s+/g, " ").slice(0, 300)}`);
  } else {
    console.error("  (no error banner shown)");
  }
}

main()
  .catch(async (e) => {
    console.error(`\nharness error: ${e.message}`);
    await dumpState().catch(() => {});
    results.push({ name: "harness ran to completion", ok: false, detail: e.message });
  })
  .finally(async () => {
    if (session) await session.deleteSession();
    if (driver) killTree(driver.pid);
    await sleep(500);
    const failed = results.filter((r) => !r.ok);
    console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
    if (failed.length) {
      console.log("failed:");
      for (const f of failed) console.log(`  - ${f.name}${f.detail ? `: ${f.detail}` : ""}`);
    }
    process.exit(failed.length ? 1 : 0);
  });
