// The solution as an isometric factory line.
//
// Flat-vector isometric in the style of assembly-line illustrations: a conveyor
// belt running a closed circuit, boxes riding it, machines beside it, a figure
// at each station. Drawn rather than generated because the picture has to be
// ACCURATE — six stations, correct order, and honest about which one exists.
//
// The argument it makes: what the client sends is messy — boxes of different
// sizes, unevenly spaced, jittering on the belt. The PM is the hinge. What
// leaves it is uniform, evenly spaced and calm. Same belt, two substances.
//
// Projection: px = (x - y) * W/2 ; py = (x + y) * H/2 - z

(function () {
  const W = 118;
  const H = 59;
  const NS = "http://www.w3.org/2000/svg";
  const iso = (x, y, z = 0) => ({ px: (x - y) * (W / 2), py: (x + y) * (H / 2) - z });
  const P = (p) => `${p.px.toFixed(1)},${p.py.toFixed(1)}`;

  const el = (n, a = {}) => {
    const e = document.createElementNS(NS, n);
    for (const [k, v] of Object.entries(a)) e.setAttribute(k, String(v));
    return e;
  };
  const poly = (g, pts, cls) =>
    g.appendChild(el("polygon", { class: cls, points: pts.map(P).join(" ") }));

  // ---- the belt route: a closed circuit -----------------------------------
  const ROUTE = [
    [0, 0],
    [6, 0],
    [6, 4],
    [0, 4],
  ];
  const BELT_W = 0.62;
  const BELT_H = 13;
  const PERIM = 20; // 6 + 4 + 6 + 4
  const PM_T = 6 / PERIM; // the PM sits at the end of the first edge

  const STATIONS = [
    { at: [0, 0], side: [-1, -1], title: "Your client", sub: "WhatsApp · Telegram · Slack\nemail · shared docs", state: "planned", kind: "desk" },
    { at: [3, 0], side: [0, -1], title: "Client channel", sub: "one inbox, no AI\na deterministic pipe", state: "planned", kind: "machine" },
    { at: [6, 0], side: [1, -1], title: "herdupd", sub: "turns chat into scoped work", state: "planned", kind: "hub" },
    { at: [6, 4], side: [1, 1], title: "The team", sub: "PM · coders · QA\nin herdr panes", state: "shipping", kind: "cells" },
    { at: [3, 4], side: [0, 1], title: "Board & thread", sub: "kanban · PM↔agent traffic", state: "planned", kind: "screen" },
    { at: [0, 4], side: [-1, 1], title: "Executive summary", sub: "status · KPIs · back to you", state: "planned", kind: "desk" },
  ];

  // ---- primitives ---------------------------------------------------------

  /// A parcel on the belt: three faces plus a tape seam, so it reads as a box.
  function box(g, x, y, z, s, cls) {
    const h = s * 190;
    const t = [
      iso(x - s, y - s, z + h), iso(x + s, y - s, z + h),
      iso(x + s, y + s, z + h), iso(x - s, y + s, z + h),
    ];
    const b = [iso(x - s, y + s, z), iso(x + s, y + s, z), iso(x + s, y - s, z)];
    poly(g, [t[3], t[2], b[1], b[0]], "bx-l " + cls);
    poly(g, [t[2], t[1], b[2], b[1]], "bx-r " + cls);
    poly(g, [t[0], t[1], t[2], t[3]], "bx-t " + cls);
    const s1 = iso(x, y - s, z + h);
    const s2 = iso(x, y + s, z + h);
    g.appendChild(el("line", { class: "bx-seam", x1: s1.px, y1: s1.py, x2: s2.px, y2: s2.py }));
  }

  /// A machine: body plus a lit control panel on its near face.
  function machine(g, x, y, w, d, h, cls) {
    const t = [iso(x - w, y - d, h), iso(x + w, y - d, h), iso(x + w, y + d, h), iso(x - w, y + d, h)];
    const b = [iso(x - w, y + d, 0), iso(x + w, y + d, 0), iso(x + w, y - d, 0)];
    poly(g, [t[3], t[2], b[1], b[0]], "mc-l " + cls);
    poly(g, [t[2], t[1], b[2], b[1]], "mc-r " + cls);
    poly(g, [t[0], t[1], t[2], t[3]], "mc-t " + cls);
    const px1 = x - w * 0.66, px2 = x + w * 0.2;
    const pz1 = h * 0.3, pz2 = h * 0.78;
    poly(
      g,
      [iso(px1, y + d, pz2), iso(px2, y + d, pz2), iso(px2, y + d, pz1), iso(px1, y + d, pz1)],
      "mc-panel " + cls,
    );
  }

  /// A worker. Deliberately simple: shadow, body, head.
  function figure(g, x, y) {
    const base = iso(x, y, 0);
    const top = iso(x, y, 32);
    const head = iso(x, y, 44);
    g.appendChild(el("ellipse", { class: "fg-shadow", cx: base.px, cy: base.py, rx: 10, ry: 5 }));
    g.appendChild(
      el("path", {
        class: "fg-body",
        d: `M ${(base.px - 6.5).toFixed(1)} ${base.py.toFixed(1)} L ${(base.px - 5.5).toFixed(1)} ${top.py.toFixed(1)} Q ${base.px.toFixed(1)} ${(top.py - 6).toFixed(1)} ${(base.px + 5.5).toFixed(1)} ${top.py.toFixed(1)} L ${(base.px + 6.5).toFixed(1)} ${base.py.toFixed(1)} Z`,
      }),
    );
    g.appendChild(el("circle", { class: "fg-head", cx: head.px, cy: head.py + 3, r: 6 }));
  }

  function drawBelt(g) {
    const legs = el("g");
    const deck = el("g");
    for (let i = 0; i < ROUTE.length; i++) {
      const [x1, y1] = ROUTE[i];
      const [x2, y2] = ROUTE[(i + 1) % ROUTE.length];
      const dx = x2 - x1, dy = y2 - y1;
      const len = Math.hypot(dx, dy);
      const nx = -dy / len, ny = dx / len;
      const hw = BELT_W / 2;

      const a = { x: x1 + nx * hw, y: y1 + ny * hw };
      const b = { x: x2 + nx * hw, y: y2 + ny * hw };
      const c = { x: x2 - nx * hw, y: y2 - ny * hw };
      const d = { x: x1 - nx * hw, y: y1 - ny * hw };

      poly(deck, [iso(a.x, a.y, BELT_H), iso(b.x, b.y, BELT_H), iso(c.x, c.y, BELT_H), iso(d.x, d.y, BELT_H)], "belt-top");
      poly(deck, [iso(d.x, d.y, BELT_H), iso(c.x, c.y, BELT_H), iso(c.x, c.y, BELT_H - 7), iso(d.x, d.y, BELT_H - 7)], "belt-side");

      const steps = Math.max(2, Math.round(len * 4));
      for (let s = 0; s <= steps; s++) {
        const k = s / steps;
        const mx = x1 + dx * k, my = y1 + dy * k;
        const r1 = iso(mx + nx * hw, my + ny * hw, BELT_H);
        const r2 = iso(mx - nx * hw, my - ny * hw, BELT_H);
        deck.appendChild(el("line", { class: "belt-roller", x1: r1.px, y1: r1.py, x2: r2.px, y2: r2.py }));
        if (s % 4 === 0) {
          const f = iso(mx, my, 0);
          const tp = iso(mx, my, BELT_H - 7);
          legs.appendChild(el("line", { class: "belt-leg", x1: f.px, y1: f.py, x2: tp.px, y2: tp.py }));
        }
      }
    }
    g.appendChild(legs);
    g.appendChild(deck);
  }

  function walker() {
    const segs = [];
    let total = 0;
    for (let i = 0; i < ROUTE.length; i++) {
      const [x1, y1] = ROUTE[i];
      const [x2, y2] = ROUTE[(i + 1) % ROUTE.length];
      const len = Math.hypot(x2 - x1, y2 - y1);
      segs.push({ x1, y1, x2, y2, len, at: total });
      total += len;
    }
    return (t) => {
      const d = (((t % 1) + 1) % 1) * total;
      const s = segs.find((g) => d <= g.at + g.len) ?? segs[segs.length - 1];
      const k = s.len ? (d - s.at) / s.len : 0;
      return { x: s.x1 + (s.x2 - s.x1) * k, y: s.y1 + (s.y2 - s.y1) * k };
    };
  }

  function build(host) {
    const svg = el("svg", { class: "iso-svg", xmlns: NS, role: "img" });
    const ttl = el("title");
    ttl.textContent =
      "Isometric factory diagram: a conveyor circuit carrying requests from your client " +
      "through the client channel to herdupd, which turns them into uniform work for the " +
      "team, then to the board and the executive summary, and back to the client.";
    svg.appendChild(ttl);

    const beltG = el("g");
    const boxG = el("g", { class: "boxes" });
    const propG = el("g");
    drawBelt(beltG);

    STATIONS.slice()
      .sort((a, b) => a.at[0] + a.at[1] - (b.at[0] + b.at[1]))
      .forEach((st) => {
        const g = el("g", { class: "iso-node" });
        const [sx, sy] = st.at;
        const [ox, oy] = st.side;
        const mx = sx + ox * 1.05;
        const my = sy + oy * 1.05;
        const on = st.state === "shipping" ? " on" : "";
        let topH = 36;

        if (st.kind === "cells") {
          [[-0.34, -0.34], [0.34, -0.34], [-0.34, 0.34], [0.34, 0.34]].forEach(([dx, dy], i) =>
            machine(g, mx + dx, my + dy, 0.25, 0.25, 30 - (i % 2) * 6, on),
          );
          topH = 30;
        } else if (st.kind === "hub") {
          machine(g, mx, my, 0.56, 0.56, 52, on);
          topH = 52;
        } else if (st.kind === "screen") {
          machine(g, mx, my, 0.52, 0.34, 34, on);
          topH = 34;
        } else if (st.kind === "desk") {
          machine(g, mx, my, 0.52, 0.4, 22, on);
          topH = 22;
        } else {
          machine(g, mx, my, 0.44, 0.44, 36, on);
        }

        figure(g, sx + ox * 0.42, sy + oy * 0.42);

        const top = iso(mx, my, topH);
        const lift = H / 2 + 22;
        const badge = el("text", { class: "iso-badge " + st.state, x: top.px, y: top.py - lift - 30 });
        badge.textContent = st.state === "shipping" ? "SHIPPING" : "PLANNED";
        g.appendChild(badge);
        const t = el("text", { class: "iso-title", x: top.px, y: top.py - lift - 14 });
        t.textContent = st.title;
        g.appendChild(t);
        st.sub.split("\n").forEach((line, i) => {
          const s = el("text", { class: "iso-sub", x: top.px, y: top.py - lift + 1 + i * 12 });
          s.textContent = line;
          g.appendChild(s);
        });
        propG.appendChild(g);
      });

    svg.appendChild(beltG);
    svg.appendChild(boxG);
    svg.appendChild(propG);
    host.appendChild(svg);

    const bb = svg.getBBox();
    const pad = 30;
    svg.setAttribute("viewBox", `${bb.x - pad} ${bb.y - pad} ${bb.width + pad * 2} ${bb.height + pad * 2}`);
    return { svg, boxG };
  }

  function animate({ boxG }, host) {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    if (window.gsap) {
      gsap.from(host.querySelectorAll(".iso-node"), {
        y: 24, opacity: 0, duration: 0.55, stagger: 0.09, ease: "power3.out",
        immediateRender: false,
        scrollTrigger: { trigger: host, start: "top 74%", once: true },
      });
    }

    const path = walker();
    const cargo = [];
    for (let i = 0; i < 16; i++) {
      const g = el("g");
      boxG.appendChild(g);
      cargo.push({
        g,
        t: i / 16,
        rawSize: 0.09 + Math.random() * 0.1, // every ask a different shape
        rawJitter: Math.random() * 0.14 - 0.07, // and none of them squarely on the belt
        wobble: Math.random() * 6.28,
      });
    }

    const draw = (c, now) => {
      const ordered = c.t > PM_T;
      const size = ordered ? 0.13 : c.rawSize;
      // The mess resolves exactly at the PM, not gradually somewhere after it.
      const jitter = ordered ? 0 : c.rawJitter * (1 - c.t / PM_T);
      const bob = ordered ? 0 : Math.sin(now / 320 + c.wobble) * 1.8;
      const p = path(c.t);
      while (c.g.firstChild) c.g.removeChild(c.g.firstChild);
      box(c.g, p.x + jitter, p.y + jitter, BELT_H + bob, size, ordered ? "ord" : "raw");
    };

    if (reduced) {
      cargo.forEach((c) => draw(c, 0));
      return;
    }

    let running = false, raf = 0, last = performance.now();
    const frame = (now) => {
      const dt = Math.min((now - last) / 1000, 0.05);
      last = now;
      for (const c of cargo) {
        const ordered = c.t > PM_T;
        // Uneven speeds before the PM; one steady cadence after it.
        c.t = (c.t + (ordered ? 0.03 : 0.018 + (c.rawSize - 0.09) * 0.4) * dt) % 1;
        draw(c, now);
      }
      if (running) raf = requestAnimationFrame(frame);
    };

    const io = new IntersectionObserver(
      (entries) => {
        const vis = entries.some((e) => e.isIntersecting);
        if (vis && !running) { running = true; last = performance.now(); raf = requestAnimationFrame(frame); }
        else if (!vis && running) { running = false; cancelAnimationFrame(raf); }
      },
      { threshold: 0 },
    );
    io.observe(host);
  }

  function init() {
    const host = document.querySelector("#iso");
    if (!host) return;
    host.textContent = "";
    animate(build(host), host);
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
