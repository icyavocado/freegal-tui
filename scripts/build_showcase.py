#!/usr/bin/env python3
import json
import urllib.request
from pathlib import Path

REPO = "icyavocado/freegal-tui"
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
MANIFEST = ROOT / "/tmp/opencode/showcase/manifest.json"
OUT = ROOT / "docs" / "index.html"


def baked_stars() -> str:
    try:
        req = urllib.request.Request(
            f"https://api.github.com/repos/{REPO}",
            headers={"User-Agent": "build-showcase"},
        )
        with urllib.request.urlopen(req, timeout=10) as r:
            return str(json.loads(r.read().decode())["stargazers_count"])
    except Exception:
        return "—"


TARGETS = {
    "01_home": {"TAB:0": "01_home", "TAB:1": "11_mymusic", "TAB:2": "13_queue", "TAB:3": "14_history", "TAB:4": "15_downloads", "CHIP:0": "04_home_artists", "CHIP:1": "03_home_trending", "CHIP:2": "05_home_new_albums", "CHIP:3": "06_home_albums", "CHIP:4": "07_home_playlists", "CHIP:5": "09_genres", "SEARCHSTRIP": "01_home"},
    "02_home_songs": {"CRUMBH:0": "01_home", "TAB:0": "01_home"},
    "03_home_trending": {"CRUMBH:0": "01_home"},
    "04_home_artists": {"CRUMBH:0": "01_home"},
    "05_home_new_albums": {"CRUMBH:0": "01_home"},
    "06_home_albums": {"CRUMBH:0": "01_home"},
    "07_home_playlists": {"CRUMBH:0": "01_home"},
    "08_home_library_playlist": {"CRUMBH:0": "01_home"},
    "09_genres": {"CRUMBH:0": "01_home"},
    "10_genre_pop": {"CRUMBH:0": "01_home", "CRUMBH:1": "09_genres"},
    "11_mymusic": {"TAB:0": "01_home", "TAB:1": "11_mymusic", "TAB:2": "13_queue", "TAB:3": "14_history", "TAB:4": "15_downloads", "JUMPSTRIP": "01_home"},
    "12_mymusic_wishlist": {"CRUMBM:0": "11_mymusic"},
    "13_queue": {},
    "14_history": {},
    "15_downloads": {},
    "16_help": {},
    "17_audio": {},
}


def main() -> None:
    m = json.load(open(MANIFEST))
    m["screens"].sort(key=lambda s: s["name"])
    names = {s["name"] for s in m["screens"]}
    targets = {
        screen: {tag: tgt for tag, tgt in zones.items() if tgt in names}
        for screen, zones in TARGETS.items()
        if screen in names
    }
    stars = baked_stars()
    payload = json.dumps(
        {"size": m["size"], "screens": m["screens"], "targets": targets, "stars": stars},
        ensure_ascii=False,
    )
    html = TEMPLATE.replace("__PAYLOAD__", payload).replace("__REPO__", REPO)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(html)
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes) stars={stars}")


