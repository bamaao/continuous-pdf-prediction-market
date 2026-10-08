(() => {
  const DOCS = [
    { id: "README", title: "README", path: "./content/README.md" },
    {
      id: "product-specification",
      title: "Product specification",
      path: "./content/product-specification.md",
    },
    {
      id: "software-requirements-specification",
      title: "SRS",
      path: "./content/software-requirements-specification.md",
    },
    {
      id: "risk-capital-guide",
      title: "Risk capital guide",
      path: "./content/risk-capital-guide.md",
    },
    {
      id: "system-architecture",
      title: "System architecture",
      path: "./content/system-architecture.md",
    },
    {
      id: "technical-architecture",
      title: "Technical architecture",
      path: "./content/technical-architecture.md",
    },
  ];

  const nav = document.getElementById("docs-nav");
  const status = document.getElementById("docs-status");
  const content = document.getElementById("docs-content");
  if (!nav || !status || !content) return;

  const params = new URLSearchParams(window.location.search);
  const initial = params.get("doc") || "README";

  for (const doc of DOCS) {
    const a = document.createElement("a");
    a.href = `./docs.html?doc=${encodeURIComponent(doc.id)}`;
    a.textContent = doc.title;
    a.dataset.id = doc.id;
    if (doc.id === initial) a.classList.add("is-active");
    nav.appendChild(a);
  }

  const selected = DOCS.find((d) => d.id === initial) || DOCS[0];

  async function loadDoc(doc) {
    status.textContent = `Loading ${doc.title}…`;
    content.innerHTML = "";
    document.title = `${doc.title} — Continuous`;

    try {
      const res = await fetch(doc.path, { cache: "no-cache" });
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      const md = await res.text();
      if (typeof marked === "undefined") {
        throw new Error("Markdown renderer unavailable");
      }
      marked.setOptions({
        gfm: true,
        breaks: false,
      });
      content.innerHTML = marked.parse(md);
      status.textContent = doc.title;
      window.scrollTo({ top: 0, behavior: "smooth" });
    } catch (err) {
      status.textContent = "Failed to load";
      content.innerHTML = `<p>Could not load <code>${doc.path}</code>.</p><p>${String(
        err.message || err
      )}</p><p>If you opened the file directly, serve the folder instead (<code>npx serve website</code>) so Markdown can be fetched.</p>`;
    }
  }

  loadDoc(selected);
})();
