// A minimal W3C WebDriver client.
//
// Hand-rolled rather than pulling in WebdriverIO or selenium-webdriver: the
// surface needed here is about eight endpoints, and keeping it explicit means
// no framework version has to track Tauri's or Edge's.
//
// Elements are addressed by CSS selector — specifically `data-testid` — never by
// screen position. That is the whole point: an earlier attempt to verify this
// app by clicking screen coordinates drove a real launch into the wrong folder
// because the window had moved between screenshot and click. A selector cannot
// land outside the window.

const BASE = "http://127.0.0.1:4444";

async function call(method, path, body) {
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let json;
  try {
    json = text ? JSON.parse(text) : {};
  } catch {
    throw new Error(`${method} ${path}: non-JSON response: ${text.slice(0, 200)}`);
  }
  if (!res.ok) {
    const err = json?.value?.error ?? res.status;
    const msg = json?.value?.message ?? text;
    throw new Error(`${method} ${path} failed (${err}): ${String(msg).split("\n")[0]}`);
  }
  return json.value;
}

export async function newSession(application, args = []) {
  const value = await call("POST", "/session", {
    capabilities: {
      alwaysMatch: {
        "tauri:options": { application, args },
      },
    },
  });
  // tauri-driver returns the session id at the top level of `value` in some
  // versions and beside it in others; accept either.
  const id = value?.sessionId ?? value?.capabilities?.sessionId;
  if (!id) throw new Error(`no sessionId in response: ${JSON.stringify(value).slice(0, 300)}`);
  return new Session(id);
}

class Session {
  constructor(id) {
    this.id = id;
  }

  #p(path) {
    return `/session/${this.id}${path}`;
  }

  async deleteSession() {
    try {
      await call("DELETE", this.#p(""));
    } catch {
      /* the window may already be gone */
    }
  }

  /// Find one element by CSS selector, or null.
  async find(selector) {
    try {
      const value = await call("POST", this.#p("/element"), {
        using: "css selector",
        value: selector,
      });
      const ref = Object.values(value)[0];
      return new Element(this, ref);
    } catch {
      return null;
    }
  }

  /// Wait until a selector appears, then return it.
  async waitFor(selector, { timeout = 15000, interval = 200 } = {}) {
    const deadline = Date.now() + timeout;
    let last = null;
    while (Date.now() < deadline) {
      last = await this.find(selector);
      if (last) return last;
      await sleep(interval);
    }
    throw new Error(`timed out after ${timeout}ms waiting for ${selector}`);
  }

  async source() {
    return call("GET", this.#p("/source"));
  }

  /// Base64 PNG of the current window, for eyeballing the real app.
  async screenshot() {
    return call("GET", this.#p("/screenshot"));
  }

  /// Run JS in the page and return its value.
  async execute(script, args = []) {
    return call("POST", this.#p("/execute/sync"), { script, args });
  }

  async setWindowRect(width, height) {
    return call("POST", this.#p("/window/rect"), { width, height, x: null, y: null });
  }
}

class Element {
  constructor(session, ref) {
    this.session = session;
    this.ref = ref;
  }

  #p(path) {
    return `/session/${this.session.id}/element/${this.ref}${path}`;
  }

  click() {
    return call("POST", this.#p("/click"), {});
  }

  clear() {
    return call("POST", this.#p("/clear"), {});
  }

  /// Type text. Sent as a single value, so paths with spaces or backslashes
  /// arrive intact — unlike SendKeys, which mangled one into `ppalippali`.
  sendKeys(text) {
    return call("POST", this.#p("/value"), { text });
  }

  text() {
    return call("GET", this.#p("/text"));
  }

  async enabled() {
    return call("GET", this.#p("/enabled"));
  }

  async selected() {
    return call("GET", this.#p("/selected"));
  }
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function waitForDriver({ timeout = 20000 } = {}) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    try {
      await fetch(`${BASE}/status`);
      return;
    } catch {
      await sleep(200);
    }
  }
  throw new Error("tauri-driver did not become reachable on :4444");
}
