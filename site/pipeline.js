// The pipeline diagram: an isometric factory line, animated on scroll.
//
// Deliberately built in code rather than generated. A generated 3D factory
// looks the part but depicts nothing — no correct flow, no readable labels,
// no relationship to what herdup actually does. The generated plate behind it
// supplies the art direction; this supplies the meaning.
//
// The line runs: a request arrives from a channel → the PM turns it into work →
// coders and QA execute → the board and summary come back out. Blocks moving
// along the belt ARE requests; the amber one is the request currently in play.

(function () {
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function init() {
    const line = document.querySelector("#factory");
    if (!line || !window.gsap) return;

    const belt = line.querySelector(".belt-run");
    const stations = gsap.utils.toArray("#factory .station");

    // Stations light up in sequence, so the eye follows the flow rather than
    // taking the whole diagram in at once.
    const tl = gsap.timeline({
      scrollTrigger: { trigger: line, start: "top 72%", once: true },
    });
    tl.from(stations, {
      y: 22,
      opacity: 0,
      duration: 0.55,
      stagger: 0.13,
      ease: "power3.out",
      immediateRender: false,
    }).from(
      line.querySelectorAll(".link"),
      { scaleX: 0, transformOrigin: "left center", duration: 0.4, stagger: 0.13, ease: "power2.out", immediateRender: false },
      0.2,
    );

    if (reduced || !belt) return;

    // The belt only runs while the diagram is on screen — an off-screen
    // requestAnimationFrame loop is wasted battery on a marketing page.
    let running = false;
    let raf = 0;
    const blocks = gsap.utils.toArray("#factory .block");
    const started = performance.now();

    const frame = (now) => {
      const t = (now - started) / 1000;
      blocks.forEach((b, i) => {
        // Evenly spaced, wrapping 0→1 along the belt.
        const p = (t * 0.13 + i / blocks.length) % 1;
        b.style.left = (p * 100).toFixed(2) + "%";
        // Fade in and out at the ends so blocks appear to enter and leave.
        b.style.opacity = String(Math.min(1, Math.min(p, 1 - p) * 8));
      });
      if (running) raf = requestAnimationFrame(frame);
    };

    const io = new IntersectionObserver(
      (entries) => {
        const visible = entries.some((e) => e.isIntersecting);
        if (visible && !running) {
          running = true;
          raf = requestAnimationFrame(frame);
        } else if (!visible && running) {
          running = false;
          cancelAnimationFrame(raf);
        }
      },
      { threshold: 0 },
    );
    io.observe(line);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
