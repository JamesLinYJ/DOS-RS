#!/usr/bin/env python3
"""Index POSIX.1-2024 headings, named interfaces, option shading and documented choices.

Use a local copy of the normative volumes. No standard prose is reproduced.
Rows remain open until their own evidence is recorded on both native architectures.
"""

import argparse
from collections import Counter
import csv
from html import unescape
from html.parser import HTMLParser
from pathlib import Path
import re

VOLUMES = ("basedefs", "functions", "utilities")
# XBD 1.8.1: CX and OB do not relax base conformance; OH marks a header
# inclusion, not the function's existence. OF is explicitly an XSI extension.
INFORMATIONAL = {"CX", "OB", "OH"}
TAG = re.compile(r"<[^>]*>", re.S)
MATCH = re.compile(r"\b(shall|implementation[\s-]+defined)\b", re.I)
SECTIONS = re.compile(r"(<h[1-6]\b.*?</h[1-6]>)", re.S | re.I)
NAMED = re.compile(r"NAME</h4>\s*<blockquote[^>]*>(.*?)</blockquote>", re.S | re.I)
SYNOPSIS = re.compile(
    r"SYNOPSIS</h4>\s*<blockquote[^>]*>(.*?)</blockquote>", re.S | re.I
)
ANCHOR = re.compile(r'\b(?:id|name)="(tag_[0-9_]+)"')


def plain(source):
    return " ".join(unescape(TAG.sub("", source)).split())


class Shading(HTMLParser):
    """Retain the option expression in scope for each run of text."""

    def __init__(self, source):
        super().__init__()
        self.runs = []
        self.active = []
        self.capturing_code = False
        self.code = ""
        self.feed(source)
        self.close()

    def handle_starttag(self, tag, pairs):
        attrs = dict(pairs)
        if tag == "sup":
            self.capturing_code = True
            self.code = ""
        elif tag == "img":
            image = attrs.get("src", "")
            if image.endswith("opt-start.gif"):
                self.active.append(self.code.strip("[] "))
            elif image.endswith("opt-end.gif") and self.active:
                self.active.pop()

    def handle_endtag(self, tag):
        if tag == "sup":
            self.capturing_code = False

    def handle_data(self, data):
        if self.capturing_code:
            self.code += data
        else:
            self.runs.append((tuple(self.active), data))


def condition(stack):
    return "&".join(c for c in stack if c and c not in INFORMATIONAL)


def reference_items(source):
    name = NAMED.search(source)
    if not name:
        return []
    aliases = re.split(r"\s+[—–]\s+", plain(name.group(1)), maxsplit=1)[0]
    aliases = [n.strip() for n in aliases.split(",") if n.strip()]
    syn = SYNOPSIS.search(source)
    shading = Shading(syn.group(1)) if syn else None
    result = []
    for name in aliases:
        key = name.strip("<>")
        pattern = re.compile(r"(?<![\w])" + re.escape(key) + r"(?![\w])")
        states = (
            {condition(codes) for codes, text in shading.runs if pattern.search(text)}
            if shading
            else set()
        )
        # Confirm the entire synopsis is conditional when a utility is printed
        # using another spelling, e.g. a single name covering several commands.
        if not states and shading:
            states = {condition(codes) for codes, text in shading.runs if text.strip()}
        scope = (
            "review"
            if not states
            else "base"
            if "" in states
            else " | ".join(sorted(states))
        )
        result.append((name, scope))
    return result


