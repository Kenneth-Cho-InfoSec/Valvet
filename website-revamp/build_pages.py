#!/usr/bin/env python3
"""Build the docs and examples pages from the plain-text site snapshot."""

from __future__ import annotations

import html
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT / "source-text"
SITE = ROOT / "site"


def esc(text: str) -> str:
    return html.escape(text, quote=False)


def inline(text: str) -> str:
    parts = text.split("`")
    out: list[str] = []
    for index, part in enumerate(parts):
        if index % 2 == 1:
            out.append(f"<code data-ui>{esc(part)}</code>")
        else:
            safe = esc(part)
            safe = re.sub(
                r"\[([^\]]+)\]\(([^)]+)\)",
                r'<a href="\2">\1</a>',
                safe,
            )
            out.append(safe)
    rendered = "".join(out)
    return rendered.replace('<a href="examples.html">example</a>', "example")


def slug(text: str) -> str:
    text = text.lower()
    text = re.sub(r"[^a-z0-9]+", "-", text).strip("-")
    return text or "section"


def fence_lang(label: str) -> str:
    low = label.lower()
    if "valvet" in low:
        return "valvet"
    if "bash" in low or "shell" in low:
        return "bash"
    return "text"


def code_card(label: str, lang: str, body: str, card_class: str = "") -> str:
    kind = "output" if "output" in label.lower() else "error" if "error" in label.lower() else "code"
    classes = "code-card" + (f" {kind}-card" if kind != "code" else "") + (f" {card_class}" if card_class else "")
    language = fence_lang(label) if lang == "text" else lang
    return f"""
<div class="{classes}" data-ui>
  <div class="code-head">
    <span class="code-label">{esc(label)}</span>
    <button class="copy-btn" type="button" data-copy aria-label="Copy {esc(label)}">Copy</button>
  </div>
  <pre class="code-pre"><code class="language-{language}">{esc(body)}</code></pre>
</div>"""


def parse_blocks(lines: list[str]):
    blocks = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
            continue
        if line.startswith("```"):
            fence = re.match(r"^(`{3,})", line).group(1)
            lang = line[len(fence):].strip()
            body = []
            i += 1
            while i < len(lines) and not lines[i].startswith(fence):
                body.append(lines[i])
                i += 1
            i += 1
            blocks.append(("code", "Code", lang or "text", "\n".join(body)))
            continue
        if line.startswith("### "):
            blocks.append(("heading", 3, line[4:].strip()))
            i += 1
            continue
        if line.startswith("## "):
            blocks.append(("heading", 2, line[3:].strip()))
            i += 1
            continue
        if line.startswith("|"):
            rows = []
            while i < len(lines) and lines[i].startswith("|"):
                row = lines[i].strip().strip("|")
                rows.append([cell.strip() for cell in row.split("|")])
                i += 1
            blocks.append(("table", rows))
            continue
        if line.startswith("- "):
            items = []
            while i < len(lines) and lines[i].startswith("- "):
                items.append(lines[i][2:].strip())
                i += 1
            blocks.append(("list", items))
            continue
        if re.match(r"^(Input · Valvet|Output|Bash|Error):$", line.strip()):
            label = line.strip().rstrip(":")
            if i + 1 < len(lines) and lines[i + 1].startswith("```"):
                fence_line = lines[i + 1]
                fence = re.match(r"^(`{3,})", fence_line).group(1)
                body = []
                i += 2
                while i < len(lines) and not lines[i].startswith(fence):
                    body.append(lines[i])
                    i += 1
                i += 1
                blocks.append(("code", label, "valvet" if "Valvet" in label else "text", "\n".join(body)))
                continue
        paragraph = [line.strip()]
        i += 1
        while i < len(lines):
            nxt = lines[i]
            if (
                not nxt.strip()
                or nxt.startswith("```")
                or nxt.startswith("## ")
                or nxt.startswith("### ")
                or nxt.startswith("|")
                or nxt.startswith("- ")
                or re.match(r"^(Input · Valvet|Output|Bash|Error):$", nxt.strip())
            ):
                break
            paragraph.append(nxt.strip())
            i += 1
        blocks.append(("p", " ".join(paragraph)))
    return blocks


