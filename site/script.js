(() => {
  const toast = document.querySelector(".toast");
  let toastTimer;

  function showToast(message = "Copied") {
    toast.textContent = message;
    toast.classList.add("is-visible");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.classList.remove("is-visible"), 1600);
  }

  async function copyText(text, button) {
    try {
      await navigator.clipboard.writeText(text);
      if (button) {
        const original = button.textContent;
        button.textContent = "copied";
        setTimeout(() => { button.textContent = original; }, 1200);
      }
      showToast();
    } catch {
      showToast("Copy unavailable");
    }
  }

  document.querySelectorAll("[data-copy]").forEach((button) => {
    button.addEventListener("click", () => copyText(button.dataset.copy.replace(/\\n/g, "\n"), button));
  });

  const menuToggle = document.querySelector(".menu-toggle");
  const mobileNav = document.querySelector(".mobile-nav");
  menuToggle?.addEventListener("click", () => {
    const open = mobileNav.classList.toggle("is-open");
    menuToggle.setAttribute("aria-expanded", String(open));
  });
  mobileNav?.querySelectorAll("a").forEach((link) => link.addEventListener("click", () => {
    mobileNav.classList.remove("is-open");
    menuToggle?.setAttribute("aria-expanded", "false");
  }));

  const clientName = document.querySelector("[data-client-name]");
  document.querySelectorAll("[data-tab]").forEach((tab) => {
    tab.addEventListener("click", () => {
      const name = tab.dataset.tab;
      document.querySelectorAll("[data-tab]").forEach((item) => {
        const selected = item === tab;
        item.classList.toggle("is-selected", selected);
        item.setAttribute("aria-selected", String(selected));
      });
      document.querySelectorAll("[data-panel]").forEach((panel) => {
        panel.classList.toggle("is-visible", panel.dataset.panel === name);
      });
      if (clientName) clientName.textContent = name === "claude" ? "claude-desktop" : name;
    });
  });

  const sidebarLinks = [...document.querySelectorAll(".sidebar-link")];
  const sections = sidebarLinks.map((link) => document.querySelector(link.getAttribute("href"))).filter(Boolean);
  const observer = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      sidebarLinks.forEach((link) => link.classList.toggle("is-active", link.getAttribute("href") === `#${entry.target.id}`));
    });
  }, { rootMargin: "-20% 0px -65% 0px" });
  sections.forEach((section) => observer.observe(section));
})();
