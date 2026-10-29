#!/usr/bin/env python3
"""Strip the deployed Valvet Pages site to readable plain-text files."""

import hashlib
import html
import re
import shutil
import sys
from html.parser import HTMLParser
from pathlib import Path

FETCHED = "2026-10-20"
SITE = "https://kenneth-cho-infosec.github.io/Valvet/"
PAGES = [
    ("index.html", "index.txt"),
    ("docs.html", "docs.txt"),
    ("examples.html", "examples.txt"),
]
SKIP_TAGS = {"script", "style", "svg", "button", "nav"}
BLOCK_TAGS = {
    "html", "head", "body", "main", "div", "section", "header", "footer",
    "article", "aside", "p", "ul", "ol", "table", "tr", "pre", "blockquote",
}
CODE_LANG = [
    ("valvet", "valvet"),
    ("bash", "bash"),
    ("output", "text"),
    ("error", "text"),
]


def clean_inline(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def code_lang(label: str) -> str:
    low = label.lower()
    for needle, lang in CODE_LANG:
        if needle in low:
            return lang
    return "text"


def fence_safe(text: str) -> str:
    fence = "```"
    while fence in text:
        fence += "`"
    return fence


class Converter(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.lines = []
        self.cur = []
        self.skip_tag = None
        self.skip_depth = 0
        self.in_title = False
        self.in_pre = False
        self.pre = []
        self.in_install = False
        self.install = []
        self.in_code_box = False
        self.in_chead = False
        self.chead = []
        self.code_label = None
        self.heading_level = 0
        self.heading_id = None
        self.headings = []
        self.table_rows = []
        self.row = None
        self.cell = None
        self.cell_header = False
        self.list_depth = 0
        self.inline_code = 0
        self.link_stack = []
        self.code_blocks = 0
        self.images = []

    def inline(self, text):
        if self.cell is not None:
            self.cell.append(text)
        else:
            self.cur.append(text)

    def flush(self):
        text = clean_inline("".join(self.cur))
        if text:
            self.lines.append(text)
        self.cur = []

    def blank(self):
        if self.lines and self.lines[-1] != "":
            self.lines.append("")

    def emit_heading(self, level, text, anchor):
        self.blank()
        self.lines.append("#" * level + " " + text)
        self.blank()
        self.headings.append((level, text, anchor))

    def emit_code(self, text, label):
        self.flush()
        self.blank()
        label = label.strip(" :") or "Code"
        body = text.replace("\r\n", "\n").replace("\r", "\n").rstrip("\n")
        fence = fence_safe(body)
        self.lines.append(f"{label}:")
        self.lines.append(fence + code_lang(label))
        self.lines.extend(body.split("\n") if body else [""])
        self.lines.append(fence)
        self.blank()
        self.code_blocks += 1

    def emit_table(self):
        self.flush()
        self.blank()
        rows = [r for r in self.table_rows if r]
        if rows:
            width = max(len(r) for r in rows)
            rows = [r + [""] * (width - len(r)) for r in rows]
            self.lines.append("| " + " | ".join(rows[0]) + " |")
            self.lines.append("|" + "|".join([" --- "] * width) + "|")
            for row in rows[1:]:
                self.lines.append("| " + " | ".join(row) + " |")
            self.blank()
        self.table_rows = []

    def handle_starttag(self, tag, attrs):
        if self.skip_tag:
            if tag == self.skip_tag:
                self.skip_depth += 1
            return
        if tag in SKIP_TAGS:
            self.skip_tag = tag
            self.skip_depth = 1
            return
        attr = dict(attrs)
        classes = set((attr.get("class") or "").split())

        if tag == "title":
            self.in_title = True
            return
        if tag == "img":
            self.images.append(attr.get("src", ""))
            return
        if tag == "br":
            if self.in_pre:
                self.pre.append("\n")
            elif self.in_install:
                self.install.append("\n")
            elif self.cell is not None:
                self.cell.append(" ")
            else:
                self.flush()
            return
        if tag == "pre":
            self.flush()
            self.in_pre = True
            self.pre = []
            return
        if tag == "div" and "install" in classes:
            self.flush()
            self.in_install = True
            self.install = []
            return
        if tag == "div" and "code" in classes:
            self.flush()
            self.in_code_box = True
            self.code_label = None
            return
        if tag == "div" and "chead" in classes:
            self.in_chead = True
            self.chead = []
            return
        if tag in ("h1", "h2", "h3", "h4", "h5", "h6"):
            self.flush()
            self.heading_level = int(tag[1])
            self.heading_id = attr.get("id")
            return
        if tag == "table":
            self.flush()
            self.blank()
            self.table_rows = []
            return
        if tag == "tr":
            self.flush()
            self.row = []
            return
        if tag in ("th", "td"):
            self.flush()
            self.cell = []
            self.cell_header = tag == "th"
            return
        if tag in ("ul", "ol"):
            self.flush()
            self.list_depth += 1
            return
        if tag == "li":
            self.flush()
            self.cur.append("  " * max(0, self.list_depth - 1) + "- ")
            return
        if tag == "a":
            self.link_stack.append(attr.get("href", ""))
            self.inline("[")
            return
        if tag == "code":
            if not (self.in_pre or self.in_install):
                self.inline("`")
                self.inline_code += 1
            return
        if tag in BLOCK_TAGS:
            self.flush()
            self.blank()

    def handle_endtag(self, tag):
        if self.skip_tag:
            if tag == self.skip_tag:
                self.skip_depth -= 1
                if self.skip_depth == 0:
                    self.skip_tag = None
            return
        if tag == "title":
            self.in_title = False
            return
        if tag == "pre":
            text = "".join(self.pre)
            self.in_pre = False
            self.pre = []
            self.emit_code(text, self.code_label or "Code")
            self.code_label = None
            return
        if tag == "div" and self.in_install:
            text = "".join(self.install)
            self.in_install = False
            self.install = []
            self.emit_code(text, "Bash")
            return
        if tag == "div" and self.in_chead:
            self.code_label = clean_inline("".join(self.chead))
            self.in_chead = False
            self.chead = []
            return
        if tag == "div" and self.in_code_box:
            self.in_code_box = False
            return
        if tag in ("h1", "h2", "h3", "h4", "h5", "h6"):
            text = clean_inline("".join(self.cur))
            self.cur = []
            if text:
                self.emit_heading(self.heading_level, text, self.heading_id)
            self.heading_level = 0
            self.heading_id = None
            return
        if tag == "table":
            self.emit_table()
            return
        if tag == "tr":
            if self.row is not None:
                self.table_rows.append(self.row)
                self.row = None
            return
        if tag in ("th", "td"):
            text = clean_inline("".join(self.cell or []))
            text = text.replace("|", "\\|")
            if self.row is not None:
                self.row.append(text)
            self.cell = None
            return
        if tag == "li":
            self.flush()
            return
        if tag in ("ul", "ol"):
            self.flush()
            self.list_depth = max(0, self.list_depth - 1)
            self.blank()
            return
        if tag == "a":
            href = self.link_stack.pop() if self.link_stack else ""
            self.inline(f"]({href})" if href else "]")
            return
        if tag == "code":
            if not (self.in_pre or self.in_install):
                self.inline("`")
                self.inline_code = max(0, self.inline_code - 1)
            return
        if tag in BLOCK_TAGS:
            self.flush()
            self.blank()

    def handle_data(self, data):
        if self.skip_tag:
            return
        if self.in_title:
            self.title = getattr(self, "title", "") + data
            return
        if self.in_pre:
            self.pre.append(data)
            return
        if self.in_install:
            self.install.append(data)
            return
        if self.in_chead:
            self.chead.append(data)
            return
        if self.cell is not None:
            self.cell.append(data)
            return
        self.cur.append(data)

    def result(self):
        self.flush()
        clean = []
        for line in self.lines:
            if line == "" and (not clean or clean[-1] == ""):
                continue
            clean.append(line)
        while clean and clean[0] == "":
            clean.pop(0)
        while clean and clean[-1] == "":
            clean.pop()
        return "\n".join(clean) + "\n"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def nav_links(raw):
    m = re.search(r"<nav\b.*?</nav>", raw, re.S)
    if not m:
        return []
    out = []
    for href, body in re.findall(r'<a\b[^>]*href="([^"]*)"[^>]*>(.*?)</a>', m.group(0), re.S):
        text = html.unescape(re.sub(r"<[^>]+>", "", body)).strip()
        out.append((text, href))
    return out


def main():
    src = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    dst = Path(sys.argv[2] if len(sys.argv) > 2 else "text")
    dst.mkdir(parents=True, exist_ok=True)

    summaries = []
    nav_seen = None
    for html_name, txt_name in PAGES:
        path = src / html_name
        raw = path.read_text(encoding="utf-8")
        conv = Converter()
        conv.feed(raw)
        conv.close()
        body = conv.result()
        title = clean_inline(getattr(conv, "title", "")) or html_name
        header = (
            f"Source: {SITE}{html_name}\n"
            f"Fetched: {FETCHED}\n"
            f"Page title: {title}\n\n"
        )
        (dst / txt_name).write_text(header + body, encoding="utf-8")
        nav = nav_links(raw)
        if nav_seen is None:
            nav_seen = nav
        summaries.append({
            "page": txt_name,
            "source": html_name,
            "sha256": sha256(path),
            "title": title,
            "headings": conv.headings,
            "code_blocks": conv.code_blocks,
            "words": len(re.findall(r"\S+", body)),
            "chars": len(body),
            "nav": nav,
        })

    all_parts = []
    all_parts.append("VALVET WEBSITE / ALL PAGES")
    all_parts.append("")
    all_parts.append(f"Live site: {SITE}")
    all_parts.append("Source branch: website @ e040a3a")
    all_parts.append(f"Fetched: {FETCHED}")
    all_parts.append("")
    all_parts.append("=" * 72)
    for idx, item in enumerate(summaries, 1):
        body_path = dst / item["page"]
        body = body_path.read_text(encoding="utf-8")
        if body.startswith("Source: "):
            body = body.split("\n", 4)[4]
        title = item["page"].replace(".txt", "").upper()
        all_parts.append("")
        all_parts.append(f"PAGE {idx}/{len(summaries)}: {title}")
        all_parts.append(f"Source: {SITE}{item['source']}")
        all_parts.append(f"Page title: {item['title']}")
        all_parts.append("=" * 72)
        all_parts.append("")
        all_parts.append(body.rstrip("\n"))
        all_parts.append("")
    (dst / "all-pages.txt").write_text("\n".join(all_parts).rstrip("\n") + "\n", encoding="utf-8")

    style = src / "style.css"
    if style.exists():
        css = style.read_text(encoding="utf-8")
        (dst / "style-css.txt").write_text(
            f"Source: {SITE}style.css\nFetched: {FETCHED}\n\n" + css,
            encoding="utf-8",
        )

    out = []
    out.append("VALVET WEBSITE TEXT SNAPSHOT")
    out.append("")
    out.append(f"Live site: {SITE}")
    out.append("Source branch: website @ e040a3a")
    out.append(f"Fetched: {FETCHED}")
    out.append("")
    out.append("PURPOSE")
    out.append("  Plain-text extraction of the deployed Pages site, for a website revamp.")
    out.append("  HTML tags, navigation chrome, buttons, and scripts are removed.")
    out.append("  Section hierarchy, paragraphs, lists, tables, links, and code blocks are kept.")
    out.append("")
    out.append("FILES")
    for item in summaries:
        out.append(f"  {item['page']:<14} {item['words']:>5} words, {item['code_blocks']:>2} code blocks, {item['chars']:>6} chars")
    out.append(f"  {'all-pages.txt':<14} all three pages in one file")
    out.append(f"  {'style-css.txt':<14} raw stylesheet, unchanged")
    out.append("  site-outline.txt  this file")
    out.append("  README.txt       provenance and refresh steps")
    out.append("  _extract_pages.py  extraction script")
    out.append("")
    out.append("FETCHED SOURCE HASHES (SHA-256)")
    for item in summaries:
        out.append(f"  {item['sha256']}  {item['source']}")
    if style.exists():
        out.append(f"  {sha256(style)}  style.css")
    out.append("")
    out.append("GLOBAL NAVIGATION")
    for text, href in (nav_seen or []):
        out.append(f"  {text} -> {href}")
    out.append("")
    out.append("PAGE OUTLINE")
    for item in summaries:
        out.append("")
        out.append(f"  {item['page']}  ({item['title']})")
        for level, text, anchor in item["headings"]:
            suffix = f"  [#{anchor}]" if anchor else ""
            out.append(f"    {'  ' * (level - 1)}{'#' * level} {text}{suffix}")
    out.append("")
    out.append("NOTES")
    out.append("  * The live pages are byte-identical to the site/ directory on the website branch.")
    out.append("  * Internal anchor links are preserved as [text](#anchor).")
    out.append("  * Code block labels are the original UI labels (Input, Output, Bash, Error).")
    out.append("  * The logo asset is not duplicated here; it is site/assets/logo.png upstream.")
    out.append("")
    (dst / "site-outline.txt").write_text("\n".join(out), encoding="utf-8")

    readme = [
        "VALVET WEBSITE: PLAIN-TEXT SNAPSHOT",
        "",
        "What this is",
        "  The deployed Valvet Pages site stripped to plain text for a revamp.",
        "  HTML tags, scripts, buttons, and navigation chrome are removed.",
        "  Headings, paragraphs, lists, tables, links, and code blocks remain.",
        "",
        "Live site",
        f"  {SITE}",
        "",
        "Files",
        "  index.txt        home page",
        "  docs.txt         language documentation",
        "  examples.txt     runnable examples",
        "  all-pages.txt    the three pages in one file",
        "  style-css.txt    the deployed stylesheet, unchanged",
        "  site-outline.txt page and heading inventory, source hashes, nav",
        "  _extract_pages.py  regenerates these files from downloaded HTML",
        "",
        "Provenance",
        "  Fetched 2026-10-20. The live files are byte-identical to the",
        "  site/ directory on the website branch at commit e040a3a.",
        "",
        "Refresh",
        "  mkdir -p /tmp/valvet-pages && cd /tmp/valvet-pages",
        "  curl -fsSLO https://kenneth-cho-infosec.github.io/Valvet/index.html",
        "  curl -fsSLO https://kenneth-cho-infosec.github.io/Valvet/docs.html",
        "  curl -fsSLO https://kenneth-cho-infosec.github.io/Valvet/examples.html",
        "  curl -fsSLO https://kenneth-cho-infosec.github.io/Valvet/style.css",
        "  python3 /path/to/source-text/_extract_pages.py . /path/to/source-text",
        "",
    ]
    (dst / "README.txt").write_text("\n".join(readme), encoding="utf-8")
    shutil.copyfile(__file__, dst / "_extract_pages.py")

    print(f"wrote {len(summaries) + 5} files to {dst}")
    for item in summaries:
        print(f"  {item['page']}: {item['words']} words, {item['code_blocks']} code blocks")


if __name__ == "__main__":
    main()