def render_blocks(
    blocks, skip_first_heading: bool = False, drop_first_id: bool = False
) -> str:
    out = []
    skipped = False
    heading_index = 0
    for block in blocks:
        kind = block[0]
        if kind == "heading":
            level, text = block[1], block[2]
            if skip_first_heading and not skipped and text in ("Documentation", "Examples"):
                skipped = True
                continue
            tag = "h2" if level <= 3 else "h3"
            heading_index += 1
            if drop_first_id and heading_index == 1:
                out.append(f"<{tag}>{inline(text)}</{tag}>")
                continue
            anchor = slug(text)
            out.append(
                f'<{tag} id="{anchor}">{inline(text)}'
                f'<a class="heading-anchor" href="#{anchor}" aria-label="Link to {esc(text)}"></a>'
                f"</{tag}>"
            )
        elif kind == "p":
            text = block[1]
            if text.startswith("[Start](#start)"):
                continue
            out.append(f"<p>{inline(text)}</p>")
        elif kind == "list":
            items = "".join(f"<li>{inline(item)}</li>" for item in block[1])
            out.append(f"<ul>{items}</ul>")
        elif kind == "table":
            rows = block[1]
            if len(rows) < 2:
                continue
            head = rows[0]
            body = rows[2:] if len(rows) > 2 else []
            thead = "".join(f"<th>{inline(cell)}</th>" for cell in head)
            tbody = "".join(
                "<tr>" + "".join(f"<td data-ui>{inline(cell)}</td>" for cell in row) + "</tr>"
                for row in body
            )
            label = esc(re.sub(r"<[^>]+>", "", head[0])) if head else "Data"
            out.append(
                f'<div class="table-wrap" tabindex="0" role="region" '
                f'aria-label="Table: {label}"><table><thead><tr>{thead}</tr></thead>'
                f"<tbody>{tbody}</tbody></table></div>"
            )
        elif kind == "code":
            label, lang, body = block[1], block[2], block[3]
            out.append(code_card(label, lang, body))
    return "\n".join(out)


def heading_title(block) -> str | None:
    return block[2] if block[0] == "heading" else None


def split_sections(blocks):
    sections = []
    current = None
    for block in blocks:
        if block[0] == "heading":
            title = block[2]
            if title in ("Documentation", "Examples"):
                continue
            if title == "Errors" and current is not None:
                current["blocks"].append(block)
                continue
            current = {"title": title, "id": slug(title), "blocks": [block]}
            sections.append(current)
        elif current is not None:
            current["blocks"].append(block)
    return sections


NAV_ITEMS = [
    ("index.html#why", "Why Valvet"),
    ("index.html#features", "Features"),
    ("index.html#code", "Code"),
    ("docs.html", "Docs"),
    ("examples.html", "Examples"),
]


def nav(active: str = "") -> str:
    links = []
    for href, label in NAV_ITEMS:
        is_active = label.lower() == active.lower()
        current = ' aria-current="page"' if is_active else ""
        cls = ' class="is-active"' if is_active else ""
        links.append(f'<a href="{href}"{cls}{current}>{label}</a>')
    mobile_links = "\n".join(
        f'<a href="{href}">{label}<span aria-hidden="true">↗</span></a>' for href, label in NAV_ITEMS
    )
    return f"""
<header class="site-header" id="site-header">
  <div class="container nav-shell">
    <a class="brand" href="index.html" aria-label="Valvet home">
      <img src="assets/logo-128.png" width="36" height="36" alt="">
      <span class="brand-name">Valvet</span>
      <span class="brand-version">0.3.0</span>
    </a>
    <nav class="nav-links" aria-label="Primary">
      {''.join(links)}
    </nav>
    <div class="nav-actions">
      <button class="icon-btn theme-toggle" type="button" aria-label="Switch to dark theme" title="Switch theme">
        <svg class="icon-sun" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>
        <svg class="icon-moon" viewBox="0 0 24 24" aria-hidden="true"><path d="M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5 8.5 8.5 0 1 0 20.5 14.2Z"/></svg>
      </button>
      <a class="btn btn-primary btn-small nav-install" href="index.html#install">Install</a>
      <button class="icon-btn menu-toggle" type="button" aria-label="Open menu" aria-expanded="false" aria-controls="mobile-menu">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M4 12h16M4 17h16"/></svg>
      </button>
    </div>
  </div>
  <div class="mobile-menu" id="mobile-menu" hidden>
    <nav class="container mobile-nav" aria-label="Mobile">
      {mobile_links}
      <a href="https://github.com/Kenneth-Cho-InfoSec/valvet">GitHub<span aria-hidden="true">↗</span></a>
    </nav>
    <div class="container mobile-menu-foot">
      <a class="btn btn-primary" href="index.html#install">Install Valvet</a>
      <p>Native binaries. No garbage collector. No runtime.</p>
    </div>
  </div>
</header>"""


