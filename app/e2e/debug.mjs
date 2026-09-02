// Diagnostic: start the app under tauri-driver and dump what the webview holds.
import { spawn } from "node:child_process";
import { join, resolve } from "node:path";
import { existsSync } from "node:fs";
import { newSession, waitForDriver, sleep } from "./webdriver.mjs";

const APP = resolve(process.cwd(), "../target/release/herdup-app.exe");
const EDGE_DRIVER =
  process.env.EDGE_DRIVER ??
  join(process.env.LOCALAPPDATA ?? "", "Programs", "msedgedriver", "msedgedriver.exe");

console.log("app exists:", existsSync(APP));
const driver = spawn("tauri-driver", ["--native-driver", EDGE_DRIVER], { stdio: "ignore", shell: true });
await waitForDriver();

let session;
try {
  session = await newSession(APP);
  console.log("session:", session.id);
  for (const wait of [1000, 3000, 5000]) {
    await sleep(wait);
    const src = await session.source().catch((e) => `<source failed: ${e.message}>`);
    console.log(`\n--- after ${wait}ms, source length ${String(src).length} ---`);
    console.log(String(src).slice(0, 1200));
    if (String(src).includes("data-testid")) {
      console.log("\n>>> data-testid IS present");
      break;
    }
  }
} catch (e) {
  console.error("failed:", e.message);
} finally {
  if (session) await session.deleteSession();
  driver.kill();
  await sleep(400);
  process.exit(0);
}
