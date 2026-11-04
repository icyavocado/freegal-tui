# The showcase site

`docs/index.html` is a self-contained, dependency-free page that shows the real
freegal TUI screens as text and lets visitors click around inside them. The
screens are captured from the actual app on this machine (signed in, real
feeds), then baked into a single static HTML file.

Live pages are driven from generated assets — **never hand-edit
`docs/index.html`**; change the generator or the capture step instead.

## What is generated

| Artifact | Source | Command |
|---|---|---|
| 22 frames (`00_login.txt` … `21_home_playlist.txt`) + `manifest.json` | `src/ui/showcase.rs` (capture harness) | `cargo test --release -- --ignored showcase_capture` |
| `docs/index.html` (one file, ~74 KB) | `scripts/build_showcase.py` | `python3 scripts/build_showcase.py` |

The capture harness is a `#[ignore]`d integration test under `src/ui/showcase.rs`
(wired through `src/ui/mod.rs`). It:

1. Loads credentials from `~/.config/freegal/creds.toml` and connects to the
   real API task (same runtime pattern as `src/main.rs`).
2. Signs in, waits for each fetch to quiesce, and presses the same keys a user
   would — so every frame is the genuine renderer output at **100×32**,
   rendered via `TestBackend`.
3. Injects a fake playing bar (track from the featured feed + a synthetic
   `PlayerEvent::Status`) so the site can show playback without mpv.
4. Writes each frame's text plus the renderer's clickable zones
   (`app.click_zones`) into `manifest.json`.

Output lands in `/tmp/opencode/showcase/`.

## Capture sequence

Frames are numbered in capture order (the generator sorts by name):

| Frame | Screen |
|---|---|
| `00_login` | login wizard (not signed in) |
| `01_home` | Home, signed in, sections + sidebar, fake playing bar |
| `02`–`07` | Home section drills: Featured Songs → Trending → Artists → New Albums → Featured Albums → Playlists |
| `08_home_library_playlist` | a library playlist's songs |
| `09_genres` | Genres list |
| `10_genre_pop` | Genre detail (Pop, with subgenre row) |
| `11_mymusic` | My Music root |
| `12_mymusic_wishlist` | My Music drill |
| `13_queue` / `14_history` / `15_downloads` | those tabs |
| `16_help` / `17_audio` | Help overlay / audio-output menu |
| `18_home_featured_album` | track list of the first featured album |
| `19_home_artist_albums` | albums of the first featured artist |
| `20_home_new_album` | track list of the first new album |
| `21_home_playlist` | songs of the first featured playlist (reached from `07`'s rows) |

## Manifest shape

```json
{
  "size": [100, 32],
  "screens": [
    {
      "name": "01_home",
      "title": "…",
      "text": "…80/32 text…",
      "zones": [{ "x": 11, "y": 0, "w": 8, "h": 1, "tag": "TAB:0" }]
    }
  ]
}
```

Zone tags come from the renderer's click zones:
`TAB:i`, `CHIP:i`, `CRUMBH:i`, `CRUMBM:i`, `SEARCHSTRIP`, `HISTFILTER`,
`JUMPSTRIP`, `PROGRESS`, `ROWS:…`. The generator maps tags to target screens in
its `TARGETS` dict (e.g. `CHIP:1` → `03_home_trending`); any `ROW:<screen>` tag
routes straight to that screen by name.

## Interactivity model

- **L1** — a tray of all screens plus `←`/`→` keys; `Esc` returns to Home.
- **Auto-loop** — the page advances one screen every 4 s until the visitor
  clicks or presses a key; the `⏸ auto` / `▶ auto` button in the terminal title
  bar toggles it, and `prefers-reduced-motion` disables it on load.
- **L2** — the baked zone overlays are clickable: the tab bar, chip rack,
  breadcrumbs, search strips and row zones navigate between screens:
  - Home song rows → Featured Songs; sidebar rows → a library playlist.
  - Artist rows → that artist's albums; album rows → that album's tracks;
    playlist rows → its songs; genre rows → the Pop detail.
  - My Music rows → the wishlist drill.
  - Remaining row zones (song rows inside song drills, queue/history/download
    rows, the mixed genre-detail rows) are playback actions in the real app,
    not navigation, so the demo leaves them inert.
- Click-zone overlays are positioned with the **measured** monospace character
  width (not a hard-coded estimate), so they align exactly on any font stack.
- The heading links to the GitHub repo; the star count is fetched client-side
  from `api.github.com` with a baked fallback when the request fails.

## Regeneration workflow

```sh
cargo test --release -- --ignored showcase_capture   # 1. recapture frames
python3 scripts/build_showcase.py                    # 2. rebuild index.html
python3 -m http.server 8123 --directory docs         # 3. preview → localhost:8123
```

then verify in a browser: tray, `←`/`→`/`Esc`, and a few chip/crumb zones
(the build loop uses chrome-devtools tooling for this pass). Commit only when
the page looks right.

## Adding a screen

1. Append a capture step in `src/ui/showcase.rs` (drive keys like a user, pump
   to quiesce, then `capture` with a new name — keep the `NN_` prefix order).
2. Rerun the capture + generator.
3. Row/crumb zones inside the new frame route automatically via the `ROW:<name>`
   convention; tag-only targets (chips, tabs) still go through `TARGETS`.

## Notes

- The API can flake mid-capture (rare); just rerun the capture command.
- Frames contain real listening data for this machine's account by design —
  they are the promotion material. No tokens or credentials are ever written
  into frames or into the page.
- Regenerating refreshes the data; committed frames go stale in looks only.