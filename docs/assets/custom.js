// Inject a "back to the migrant landing page" link at the top of the sidebar.
// The landing page lives one level up from the book (site root vs /guide/).
(function () {
  function addHomeLink() {
    var scroll = document.querySelector(".sidebar .sidebar-scrollbox");
    if (!scroll || scroll.querySelector(".migrant-home")) return;
    var a = document.createElement("a");
    a.className = "migrant-home";
    a.href = "../";
    a.textContent = "← migrant home";
    scroll.insertBefore(a, scroll.firstChild);
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", addHomeLink);
  } else {
    addHomeLink();
  }
})();
