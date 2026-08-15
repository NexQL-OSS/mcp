/**
 * NexQL MCP — site chrome.
 *
 * Header offset sync, mobile nav, scroll state, and the scroll-reveal observer
 * that toggles `.is-visible` on `[data-nx-reveal]` elements (the transition
 * itself lives in styles/nx.css under `.nx-root [data-nx-reveal]`).
 *
 * Replaces the old feature-page-boot.js. Deliberately plain, deferred script
 * rather than an island: it must run on every page including the ones that
 * ship no Preact at all.
 */
(function initSiteChrome() {
  const header = () => document.querySelector(".site-header.landing-topbar");

  function syncHeaderOffset() {
    const el = header();
    if (!el) return;
    document.documentElement.style.setProperty(
      "--site-header-offset",
      `${Math.ceil(el.getBoundingClientRect().height)}px`,
    );
  }

  function wireMobileNav() {
    const btn = document.getElementById("btn-toggle-topbar");
    const nav = document.getElementById("site-nav");
    if (!btn || !nav) return;

    btn.addEventListener("click", () => {
      const open = nav.classList.toggle("show");
      btn.setAttribute("aria-expanded", open ? "true" : "false");
      requestAnimationFrame(syncHeaderOffset);
    });

    // Tapping a link closes the sheet; without this the nav stays open behind
    // the freshly navigated page on same-document anchor links.
    nav.addEventListener("click", (e) => {
      if (!(e.target instanceof HTMLAnchorElement)) return;
      if (!nav.classList.contains("show")) return;
      nav.classList.remove("show");
      btn.setAttribute("aria-expanded", "false");
      requestAnimationFrame(syncHeaderOffset);
    });
  }

  function wireHeaderScroll() {
    const el = header();
    if (!el) return;
    const sync = () => el.classList.toggle("is-scrolled", window.scrollY > 8);
    window.addEventListener("scroll", sync, { passive: true });
    sync();
  }

  function wireReveal() {
    const targets = document.querySelectorAll("[data-nx-reveal]");
    if (!targets.length) return;

    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce || !("IntersectionObserver" in window)) {
      targets.forEach((el) => el.classList.add("is-visible"));
      return;
    }

    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (!entry.isIntersecting) return;
          entry.target.classList.add("is-visible");
          io.unobserve(entry.target);
        });
      },
      { rootMargin: "0px 0px -12% 0px", threshold: 0.05 },
    );

    targets.forEach((el) => io.observe(el));
  }

  function boot() {
    wireMobileNav();
    wireHeaderScroll();
    wireReveal();
    syncHeaderOffset();
    window.addEventListener("resize", syncHeaderOffset, { passive: true });
    // The theme picker injects its trigger label after fetching the theme
    // summary, which can change the header height by a pixel or two.
    if (window.NexqlThemes?.ready) {
      void window.NexqlThemes.ready.then(syncHeaderOffset).catch(() => {});
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