def index(spec, indices, directory, evidence):
    with evidence.open() as recorded:
        data = list(csv.DictReader(recorded, delimiter="\t"))
    results = {(row["source"], row["item"]): row for row in data}
    if len(results) != len(data):
        raise ValueError("duplicate conformance evidence key")

    def status(source, item):
        row = results.pop((source, item), None)
        return (
            (row["i686"], row["x86_64"], row["test"]) if row else ("open", "open", "")
        )

    directory.mkdir(parents=True, exist_ok=True)
    units = []
    clauses = []
    for volume in VOLUMES:
        toc = Path(indices[volume]).read_text()
        links = list(
            dict.fromkeys(
                re.findall(
                    r'href="\.\./(' + volume + r"/[A-Za-z0-9_.-]+\.html)#tag_", toc
                )
            )
        )
        for link in links:
            path = spec / link
            if not path.is_file():
                raise ValueError(f"missing normative page {link}")
            source = path.read_text()
            # The shell-language HTML concatenates the chapter and fourteen
            # special-builtin pages, each with a distinct navigation header.
            for page in source.split('<div class="NAVHEADER">'):
                if not re.search(r"<h[1-6]\b", page):
                    continue
                aliases = reference_items(page)
                if aliases:
                    page = page.split("The following sections are informative.", 1)[0]
                file_scope = ""
                if aliases and all(
                    scope not in ("base", "review") for _, scope in aliases
                ):
                    file_scope = " | ".join(sorted({scope for _, scope in aliases}))
                sections = SECTIONS.split(page)
                if aliases:
                    anchor = ANCHOR.search(sections[0])
                    root = link + ("#" + anchor.group(1) if anchor else "")
                    for name, scope in aliases:
                        units.append((volume, name, scope, root, *status(root, name)))
                info_level = None
                for heading, body in zip(sections[1::2], sections[2::2]):
                    anchor = ANCHOR.search(heading)
                    if not anchor:
                        continue
                    label = plain(heading).lower()
                    level = int(heading[2])
                    if info_level is not None and level <= info_level:
                        info_level = None
                    if label in (
                        "examples",
                        "rationale",
                        "application usage",
                        "future directions",
                        "change history",
                    ):
                        info_level = level
                    if info_level is not None:
                        continue
                    href = link + "#" + anchor.group(1)
                    if not aliases:
                        units.append(
                            (
                                volume,
                                anchor.group(1),
                                "chapter",
                                href,
                                *status(href, anchor.group(1)),
                            )
                        )
                    # A locator distinguishes repeated markers within the same
                    # published heading without copying the underlying prose.
                    occurrences = Counter()
                    for active, text in Shading(body).runs:
                        for hit in MATCH.finditer(text):
                            kind = (
                                "implementation-defined"
                                if hit[1].lower() != "shall"
                                else "shall"
                            )
                            occurrences[kind] += 1
                            scope = condition(active) or file_scope or "base"
                            clauses.append(
                                (
                                    href,
                                    kind,
                                    occurrences[kind],
                                    scope,
                                    *status(href, f"{kind}:{occurrences[kind]}"),
                                )
                            )
    if results:
        raise ValueError(f"unknown standard evidence locators: {sorted(results)}")
    for name, columns, records in [
        (
            "units.tsv",
            ["volume", "unit", "scope", "source", "i686", "x86_64", "test"],
            units,
        ),
        (
            "obligations.tsv",
            ["source", "marker", "occurrence", "scope", "i686", "x86_64", "test"],
            clauses,
        ),
    ]:
        with (directory / name).open("w", newline="") as out:
            writer = csv.writer(out, delimiter="\t", lineterminator="\n")
            writer.writerow(columns)
            writer.writerows(records)
    print(
        f"{len(units)} named units and chapter headings; {len(clauses)} normative markers"
    )


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument(
        "--spec",
        type=Path,
        required=True,
        help="root of the unmodified online Issue 8 HTML tree",
    )
    cli.add_argument("--output", type=Path, required=True)
    cli.add_argument(
        "--evidence",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "tests/posix-2024/evidence.tsv",
    )
    for volume in VOLUMES:
        cli.add_argument("--" + volume, type=Path)
    args = cli.parse_args()
    indices = {
        vol: getattr(args, vol) or args.spec / vol / "contents.html" for vol in VOLUMES
    }
    index(args.spec, indices, args.output, args.evidence)