def footer() -> str:
    return """
<footer class="site-footer">
  <div class="container footer-grid">
    <div class="footer-brand">
      <a class="brand" href="index.html" aria-label="Valvet home">
        <img src="assets/logo-128.png" width="40" height="40" alt="">
        <span class="brand-name">Valvet</span>
      </a>
      <p>A small, safe, compiled language for data work.</p>
      <p class="footer-note">Native binaries. No garbage collector. No runtime.</p>
    </div>
    <div class="footer-col">
      <h2>Language</h2>
      <a href="index.html#why">Why Valvet</a>
      <a href="index.html#features">Features</a>
      <a href="index.html#code">Code</a>
      <a href="index.html#pipeline">How it runs</a>
    </div>
    <div class="footer-col">
      <h2>Resources</h2>
      <a href="docs.html">Documentation</a>
      <a href="examples.html">Examples</a>
      <a href="index.html#stdlib">Standard library</a>
      <a href="index.html#install">Install</a>
    </div>
    <div class="footer-col">
      <h2>Project</h2>
      <a href="https://github.com/Kenneth-Cho-InfoSec/valvet">GitHub</a>
      <a href="https://github.com/Kenneth-Cho-InfoSec/valvet/blob/main/LICENSE">License</a>
      <a href="https://github.com/Kenneth-Cho-InfoSec/valvet">Issues</a>
      <a href="privacy.html">Privacy policy</a>
    </div>
  </div>
  <div class="container footer-bottom">
    <p>Copyright 2026 Kenneth Cho. All rights reserved.</p>
    <p class="footer-built">Compiled through C11. Shipped as one binary.</p>
  </div>
  <div class="footer-wordmark" aria-hidden="true">VALVET</div>
</footer>"""


def page_shell(title: str, description: str, body_class: str, content: str, active: str = "", canonical_path: str = "docs.html") -> str:
    canonical = f"https://kenneth-cho-infosec.github.io/Valvet/{canonical_path}"
    return f"""<!doctype html>
<html lang="en" data-theme="light">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(title)}</title>
<meta name="description" content="{esc(description)}">
<meta name="theme-color" content="#f7f3ff">
<link rel="canonical" href="{canonical}">
<meta property="og:title" content="{esc(title)}">
<meta property="og:description" content="{esc(description)}">
<meta property="og:type" content="website">
<meta property="og:url" content="{canonical}">
<meta property="og:site_name" content="Valvet">
<meta property="og:image" content="https://kenneth-cho-infosec.github.io/Valvet/assets/og.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="Valvet: Data work, compiled. A dark code window and lavender background.">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{esc(title)}">
<meta name="twitter:description" content="{esc(description)}">
<meta name="twitter:image" content="https://kenneth-cho-infosec.github.io/Valvet/assets/og.png">
<link rel="icon" href="assets/favicon.png" type="image/png">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500&family=Sora:wght@400;500;600;700;800&display=swap" rel="stylesheet">
<link rel="stylesheet" href="style.css">
<noscript>
<style>
  .theme-toggle, .menu-toggle, .back-to-top, .copy-btn, .code-tabs, .pipeline-rail, .stdlib-toolbar, .docs-search, .filter-row, .toast {{ display: none !important; }}
  .code-panel[hidden], .stage-panel[hidden] {{ display: block !important; }}
  .stage-panel[hidden] {{ display: grid !important; }}
  .code-panel {{ margin-bottom: 18px; }}
  .stage-panel {{ margin-bottom: 18px; }}
  .docs-toc summary, .examples-toc summary {{ display: block !important; }}
</style>
</noscript>
<script>
(function () {{
  document.documentElement.classList.add('js');
  try {{
    var saved = localStorage.getItem('valvet-theme');
    if (saved === 'dark' || saved === 'light') document.documentElement.dataset.theme = saved;
  }} catch (error) {{}}
}})();
</script>
</head>
<body class="{body_class}">
<a class="skip-link" href="#main">Skip to content</a>
{nav(active)}
<main id="main">
{content}
</main>
{footer()}
<div class="toast" role="status" aria-live="polite"></div>
<button class="back-to-top" type="button" aria-label="Back to top">
  <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6"/></svg>
</button>
<script src="app.js" defer></script>
</body>
</html>
"""


