// Progressive enhancement for the landing page. Nothing here is required for
// the page to render or for the links to work.

// Copy-to-clipboard on the command snippets.
document.querySelectorAll(".copy").forEach((btn) => {
  if (btn.tagName === "A") return; // the releases "Open" link is a real link
  btn.addEventListener("click", async () => {
    const text = btn.getAttribute("data-copy") || "";
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard API blocked (e.g. file://). Fall back to a hidden textarea.
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); } catch { /* give up quietly */ }
      ta.remove();
    }
    const original = btn.textContent;
    btn.textContent = "Copied";
    btn.classList.add("copied");
    setTimeout(() => {
      btn.textContent = original;
      btn.classList.remove("copied");
    }, 1400);
  });
});

// Install-method tabs: toggle the active tab and show its panel.
document.querySelectorAll(".install").forEach((box) => {
  const tabs = box.querySelectorAll(".tab");
  const panels = box.querySelectorAll(".panel");
  tabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      const which = tab.getAttribute("data-tab");
      tabs.forEach((t) => {
        const on = t === tab;
        t.classList.toggle("is-active", on);
        t.setAttribute("aria-selected", on ? "true" : "false");
      });
      panels.forEach((p) => {
        const on = p.getAttribute("data-panel") === which;
        p.classList.toggle("is-active", on);
        p.hidden = !on;
      });
    });
  });
});

// Subtle pointer parallax on the hero art. Skipped when the user prefers reduced
// motion or on coarse (touch) pointers.
const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const coarse = window.matchMedia("(pointer: coarse)").matches;
const art = document.querySelector(".bird");
const stage = document.getElementById("stage");
if (art && stage && !reduce && !coarse) {
  let raf = 0;
  window.addEventListener("pointermove", (e) => {
    const dx = (e.clientX / window.innerWidth - 0.5) * 2;
    const dy = (e.clientY / window.innerHeight - 0.5) * 2;
    if (raf) return;
    raf = requestAnimationFrame(() => {
      art.style.transform = `translate(${dx * 12}px, ${dy * 10}px) rotate(${dx * 0.8}deg)`;
      raf = 0;
    });
  });
}
