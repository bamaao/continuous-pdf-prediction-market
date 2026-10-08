(() => {
  const nodes = document.querySelectorAll(".reveal");
  if (nodes.length) {
    if (!("IntersectionObserver" in window)) {
      nodes.forEach((el) => el.classList.add("in"));
    } else {
      const io = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            if (!entry.isIntersecting) continue;
            entry.target.classList.add("in");
            io.unobserve(entry.target);
          }
        },
        { rootMargin: "0px 0px -8% 0px", threshold: 0.12 }
      );
      nodes.forEach((el) => io.observe(el));
    }
  }

  const video = document.getElementById("lifecycle-video");
  const tabs = document.querySelectorAll(".video-tab");
  const panels = document.querySelectorAll(".video-biz-panel");
  if (!video || !tabs.length) return;

  function showBiz(family) {
    panels.forEach((panel) => {
      const match = panel.getAttribute("data-family") === family;
      panel.classList.toggle("is-active", match);
      panel.hidden = !match;
    });
  }

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      const src = tab.getAttribute("data-src");
      const poster = tab.getAttribute("data-poster");
      const family = tab.getAttribute("data-family");
      if (!src) return;

      tabs.forEach((t) => {
        t.classList.toggle("is-active", t === tab);
        t.setAttribute("aria-selected", t === tab ? "true" : "false");
      });

      if (family) showBiz(family);

      const wasPlaying = !video.paused;
      if (poster) video.setAttribute("poster", poster);
      const source = video.querySelector("source");
      if (source) source.src = src;
      video.src = src;
      video.load();
      if (wasPlaying) {
        video.play().catch(() => {});
      }
    });
  });
})();