DOC_GROUPS = [
    ("Start", ["Getting started", "Script and explicit modes"]),
    (
        "Language",
        [
            "Lexical structure",
            "Values and types",
            "Variables",
            "Functions",
            "Operators and precedence",
            "Control flow",
            "Structs and enums",
            "Pattern matching",
            "Vectors",
            "Ownership",
            "References",
            "Memory model",
            "Modules",
            "Units of measure",
        ],
    ),
    (
        "Data stack",
        [
            "Frames and audited joins",
            "Forward differentiation",
            "Deterministic parallelism",
            "Standard library and REPL",
        ],
    ),
    ("Toolchain", ["Build pipeline", "Testing", "CLI", "Errors", "Scope"]),
]


def build_docs():
    raw = (SOURCE / "docs.txt").read_text(encoding="utf-8").splitlines()
    blocks = parse_blocks(raw)
    sections = split_sections(blocks)
    by_title = {section["title"]: section for section in sections}

    sidebar = []
    for group, titles in DOC_GROUPS:
        links = []
        for title in titles:
            section = by_title.get(title)
            if not section:
                continue
            links.append(f'<a href="#{section["id"]}" data-doc-link="{section["id"]}">{esc(title)}</a>')
        sidebar.append(
            f'<div class="toc-group"><p class="toc-group-title">{esc(group)}</p>{"".join(links)}</div>'
        )

    content = []
    for section in sections:
        body = render_blocks(section["blocks"], drop_first_id=True)
        content.append(
            f'<section class="doc-section" id="{section["id"]}" data-doc-section>'
            f"{body}</section>"
        )

    page = f"""
<div class="reading-progress" data-progress aria-hidden="true"></div>
<section class="page-hero docs-hero">
  <div class="container page-hero-inner">
    <h1>The Valvet language</h1>
    <p class="page-lede">From lexical structure to the standard library. Everything the compiler accepts, in one place.</p>
    <div class="page-meta"><span>Version 0.3.0</span><span>Updated 2026</span><span>27 stdlib modules</span></div>
    <div class="page-hero-actions">
      <a class="btn btn-primary" href="index.html#install">Install Valvet</a>
    </div>
  </div>
</section>
<div class="container docs-layout">
  <aside class="docs-sidebar">
    <div class="docs-search">
      <label for="docs-search">Search documentation</label>
      <input id="docs-search" type="search" placeholder="Frames, ownership, SQL…" autocomplete="off" spellcheck="false">
      <p class="search-status" data-search-status>{len(sections)} sections</p>
    </div>
    <nav class="toc-disclosure" aria-label="Documentation sections">
      <details class="docs-toc">
        <summary>On this page<svg class="toc-chevron" viewBox="0 0 16 16" aria-hidden="true"><path d="M4 6l4 4 4-4"/></svg></summary>
        <div class="toc-links">
          {''.join(sidebar)}
        </div>
      </details>
    </nav>
    <div class="sidebar-note">
      <p>Need the whole language at a glance?</p>
      <a href="examples.html">Start from a runnable example <span aria-hidden="true">→</span></a>
    </div>
  </aside>
  <article class="docs-content" id="docs-content">
    {''.join(content)}
  </article>
</div>"""

    (SITE / "docs.html").write_text(
        page_shell(
            "Documentation · Valvet",
            "The complete Valvet language guide: values and types, ownership, modules, units, frames, autodiff, the standard library, and the CLI.",
            "page-docs",
            page,
            "Docs",
            "docs.html",
        ),
        encoding="utf-8",
    )


EXAMPLE_CATEGORY = {
    "Hello": "basics",
    "Fibonacci and pattern matching": "basics",
    "Mean of a vector": "basics",
    "FizzBuzz": "basics",
    "Newton square root": "basics",
    "Vector dot product": "basics",
    "Shapes and areas": "basics",
    "Loop with a break value": "basics",
    "Strings": "basics",
    "Least-squares slope": "data",
    "Script and explicit modes": "language",
    "Units of measure": "language",
    "Frames and grouping": "data",
    "SQL over frames": "data",
}
CATEGORY_LABEL = {"basics": "Basics", "data": "Data", "language": "Language"}


