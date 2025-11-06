#!/usr/bin/env python3
"""
Builds the GitHub Pages artifact by combining cargo doc output with markdown guides.
"""

import html
import os
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET_DOC = ROOT / "target" / "doc"
DOCS_SRC = ROOT / "docs"
DIST = ROOT / "dist"


def ensure_sources():
    if not TARGET_DOC.exists():
        raise SystemExit("cargo doc output missing; run cargo doc before build_pages")
    if not DOCS_SRC.exists():
        raise SystemExit("docs directory missing")


def clean_dist():
    if DIST.exists():
        shutil.rmtree(DIST)
    DIST.mkdir(parents=True)


def copy_rustdoc():
    shutil.copytree(TARGET_DOC, DIST / "rustdoc")


def render_markdown_placeholder(path: Path) -> str:
    with path.open("r", encoding="utf-8") as f:
        text = f.read()
    escaped = html.escape(text)
    return f"""<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>{path.stem}</title>
    <style>
      body {{
        font-family: system-ui, sans-serif;
        margin: 2rem;
        max-width: 60rem;
      }}
      pre {{
        background: #f4f4f4;
        padding: 1rem;
        white-space: pre-wrap;
      }}
      code {{
        background: #f4f4f4;
        padding: 0 0.3rem;
      }}
    </style>
  </head>
  <body>
    <h1>{path.stem}</h1>
    <pre>{escaped}</pre>
  </body>
</html>
"""


def copy_docs() -> list[tuple[str, str]]:
    docs_out = DIST / "docs"
    docs_out.mkdir()
    entries: list[tuple[str, str]] = []
    for md in sorted(DOCS_SRC.glob("*.md")):
        html_content = render_markdown_placeholder(md)
        output = docs_out / f"{md.stem}.html"
        output.write_text(html_content, encoding="utf-8")

        title = md.stem
        with md.open("r", encoding="utf-8") as f:
            for line in f:
                if line.startswith("#"):
                    title = line.lstrip("# ").strip()
                    break

        entries.append((title, f"docs/{output.name}"))
    return entries


def write_index(doc_entries: list[tuple[str, str]]):
    index = DIST / "index.html"
    doc_links = "\n".join(
        f'      <li><a href="{href}">{html.escape(title)}</a></li>'
        for title, href in doc_entries
    )
    index.write_text(
        (
            """<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>greentic-dev documentation</title>
    <style>
      body { font-family: system-ui, sans-serif; margin: 2rem; }
      ul { line-height: 1.6; }
      a { color: #0b65c2; text-decoration: none; }
      a:hover { text-decoration: underline; }
    </style>
  </head>
  <body>
    <h1>greentic-dev documentation</h1>
    <ul>
      <li><a href="rustdoc/greentic_dev/index.html">Rust API docs</a></li>
{doc_links}
    </ul>
  </body>
</html>
""".replace(
                "{doc_links}", doc_links
            )
        ),
        encoding="utf-8",
    )


def main():
    ensure_sources()
    clean_dist()
    copy_rustdoc()
    entries = copy_docs()
    write_index(entries)


if __name__ == "__main__":
    main()
