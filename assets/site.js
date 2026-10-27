// Respect reduced motion without requiring a client framework.
const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
if (motion.matches) document.documentElement.style.scrollBehavior = "auto";

// Highlight the target section for keyboard users when navigating anchors.
for (const anchor of document.querySelectorAll('a[href^="#"]')) {
  anchor.addEventListener("click", () => {
    const target = document.getElementById(anchor.getAttribute("href").slice(1));
    if (target) target.setAttribute("tabindex", "-1");
  });
}