def build_examples():
    raw = (SOURCE / "examples.txt").read_text(encoding="utf-8").splitlines()
    blocks = parse_blocks(raw)
    sections = split_sections(blocks)
    sections = [section for section in sections if section["title"] != "Examples"]

    sidebar = []
    cards = []
    counts = {"basics": 0, "data": 0, "language": 0}
    for section in sections:
        title = section["title"]
        category = EXAMPLE_CATEGORY.get(title, "basics")
        counts[category] += 1
        sidebar.append(
            f'<a href="#{section["id"]}" data-example-link="{section["id"]}" data-category="{category}">{esc(title)}</a>'
        )
        body = render_blocks(section["blocks"][1:])
        cards.append(
            f"""
<article class="example-card" id="{section["id"]}" data-example-card data-category="{category}">
  <header class="example-head">
    <h2>{esc(title)}</h2>
    <span class="example-tag">{CATEGORY_LABEL[category]}</span>
  </header>
  {body}
</article>"""
        )

    filters = [
        f'<button class="filter-btn is-active" type="button" data-filter="all">All <span>{len(sections)}</span></button>',
        f'<button class="filter-btn" type="button" data-filter="basics">Basics <span>{counts["basics"]}</span></button>',
        f'<button class="filter-btn" type="button" data-filter="data">Data <span>{counts["data"]}</span></button>',
        f'<button class="filter-btn" type="button" data-filter="language">Language <span>{counts["language"]}</span></button>',
    ]

    page = f"""
<div class="reading-progress" data-progress aria-hidden="true"></div>
<section class="page-hero examples-hero">
  <div class="container page-hero-inner">
    <h1>Examples</h1>
    <p class="page-lede">Every sample runs as shown with <code>valvet run</code>. Start with a mean, end with SQL over frames.</p>
    <div class="page-meta"><span>14 programs</span><span>Every output verified</span><span>No runtime</span></div>
  </div>
</section>
<div class="container examples-layout">
  <aside class="examples-sidebar">
    <nav class="toc-disclosure" aria-label="Examples">
      <details class="examples-toc">
        <summary>On this page<svg class="toc-chevron" viewBox="0 0 16 16" aria-hidden="true"><path d="M4 6l4 4 4-4"/></svg></summary>
        <div class="toc-links">
          {''.join(sidebar)}
        </div>
      </details>
    </nav>
    <div class="sidebar-note">
      <p>Want the rules behind the code?</p>
      <a href="docs.html">Read the language guide <span aria-hidden="true">→</span></a>
    </div>
  </aside>
  <div class="examples-content">
    <div class="filter-row" role="group" aria-label="Filter examples">
      {''.join(filters)}
    </div>
    <p class="sr-only" data-filter-status aria-live="polite">Showing all 14 examples</p>
    <div class="examples-list">
      {''.join(cards)}
    </div>
    <div class="empty-state" data-empty-state hidden>
      <p>No examples in this category.</p>
      <button class="btn btn-ghost" type="button" data-filter="all">Show all examples</button>
    </div>
  </div>
</div>"""

    (SITE / "examples.html").write_text(
        page_shell(
            "Examples · Valvet",
            "Runnable Valvet programs: vectors, pattern matching, units, frames, and SQL. Every output shown is the real program output.",
            "page-examples",
            page,
            "Examples",
            "examples.html",
        ),
        encoding="utf-8",
    )


