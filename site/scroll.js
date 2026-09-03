// The scroll narrative.
//
// One idea carries the whole page: a single field of points that starts
// scattered, gathers into a herd, and finally resolves into the four lanes of
// herdup's real workspace. Chaos becoming order — which is both the product's
// name and what it actually does.
//
// Architecture: GSAP ScrollTrigger owns exactly one number (`view.t`, 0→1
// across the story). three.js reads it every frame and interpolates between
// three precomputed layouts. Nothing else drives the canvas, so the scroll and
// the render can never disagree.

const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/* ------------------------------------------------------------------ */
/* the point field                                                     */
/* ------------------------------------------------------------------ */

const view = { t: 0, smoothed: 0 };
const COUNT = 1100;
const LANES = 4;

// Exposed so the rendered state can be checked against the scroll position
// from the console. Read-only diagnostic; nothing depends on it.
window.__herd = view;

function layouts() {
  const scatter = new Float32Array(COUNT * 3);
  const gather = new Float32Array(COUNT * 3);
  const lanes = new Float32Array(COUNT * 3);
  const lead = new Float32Array(COUNT); // 1 for points that end in the lead lane

  for (let i = 0; i < COUNT; i++) {
    const i3 = i * 3;

    // Scattered: wide, thin, disorderly — a lot of animals, no direction.
    scatter[i3] = (Math.random() - 0.5) * 30;
    scatter[i3 + 1] = (Math.random() - 0.5) * 12;
    scatter[i3 + 2] = (Math.random() - 0.5) * 14;

    // Gathered: one loose drifting mass, still organic.
    const a = Math.random() * Math.PI * 2;
    const r = Math.pow(Math.random(), 0.6) * 5.2;
    gather[i3] = Math.cos(a) * r * 1.7;
    gather[i3 + 1] = Math.sin(a) * r * 0.55;
    gather[i3 + 2] = (Math.random() - 0.5) * 4;

    // Lanes: four upright columns, matching the workspace the app builds.
    // The gap has to be wider than the column, and the column clearly taller
    // than it is wide, or four columns just read as one blob.
    const lane = i % LANES;
    const w = 1.5;
    const gap = 4.2;
    const x0 = (lane - (LANES - 1) / 2) * gap;
    lanes[i3] = x0 + (Math.random() - 0.5) * w;
    lanes[i3 + 1] = (Math.random() - 0.5) * 16;
    lanes[i3 + 2] = (Math.random() - 0.5) * 1.2;
    lead[i] = lane === 0 ? 1 : 0;
  }
  return { scatter, gather, lanes, lead };
}

