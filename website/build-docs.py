#!/usr/bin/env python3
"""Render website/content/*.md into standalone HTML under website/docs/."""

from __future__ import annotations

from pathlib import Path

import markdown

ROOT = Path(__file__).resolve().parent
CONTENT = ROOT / "content"
OUT = ROOT / "docs"

DOCS = [
    ("README.md", "readme", "README"),
    ("product-specification.md", "product-specification", "Product specification"),
    (
        "software-requirements-specification.md",
        "software-requirements-specification",
        "SRS",
    ),
    ("risk-capital-guide.md", "risk-capital-guide", "Risk capital guide"),
    ("system-architecture.md", "system-architecture", "System architecture"),
    ("technical-architecture.md", "technical-architecture", "Technical architecture"),
]

MD = markdown.Markdown(
    extensions=[
        "tables",
        "fenced_code",
        "sane_lists",
        "toc",
        "nl2br",
        "smarty",
    ]
)


def nav_html(active: str) -> str:
    items = []
    for _, slug, title in DOCS:
        cls = ' class="is-active"' if slug == active else ""
        items.append(f'<a href="./{slug}.html"{cls}>{title}</a>')
    return "\n          ".join(items)


def page(slug: str, title: str, body: str) -> str:
    return f"""<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{title} — Continuous</title>
    <meta name="theme-color" content="#0e0d0b" />
    <link rel="icon" href="../assets/icon-192.png" type="image/png" />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link
      href="https://fonts.googleapis.com/css2?family=Cormorant+Garamond:wght@500;600;700&family=IBM+Plex+Mono:wght@400;500&display=swap"
      rel="stylesheet"
    />
    <link rel="stylesheet" href="../styles.css" />
    <link rel="stylesheet" href="../docs.css" />
  </head>
  <body class="docs-body">
    <div class="site docs-shell">
      <header class="wrap nav">
        <a class="brand" href="../index.html">
          <img
            class="brand-mark"
            src="../assets/icon-192.png"
            width="36"
            height="36"
            alt="Continuous"
          />
          <span class="brand-text">
            Continuous
            <small>docs</small>
          </span>
        </a>
        <nav class="nav-links" aria-label="Primary">
          <a href="../index.html#flow">Flow</a>
          <a href="../index.html#rules">Rules</a>
          <a href="../index.html">Landing</a>
          <a href="https://github.com/bamaao/continuous-pdf-prediction-market">GitHub</a>
        </nav>
      </header>

      <div class="docs-layout wrap">
        <aside class="docs-side" aria-label="Document list">
          <p class="kicker">Library</p>
          <nav class="docs-nav">
          {nav_html(slug)}
          </nav>
        </aside>
        <article class="docs-article">
          <p class="docs-status">{title}</p>
          <div class="markdown-body">
{body}
          </div>
        </article>
      </div>
    </div>
  </body>
</html>
"""


ASSETS = [
    "system-arch.png",
    "tech-arch.png",
    "business-flow.png",
]


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for filename, slug, title in DOCS:
        src = CONTENT / filename
        if not src.exists():
            raise SystemExit(f"missing {src}")
        MD.reset()
        body = MD.convert(src.read_text(encoding="utf-8"))
        out = OUT / f"{slug}.html"
        out.write_text(page(slug, title, body), encoding="utf-8")
        print(f"wrote {out.relative_to(ROOT)}")

    # Diagrams referenced as ./system-arch.png etc. from the rendered HTML pages.
    repo_docs = ROOT.parent / "docs"
    for name in ASSETS:
        src = repo_docs / name
        if not src.exists():
            # Also accept copies staged under website/content/
            alt = CONTENT / name
            src = alt if alt.exists() else src
        if not src.exists():
            print(f"warn: missing diagram {name}")
            continue
        dest = OUT / name
        dest.write_bytes(src.read_bytes())
        print(f"copied {name}")

    # convenience index
    (OUT / "index.html").write_text(
        '<!DOCTYPE html><meta charset="utf-8" />'
        '<meta http-equiv="refresh" content="0; url=./readme.html" />'
        '<link rel="canonical" href="./readme.html" />'
        '<title>Docs — Continuous</title>'
        '<a href="./readme.html">Open README</a>\n',
        encoding="utf-8",
    )
    print("wrote docs/index.html")


if __name__ == "__main__":
    main()