TEMPLATE = r"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>freegal-tui — your library, in the terminal</title>
<meta name="description" content="Interactive terminal demo of freegal-tui: browse, stream and download Freegal Music without the web UI. Rust + ratatui + mpv.">
<meta property="og:title" content="freegal-tui — your library, in the terminal">
<meta property="og:description" content="Browse, stream and download Freegal Music without the web UI. Rust + ratatui + mpv.">
<meta property="og:type" content="website">
<meta property="og:url" content="https://icyavocado.github.io/freegal-tui/">
<meta property="og:image" content="https://icyavocado.github.io/freegal-tui/og-card.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="freegal-tui — your library, in the terminal">
<meta name="twitter:description" content="Browse, stream and download Freegal Music without the web UI.">
<meta name="twitter:image" content="https://icyavocado.github.io/freegal-tui/og-card.png">
<style>
:root {
  --bg: #1a1b26; --bg-card: #1e2030; --fg: #c0caf5; --dim: #565f89;
  --accent: #7aa2f7; --accent2: #bb9af7; --green: #9ece6a; --border: #3b4261;
  --ch: 9.6px; --lh: 18px;
  --mono: ui-monospace, 'JetBrains Mono', 'SF Mono', Menlo, Consolas, monospace;
}
* { margin: 0; padding: 0; box-sizing: border-box; }
html { background: var(--bg); }
body {
  min-height: 100vh; background: var(--bg); color: var(--fg);
  font-family: var(--mono); display: flex; flex-direction: column; align-items: center;
  padding: 40px 16px 60px;
}
header { width: 100%; max-width: 1100px; display: flex; align-items: baseline; gap: 18px; margin-bottom: 26px; }
header h1 { font-size: 26px; letter-spacing: -0.5px; }
header h1 span { color: var(--accent); }
header a {
  color: var(--accent); text-decoration: none; font-size: 14px; border: 1px solid var(--border);
  border-radius: 20px; padding: 4px 14px; white-space: nowrap;
}
header a:hover { border-color: var(--accent); }
.tagline { color: var(--dim); font-size: 13px; }
.stage {
  width: 100%; max-width: 1100px; overflow: hidden;
  border-radius: 10px; box-shadow: 0 0 0 1px var(--border), 0 20px 60px rgba(0,0,0,.5);
}
.termbar {
  height: 34px; background: var(--bg-card); border-bottom: 1px solid var(--border);
  display: flex; align-items: center; gap: 8px; padding: 0 14px;
}
.termbar .dot { width: 11px; height: 11px; border-radius: 50%; background: var(--border); }
.termbar .dot:nth-child(1) { background: #f7768e; }
.termbar .dot:nth-child(2) { background: #e0af68; }
.termbar .dot:nth-child(3) { background: #9ece6a; }
.termbar .ttl { margin-left: 10px; color: var(--dim); font-size: 12px; }
.termwrap { background: #12131c; padding: 10px 0; overflow-x: auto; }
.screen { position: relative; width: max-content; margin: 0 auto; }
pre.out {
  font-family: var(--mono); font-size: 15px; line-height: var(--lh);
  color: var(--fg); white-space: pre; letter-spacing: 0;
}
pre.out .z {
  position: absolute; border-radius: 4px; cursor: pointer; z-index: 3;
}
pre.out .z:hover { background: rgba(122,162,247,.22); box-shadow: 0 0 0 1px rgba(122,162,247,.55); }
nav.tray {
  width: 100%; max-width: 1100px; margin-top: 18px; display: flex; flex-wrap: wrap; gap: 8px;
}
nav.tray button {
  font-family: var(--mono); font-size: 12px; color: var(--dim); background: transparent;
  border: 1px solid var(--border); border-radius: 6px; padding: 6px 12px; cursor: pointer;
}
nav.tray button:hover { color: var(--fg); border-color: var(--accent); }
nav.tray button.on { color: var(--bg); background: var(--accent); border-color: var(--accent); font-weight: 700; }
#autoBtn {
  margin-left: auto; font-family: var(--mono); font-size: 11px; color: var(--dim);
  background: transparent; border: 1px solid var(--border); border-radius: 6px;
  padding: 3px 10px; cursor: pointer;
}
#autoBtn:hover { color: var(--fg); border-color: var(--accent); }
#autoBtn.on { color: var(--bg); background: var(--accent); border-color: var(--accent); }
footer { margin-top: 26px; color: var(--dim); font-size: 12.5px; text-align: center; line-height: 2; }
footer a { color: var(--accent); text-decoration: none; }
</style>
</head>
<body>
<header>
  <h1>free<span>gal</span>-tui</h1>
  <span class="tagline">Your library's streaming catalogue, in the terminal.</span>
  <a href="https://github.com/__REPO__" target="_blank" rel="noopener">★ <span id="stars">…</span>&nbsp; GitHub</a>
</header>

<div class="stage">
  <div class="termbar"><span class="dot"></span><span class="dot"></span><span class="dot"></span><span class="ttl" id="ttl">freegal</span><button id="autoBtn" title="toggle the demo loop">⏸ auto</button></div>
  <div class="termwrap">
    <div class="screen" id="screenbox"><pre class="out" id="out"></pre></div>
  </div>
</div>

<nav class="tray" id="tray"></nav>

<footer>
  it loops itself until you touch it (⏸ button or any click stops the loop) · ←/→ switch screens · blue outlines are clickable (chips, tabs, breadcrumbs, rows) just like in the app<br>
  <a href="https://github.com/__REPO__/blob/main/docs/screenshot-home.png" target="_blank" rel="noopener">real screenshot</a> ·
  <a href="https://github.com/__REPO__" target="_blank" rel="noopener">source</a> — Rust + ratatui + mpv, streams with your library card
</footer>

<script>
const DATA = __PAYLOAD__;
const LH = 18;
const LABELS = {
  "00_login": "Login", "01_home": "Home", "02_home_songs": "Featured Songs",
  "03_home_trending": "Trending", "04_home_artists": "Artists", "05_home_new_albums": "New Albums",
  "06_home_albums": "Featured Albums", "07_home_playlists": "Playlists", "08_home_library_playlist": "Library Playlist",
  "09_genres": "Genres", "10_genre_pop": "Genre: Pop", "11_mymusic": "My Music",
  "12_mymusic_wishlist": "Wishlist", "13_queue": "Queue", "14_history": "History",
  "15_downloads": "Downloads", "16_help": "Help", "17_audio": "Audio",
  "18_home_featured_album": "Featured Album", "19_home_artist_albums": "Artist Albums",
  "20_home_new_album": "New Album", "21_home_playlist": "Playlist Songs"
};
const box = document.getElementById("screenbox"), out = document.getElementById("out");
const tray = document.getElementById("tray"), ttl = document.getElementById("ttl");
const meas = document.createElement("span");
meas.textContent = "X".repeat(100);
meas.style.cssText = "position:absolute;visibility:hidden;white-space:pre";
out.appendChild(meas);
const CH = meas.offsetWidth / 100 || 9.6;
out.removeChild(meas);
let cur = 1;
DATA.screens.forEach((s, i) => {
  const b = document.createElement("button");
  b.textContent = LABELS[s.name] || s.name;
  b.onclick = () => show(i);
  tray.appendChild(b);
});
function show(i) {
  cur = i;
  const s = DATA.screens[i];
  out.textContent = s.text;
  out.style.width = DATA.size[0] + "ch"; out.style.height = DATA.size[1] * LH + "px";
  ttl.textContent = "freegal — " + LABELS[s.name] + "  ·  100×32";
  box.querySelectorAll(".z").forEach(e => e.remove());
  (s.zones || []).forEach(z => {
    const tagged = (DATA.targets[s.name] || {})[z.tag];
    const tgt = tagged || (z.tag.startsWith("ROW:") ? z.tag.slice(4) : null);
    const idx = tgt == null ? -1 : DATA.screens.findIndex(x => x.name === tgt);
    if (idx < 0) return;
    const d = document.createElement("span");
    d.className = "z";
    d.style.left = z.x * CH + "px"; d.style.top = z.y * LH + "px";
    d.style.width = z.w * CH + "px"; d.style.height = z.h * LH + "px";
    d.title = LABELS[DATA.screens[idx].name] || DATA.screens[idx].name;
    d.onclick = () => show(idx);
    box.appendChild(d);
  });
  [...tray.children].forEach((b, j) => b.classList.toggle("on", j === i));
}
const AUTO_MS = 4000;
const autoBtn = document.getElementById("autoBtn");
let timer = null;
function startAuto() {
  if (timer) return;
  timer = setInterval(() => show((cur + 1) % DATA.screens.length), AUTO_MS);
  autoBtn.textContent = "⏸ auto"; autoBtn.classList.add("on");
}
function stopAuto() {
  if (!timer) return;
  clearInterval(timer); timer = null;
  autoBtn.textContent = "▶ auto"; autoBtn.classList.remove("on");
}
autoBtn.onclick = () => (timer ? stopAuto() : startAuto());
document.addEventListener("pointerdown", e => {
  if (e.target.id !== "autoBtn") stopAuto();
});
document.addEventListener("keydown", e => {
  stopAuto();
  if (e.key === "ArrowRight") show(Math.min(cur + 1, DATA.screens.length - 1));
  else if (e.key === "ArrowLeft") show(Math.max(cur - 1, 0));
  else if (e.key === "Escape") show(1);
});
const el = document.getElementById("stars");
el.textContent = DATA.stars;
fetch("https://api.github.com/repos/__REPO__")
  .then(r => r.json()).then(j => { if (j.stargazers_count != null) el.textContent = j.stargazers_count; })
  .catch(() => {});
show(cur);
if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) startAuto();
</script>
</body>
</html>
"""

if __name__ == "__main__":
    main()