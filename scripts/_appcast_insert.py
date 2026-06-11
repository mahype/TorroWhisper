#!/usr/bin/env python3
"""
Prepend a <item> entry to a Sparkle appcast.xml. Everything comes in via
env vars so the caller (a shell script) doesn't have to juggle quoting.

Required env:
  APPCAST_PATH, VERSION, RELEASE_NOTES_URL, DMG_URL, DMG_LENGTH,
  DMG_ED_SIGNATURE, MIN_SYSTEM_VERSION, PUB_DATE

Optional env:
  RELEASE_NOTES_MD   Markdown release notes. When set, they are converted to
                     simple HTML and embedded as <description> so Sparkle shows
                     them inline. Without it, the item falls back to
                     <sparkle:releaseNotesLink>, which makes Sparkle render the
                     FULL release-notes web page (GitHub chrome and all) inside
                     the update dialog — avoid that for real releases.
"""
import html
import os
import re
import sys
from xml.etree import ElementTree as ET

NS_SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", NS_SPARKLE)


def require(name):
    value = os.environ.get(name)
    if not value:
        sys.exit(f"error: {name} is required")
    return value


def render_inline(text):
    """Escape, then convert the small markdown subset GitHub's generated
    release notes actually use: bold, code spans, [text](url), bare URLs."""
    text = html.escape(text, quote=False)
    text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = re.sub(r"`([^`]+)`", r"<code>\1</code>", text)
    text = re.sub(
        r"\[([^\]]+)\]\((https?://[^)\s]+)\)", r'<a href="\2">\1</a>', text
    )
    # Autolink bare URLs, but not ones already inside an href="..." or >...</a>.
    text = re.sub(
        r'(?<!["=>])(https?://[^\s<]+)', r'<a href="\1">\1</a>', text
    )
    return text


def markdown_to_html(md):
    out = []
    in_list = False

    def close_list():
        nonlocal in_list
        if in_list:
            out.append("</ul>")
            in_list = False

    for raw in md.splitlines():
        line = raw.rstrip()
        if not line.strip():
            close_list()
            continue
        heading = re.match(r"(#{1,6})\s+(.*)", line)
        if heading:
            close_list()
            # Demote one level: the Sparkle dialog is small, h1 is shouty.
            level = min(len(heading.group(1)) + 1, 6)
            out.append(f"<h{level}>{render_inline(heading.group(2))}</h{level}>")
            continue
        bullet = re.match(r"\s*[-*]\s+(.*)", line)
        if bullet:
            if not in_list:
                out.append("<ul>")
                in_list = True
            out.append(f"<li>{render_inline(bullet.group(1))}</li>")
            continue
        close_list()
        out.append(f"<p>{render_inline(line)}</p>")
    close_list()
    return "\n".join(out)


def main():
    path = require("APPCAST_PATH")
    version = require("VERSION")
    notes_url = require("RELEASE_NOTES_URL")
    dmg_url = require("DMG_URL")
    dmg_len = require("DMG_LENGTH")
    ed_sig = require("DMG_ED_SIGNATURE")
    min_sys = require("MIN_SYSTEM_VERSION")
    pub_date = require("PUB_DATE")
    notes_md = os.environ.get("RELEASE_NOTES_MD", "").strip()

    tree = ET.parse(path)
    channel = tree.getroot().find("channel")
    if channel is None:
        sys.exit("error: <channel> not found in appcast")

    if any(
        elem.findtext(f"{{{NS_SPARKLE}}}shortVersionString") == version
        for elem in channel.findall("item")
    ):
        print(f"appcast already contains version {version}; skipping", file=sys.stderr)
        sys.exit(2)

    item = ET.Element("item")
    ET.SubElement(item, "title").text = f"Version {version}"
    ET.SubElement(item, "pubDate").text = pub_date
    ET.SubElement(item, f"{{{NS_SPARKLE}}}shortVersionString").text = version
    ET.SubElement(item, f"{{{NS_SPARKLE}}}version").text = version
    if notes_md:
        # Sparkle prefers releaseNotesLink over description when both exist,
        # so embed ONLY the description.
        ET.SubElement(item, "description").text = markdown_to_html(notes_md)
    else:
        ET.SubElement(item, f"{{{NS_SPARKLE}}}releaseNotesLink").text = notes_url
    ET.SubElement(item, f"{{{NS_SPARKLE}}}minimumSystemVersion").text = min_sys
    ET.SubElement(
        item,
        "enclosure",
        {
            "url": dmg_url,
            f"{{{NS_SPARKLE}}}edSignature": ed_sig,
            "length": dmg_len,
            "type": "application/octet-stream",
        },
    )

    last_meta_idx = 0
    for idx, elem in enumerate(list(channel)):
        if elem.tag != "item":
            last_meta_idx = idx
    channel.insert(last_meta_idx + 1, item)

    ET.indent(tree, space="  ")
    tree.write(path, encoding="UTF-8", xml_declaration=True)


if __name__ == "__main__":
    main()
