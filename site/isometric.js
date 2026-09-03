// The whole solution as one isometric diagram.
//
// Drawn as SVG from a real isometric projection rather than generated, because
// the point of this picture is that it is ACCURATE: six stations, what each one
// does, which ones exist today, and — the thing a straight pipeline diagram
// cannot show — that it closes. The summary goes back to the client, who
// replies, and it goes round again. The loop is the product.
//
// Projection is standard 2:1 isometric:  px = (x - y) * W/2
//                                        py = (x + y) * H/2 - z
// Nodes are painted back-to-front by depth (x + y) so nearer slabs overlap.

(function () {
  const W = 132; // tile width
  const H = 66; // tile height (2:1)
  const NS = "http://www.w3.org/2000/svg";

  const iso = (x, y, z = 0) => ({ px: (x - y) * (W / 2), py: (x + y) * (H / 2) - z });

  // The circuit, clockwise. `ship` marks what actually exists today — the
  // launcher builds the team, and nothing else here is written yet.
  const NODES = [
    {
      id: "channels",
      x: 0, y: 0, h: 16,
      title: "Your client",
      sub: "WhatsApp · Telegram · Slack\nemail · shared docs",
      state: "planned",
    },
    {
      id: "inbox",
      x: 2.7, y: 0, h: 20,
      title: "Client channel",
      sub: "one inbox, no AI\ndeterministic pipe",
      state: "planned",
    },
    {
      id: "daemon",
      x: 5.4, y: 0, h: 30,
      title: "herdupd",
      sub: "SQLite · unix socket\nthe only thing that talks to herdr",
      state: "planned",
      hub: true,
    },
    {
      id: "team",
      x: 5.4, y: 3.1, h: 24,
      title: "The team",
      sub: "PM · coders · QA\nin herdr panes",
      state: "shipping",
      cluster: true,
    },
    {
      id: "board",
      x: 2.7, y: 3.1, h: 18,
      title: "Board & thread",
      sub: "kanban, PM↔agent traffic\ndocuments",
      state: "planned",
    },
    {
      id: "summary",
      x: 0, y: 3.1, h: 16,
      title: "Executive summary",
      sub: "status · KPIs\nback to the client",
      state: "planned",
    },
  ];

  // Closed circuit: channels → inbox → daemon → team → board → summary → back.
  const EDGES = [
    ["channels", "inbox"],
    ["inbox", "daemon"],
    ["daemon", "team"],
    ["team", "board"],
    ["board", "summary"],
    ["summary", "channels"],
  ];

  const byId = (id) => NODES.find((n) => n.id === id);
  const el = (name, attrs = {}) => {
    const e = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
    return e;
  };

  /// One isometric slab: top face plus two sides, so it reads as a solid.
  function slab(g, n, wUnits = 1.05) {
    const s = wUnits / 2;
    const top = [
      iso(n.x - s, n.y - s, n.h),
      iso(n.x + s, n.y - s, n.h),
      iso(n.x + s, n.y + s, n.h),
      iso(n.x - s, n.y + s, n.h),
    ];
    const botL = iso(n.x - s, n.y + s, 0);
    const botM = iso(n.x + s, n.y + s, 0);
    const botR = iso(n.x + s, n.y - s, 0);
    const pts = (a) => a.map((p) => `${p.px.toFixed(1)},${p.py.toFixed(1)}`).join(" ");
    const cls = n.state === "shipping" ? " on" : "";

    g.appendChild(el("polygon", { class: "face left" + cls, points: pts([top[3], top[2], botM, botL]) }));
    g.appendChild(el("polygon", { class: "face right" + cls, points: pts([top[2], top[1], botR, botM]) }));
    g.appendChild(el("polygon", { class: "face top" + cls, points: pts(top) }));
  }

  function label(g, n) {
    // The top face is a diamond reaching H/2 above its centre, so a label
    // placed near the centre lands ON the slab. Clear it entirely.
    const p = iso(n.x, n.y, n.h);
    const lift = H / 2 + 16;

    const t = el("text", { class: "iso-title", x: p.px.toFixed(1), y: (p.py - lift - 14).toFixed(1) });
    t.textContent = n.title;
    g.appendChild(t);

    n.sub.split("\n").forEach((line, i) => {
      const s = el("text", {
        class: "iso-sub",
        x: p.px.toFixed(1),
        y: (p.py - lift + 1 + i * 12).toFixed(1),
      });
      s.textContent = line;
      g.appendChild(s);
    });

    // Only one of these is real software, and it must be obvious which.
    const badge = el("text", {
      class: "iso-badge " + n.state,
      x: p.px.toFixed(1),
      y: (p.py - H / 2 - 46).toFixed(1),
    });
    badge.textContent = n.state === "shipping" ? "SHIPPING" : "PLANNED";
    g.appendChild(badge);
  }

  function build(host) {
    const svg = el("svg", { class: "iso-svg", xmlns: NS, role: "img" });
    svg.appendChild(
      (() => {
        const t = el("title");
        t.textContent =
          "Isometric diagram of the herdup loop: your client sends a request from a chat app, " +
          "the client channel takes it in, the daemon routes it, the team executes, the board " +
          "and summary are produced, and the summary returns to the client.";
        return t;
      })(),
    );

    const edges = el("g", { class: "iso-edges" });
    const nodes = el("g", { class: "iso-nodes" });

    // Edges first so slabs sit on top of them.
    const pathFor = (a, b) => {
      const p1 = iso(a.x, a.y, 6);
      const p2 = iso(b.x, b.y, 6);
      return `M ${p1.px.toFixed(1)} ${p1.py.toFixed(1)} L ${p2.px.toFixed(1)} ${p2.py.toFixed(1)}`;
    };
    const loopPts = [];
    EDGES.forEach(([from, to]) => {
      const a = byId(from);
      const b = byId(to);
      edges.appendChild(el("path", { class: "iso-edge", d: pathFor(a, b) }));
      const p = iso(a.x, a.y, 6);
      loopPts.push(`${p.px.toFixed(1)},${p.py.toFixed(1)}`);
    });

    // The daemon hands work to several agents at once, so draw it as a fan
    // rather than a single line into a box. A one-to-one arrow would say the
    // team takes one task at a time.
    (() => {
      const d = byId("daemon");
      const t = byId("team");
      [
        [-0.28, -0.28],
        [0.28, -0.28],
        [-0.28, 0.28],
        [0.28, 0.28],
      ].forEach(([dx, dy]) => {
        const p1 = iso(d.x, d.y, 6);
        const p2 = iso(t.x + dx, t.y + dy, 10);
        edges.appendChild(
          el("path", {
            class: "iso-edge fan",
            d: `M ${p1.px.toFixed(1)} ${p1.py.toFixed(1)} L ${p2.px.toFixed(1)} ${p2.py.toFixed(1)}`,
          }),
        );
      });
    })();

    // The travelling request. One closed polyline is easier to follow than six
    // separate tweens, and it makes the circuit unmistakable.
    const loop = el("polygon", { class: "iso-loop", points: loopPts.join(" ") });
    edges.insertBefore(loop, edges.firstChild);

    NODES.slice()
      .sort((a, b) => a.x + a.y - (b.x + b.y))
      .forEach((n) => {
        const g = el("g", { class: "iso-node", "data-id": n.id });
        if (n.cluster) {
          // The team is several agents, so draw it as several slabs.
          [
            { dx: -0.28, dy: -0.28, h: n.h },
            { dx: 0.28, dy: -0.28, h: n.h - 10 },
            { dx: -0.28, dy: 0.28, h: n.h - 10 },
            { dx: 0.28, dy: 0.28, h: n.h - 18 },
          ].forEach((o) =>
            slab(g, { ...n, x: n.x + o.dx, y: n.y + o.dy, h: o.h }, 0.5),
          );
        } else {
          slab(g, n, n.hub ? 1.15 : 1.0);
        }
        label(g, n);
        nodes.appendChild(g);
      });

    svg.appendChild(edges);
    svg.appendChild(nodes);

    // MANY packets, not one. A single token circulating implies the system is
    // serial and blocking — that one request must finish its lap before the
    // next moves. It is the opposite: the client keeps writing, the PM keeps
    // assigning, and the workers run independently. The picture has to say so.
    const packets = [];
    for (let i = 0; i < 7; i++) {
      const c = el("circle", { class: "iso-packet" + (i % 3 === 0 ? " hot" : ""), r: i % 3 === 0 ? 7 : 5, cx: 0, cy: 0 });
      svg.appendChild(c);
      packets.push(c);
    }

    host.appendChild(svg);

    // Fit the viewBox to the drawn content, with room for labels above slabs.
    const box = svg.getBBox();
    const pad = 34;
    svg.setAttribute(
      "viewBox",
      `${box.x - pad} ${box.y - pad} ${box.width + pad * 2} ${box.height + pad * 2}`,
    );

    return { svg, packets, loop };
  }

  function animate({ packets, loop }, host) {
    if (!window.gsap) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    gsap.from(host.querySelectorAll(".iso-node"), {
      y: 26,
      opacity: 0,
      duration: 0.6,
      stagger: 0.1,
      ease: "power3.out",
      immediateRender: false,
      scrollTrigger: { trigger: host, start: "top 74%", once: true },
    });

    if (reduced) {
      packets.forEach((p) => p.setAttribute("opacity", "0"));
      return;
    }

    const pts = loop
      .getAttribute("points")
      .trim()
      .split(/\s+/)
      .map((p) => p.split(",").map(Number));
    const seq = pts.concat([pts[0]]);

    // One timeline per packet, each started at a different point and running at
    // a slightly different speed, so they drift apart instead of marching in
    // lockstep. No dwell at stations: nothing is waiting its turn.
    const timelines = packets.map((packet, i) => {
      const tl = gsap.timeline({ repeat: -1, defaults: { ease: "none" } });
      seq.forEach(([x, y], n) => {
        if (n === 0) {
          gsap.set(packet, { attr: { cx: x, cy: y } });
          return;
        }
        tl.to(packet, { attr: { cx: x, cy: y }, duration: 1.0 });
      });
      tl.timeScale(0.8 + (i % 4) * 0.12);
      tl.progress((i / packets.length + (i % 3) * 0.04) % 1);
      return tl;
    });

    // Every station works at its own tempo, so none of them look like they are
    // idling until a token arrives.
    host.querySelectorAll(".iso-node").forEach((n, i) => {
      n.style.setProperty("--pulse-delay", (i * 0.47).toFixed(2) + "s");
      n.style.setProperty("--pulse-dur", (2.4 + (i % 3) * 0.6).toFixed(2) + "s");
      n.classList.add("busy");
    });

    const io = new IntersectionObserver(
      (entries) =>
        entries.forEach((e) => timelines.forEach((tl) => (e.isIntersecting ? tl.play() : tl.pause()))),
      { threshold: 0 },
    );
    io.observe(host);
  }

  function init() {
    const host = document.querySelector("#iso");
    if (!host) return;
    const parts = build(host);
    animate(parts, host);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