function startField(canvas) {
  if (!window.THREE) return null;
  const THREE = window.THREE;

  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true });
  } catch {
    return null; // no WebGL — the page reads fine without it
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(48, 1, 0.1, 200);
  camera.position.z = 26;

  const L = layouts();
  const positions = new Float32Array(COUNT * 3);
  positions.set(L.scatter);

  const colors = new Float32Array(COUNT * 3);
  const amber = new THREE.Color("#d9903f");
  const dim = new THREE.Color("#6c7180");
  for (let i = 0; i < COUNT; i++) {
    const c = L.lead[i] ? amber : dim;
    colors[i * 3] = c.r;
    colors[i * 3 + 1] = c.g;
    colors[i * 3 + 2] = c.b;
  }

  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));

  const material = new THREE.PointsMaterial({
    size: 0.13,
    vertexColors: true,
    transparent: true,
    opacity: 0.9,
    depthWrite: false,
  });
  const points = new THREE.Points(geo, material);
  scene.add(points);

  const resize = () => {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (!w || !h) return;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    // Wide screens read as two columns: sentence left, herd right. Narrow
    // screens have no room for that, so the field re-centres.
    points.position.x = w >= 860 ? 7.5 : 0;
  };
  new ResizeObserver(resize).observe(canvas);
  resize();

  // Smooth the scroll value so flicks don't snap the herd around.
  let smoothed = 0;
  const lerp = (a, b, k) => a + (b - a) * k;

  const frame = (now) => {
    smoothed = lerp(smoothed, view.t, reduced ? 1 : 0.12);
    view.smoothed = smoothed;
    const t = smoothed;

    // 0 → A: scatter becomes a herd. A → B: the herd becomes the lanes.
    // B → 1: HOLD. The payoff has to finish well before the canvas scrolls
    // away, or the one thing the whole section builds to is never seen.
    const A = 0.34;
    const B = 0.70;
    const gathering = t < A;
    const phase = gathering ? t / A : Math.min(1, (t - A) / (B - A));
    const from = gathering ? L.scatter : L.gather;
    const to = gathering ? L.gather : L.lanes;
    const e = phase * phase * (3 - 2 * phase); // smoothstep

    const drift = reduced ? 0 : now * 0.00012;
    for (let i = 0; i < COUNT; i++) {
      const i3 = i * 3;
      const wob = reduced ? 0 : Math.sin(drift * 6 + i) * (1 - t) * 0.35;
      positions[i3] = from[i3] + (to[i3] - from[i3]) * e + wob;
      positions[i3 + 1] = from[i3 + 1] + (to[i3 + 1] - from[i3 + 1]) * e;
      positions[i3 + 2] = from[i3 + 2] + (to[i3 + 2] - from[i3 + 2]) * e;
    }
    geo.attributes.position.needsUpdate = true;

    // Settle the camera as order emerges: wide and tilted → square on.
    points.rotation.y = (1 - t) * 0.5 - 0.05;
    camera.position.z = 26 - t * 6;
    material.opacity = 0.35 + t * 0.5;

    renderer.render(scene, camera);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  return true;
}

/* ------------------------------------------------------------------ */
/* scroll wiring                                                       */
/* ------------------------------------------------------------------ */

function init() {
  const canvas = document.querySelector("#field");
  if (canvas) startField(canvas);

  if (!window.gsap || !window.ScrollTrigger) return;
  gsap.registerPlugin(ScrollTrigger);

  // One trigger owns the field's progress for the whole story section.
  const story = document.querySelector("#story");
  if (story) {
    ScrollTrigger.create({
      trigger: story,
      start: "top top",
      end: "bottom bottom",
      onUpdate: (self) => {
        view.t = self.progress;
      },
    });
  }

  // Each beat's text fades up as it arrives and holds while pinned.
  gsap.utils.toArray(".beat").forEach((beat) => {
    gsap.fromTo(
      beat.querySelector(".beat-in"),
      { y: 26, opacity: 0 },
      {
        y: 0,
        opacity: 1,
        ease: "power3.out",
        duration: 0.8,
        scrollTrigger: { trigger: beat, start: "top 72%", toggleActions: "play none none reverse" },
      },
    );
  });

  // The product shots arrive with a small lift — enough to feel deliberate,
  // not so much that it reads as a carousel.
  gsap.utils.toArray(".shot").forEach((shot) => {
    gsap.fromTo(
      shot,
      { y: 40, opacity: 0, scale: 0.97 },
      {
        y: 0,
        opacity: 1,
        scale: 1,
        ease: "power3.out",
        duration: 0.9,
        scrollTrigger: { trigger: shot, start: "top 80%", toggleActions: "play none none reverse" },
      },
    );
  });

  // ScrollTrigger measures the page when it is created, which is before the
  // hero image and the screenshots have loaded. Without this the story's
  // start/end are computed against a much shorter document and the herd never
  // advances for anyone who scrolls early.
  window.addEventListener("load", () => ScrollTrigger.refresh());
  document.querySelectorAll("img").forEach((img) => {
    if (!img.complete) img.addEventListener("load", () => ScrollTrigger.refresh(), { once: true });
  });

  gsap.utils.toArray(".rise").forEach((el, i) => {
    gsap.fromTo(
      el,
      { y: 18, opacity: 0 },
      {
        y: 0,
        opacity: 1,
        ease: "power3.out",
        duration: 0.7,
        delay: (i % 4) * 0.05,
        scrollTrigger: { trigger: el, start: "top 88%", toggleActions: "play none none reverse" },
      },
    );
  });
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