def build_privacy():
    page = """
<div class="container privacy-layout">
  <article class="privacy-content">
    <section class="doc-section reveal-up" id="who-runs-this-site">
      <h2>Who runs this site</h2>
      <p>Valvet is an open source project maintained by Kenneth Cho. This policy covers the website at <code>kenneth-cho-infosec.github.io/Valvet</code>. It does not cover other sites or services that link from this one.</p>
      <p>The project has no account system. The best way to reach it is through <a href="https://github.com/Kenneth-Cho-InfoSec/valvet/issues">the Valvet issue tracker on GitHub, where you can ask questions</a>.</p>
    </section>
    <section class="doc-section reveal-up" id="what-this-site-collects">
      <h2>What this site collects</h2>
      <p>Valvet does not collect personal information through this website.</p>
      <ul>
        <li>No accounts, sign-up forms, contact forms, newsletters, or comments.</li>
        <li>No analytics, advertising, tracking pixels, or cross-site trackers.</li>
        <li>No cookies set by this site.</li>
        <li>Documentation search and example filters run in your browser. What you type is not sent to a server.</li>
        <li>Copy buttons use your browser's clipboard. The copied text stays on your device.</li>
      </ul>
      <p>Valvet does not sell, rent, or share personal information.</p>
    </section>
    <section class="doc-section reveal-up" id="local-storage">
      <h2>Local storage</h2>
      <p>The site stores one preference in <code>localStorage</code> under the key <code>valvet-theme</code>. Its value is <code>light</code> or <code>dark</code>, and it keeps your theme choice between visits. It is not sent to Valvet or to any third party.</p>
      <p>You can remove it by clearing site data for this domain in your browser.</p>
    </section>
    <section class="doc-section reveal-up" id="hosting">
      <h2>Hosting</h2>
      <p>This site is served by GitHub Pages. GitHub may collect technical request data such as IP address, user agent, and requested path to deliver the site and protect its infrastructure. Valvet does not receive or store those logs.</p>
      <p>GitHub's handling of that data is described in the <a href="https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement">GitHub Privacy Statement</a>, including how request data is processed.</p>
    </section>
    <section class="doc-section reveal-up" id="fonts">
      <h2>Fonts</h2>
      <p>The site loads Sora and JetBrains Mono from Google Fonts. To show the fonts, your browser requests files from <code>fonts.googleapis.com</code> and <code>fonts.gstatic.com</code>. Google receives technical request data such as your IP address and browser information.</p>
      <p>Google's handling of that data is described in the <a href="https://policies.google.com/privacy">Google Privacy Policy</a>. If you block Google Fonts, the site falls back to system fonts and remains usable.</p>
    </section>
    <section class="doc-section reveal-up" id="external-links">
      <h2>External links</h2>
      <p>The site links to GitHub for source code, issues, and the license. If you follow an external link, the destination site's privacy policy applies.</p>
    </section>
    <section class="doc-section reveal-up" id="your-choices">
      <h2>Your choices</h2>
      <p>You can clear the theme preference through your browser's site data controls. You can block Google Fonts with a content blocker or privacy setting and still use the site.</p>
      <p>Valvet has no account data, contact submissions, or analytics profile to export or delete. For data held by GitHub, use GitHub's privacy controls.</p>
    </section>
    <section class="doc-section reveal-up" id="children">
      <h2>Children</h2>
      <p>This site is a developer resource and is not directed at children under 13. It does not collect personal information from anyone, including children.</p>
    </section>
    <section class="doc-section reveal-up" id="changes">
      <h2>Changes</h2>
      <p>This policy may be updated when the site changes. The date at the top shows the latest revision. Material changes will be reflected on this page.</p>
    </section>
    <section class="doc-section reveal-up" id="contact">
      <h2>Contact</h2>
      <p>Questions or concerns about this policy can be raised in the <a href="https://github.com/Kenneth-Cho-InfoSec/valvet/issues">Valvet issue tracker</a>.</p>
    </section>
  </article>
  <aside class="privacy-aside">
    <nav class="toc-group" aria-label="Privacy sections">
      <p class="toc-group-title">On this page</p>
      <a href="#who-runs-this-site">Who runs this site</a>
      <a href="#what-this-site-collects">What this site collects</a>
      <a href="#local-storage">Local storage</a>
      <a href="#hosting">Hosting</a>
      <a href="#fonts">Fonts</a>
      <a href="#external-links">External links</a>
      <a href="#your-choices">Your choices</a>
      <a href="#children">Children</a>
      <a href="#changes">Changes</a>
      <a href="#contact">Contact</a>
    </nav>
    <div class="sidebar-note">
      <p>Questions about this policy?</p>
      <p>Open an issue on GitHub and describe what you need. This page is the only place the site explains its data practices.</p>
      <p>No account data is stored here, so there is nothing to export or delete.</p>
      <p>For data held by GitHub, use GitHub's privacy controls.</p>
      <a href="https://github.com/Kenneth-Cho-InfoSec/valvet/issues">Open an issue <span aria-hidden="true">→</span></a>
    </div>
  </aside>
</div>"""

    (SITE / "privacy.html").write_text(
        page_shell(
            "Privacy policy · Valvet",
            "How the Valvet website handles data: no accounts, no analytics, local theme storage, GitHub Pages hosting, and Google Fonts.",
            "page-privacy",
            """
<section class="page-hero privacy-hero">
  <div class="container page-hero-inner">
    <h1>Privacy policy</h1>
    <p class="page-lede">The short version: no accounts, no analytics, no advertising, and no tracking cookies. This page explains the few technical details that do apply.</p>
    <p class="page-meta">Last updated 20 October 2026. This policy applies to this website.</p>
  </div>
</section>""" + page,
            "",
            "privacy.html",
        ),
        encoding="utf-8",
    )


def main():
    SITE.mkdir(parents=True, exist_ok=True)
    build_docs()
    build_examples()
    build_privacy()
    print("built docs.html, examples.html, and privacy.html")


if __name__ == "__main__":
    main()
