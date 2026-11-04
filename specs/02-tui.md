# TUI Design

## Screens

```
Screen::Login(WizardState)        full-screen; 4 steps: subdomain → library confirm → card → PIN
  └ S0f LibraryIdFallback modal   manual libraryId entry if library lookup fails
Screen::Home
  chrome: header(1) / tabs(1) / content(Fill) / hint(1) / bar(2)
  tabs:   1 Home │ 2 My Music │ 3 Queue │ 4 History │ 5 Downloads (search surfaces inside Home)

My Music tab (tab 2, `m` or `2`): web's My-Music composite, feed-backed sections —
  section feeds and drill fetches use separate generators (`section: true` flag on
  feed events routes fills; drills reuse the app nav gen), so a drill opened while
  sections are still streaming never wedges the tab.
  My Playlists (`user/playlists`), Downloaded Songs (`user/downloaded/songs`),
  Favorites (`wishlist/songs?contentType=favorite`), Wishlist
  (`wishlist/songs?contentType=wishlist`). Web-order headings like Home with
  `→` clickable heading rows (Enter = drill), 4 items per section, up/down/g/G
  navigation; Enter on a playlist row jumps to Home and opens the playlist
  drill there; songs play (Enter), queue (a) or download (y). Empty sections
  reproduce the site's own lines ("Your created playlists will appear here." —
  rc 115/46/174/247/640 all map to "empty", never to an error). `R` refetches.

  Home tab (default landing): the search box is a FULL-WIDTH strip above both panes
  (accent border + cursor while focused; `/` or `s` focus it from anywhere; typing shows
  results in place of the feeds; Esc with a query clears it → dashboard); below it, two
  panes when content ≥ 64 cols — left `library playlists` sidebar (←/→ focus
  switches panes; Enter opens the playlist; feed = `playlists/library-playlists`),
  right the CHIP RACK — a full-width row above the panes:
  `[→ Artists] [→ Trending] [→ New] [→ Albums] [→ Playlists] [→ Genres]` —
  Links focus cycles chips with ←/→ (wrap), Enter opens the chip's drill page
  (Genres opens the genre list)
  the web-order sections — Featured Songs, More Of What You'd Like
  (albums/recommended), The Library's Playlists ("{Library} Playlists" on the web), Recently Played Songs,
  Recommended Playlists, Featured Playlists, Artists You Might Like, New Albums,
  Trending Playlists — each capped at 4 visible rows with a clickable `→ title`
  heading row (Enter opens a full drill list/albums/playlists/artists view);
  every drill view (Home and My Music) opens with a BREADCRUMB row 'Home › <Level>'
  (My Music: 'My Music › <Section>'): the last segment is bold, every ancestor
  segment is dim and CLICKABLE (mouse) to jump back to that level; Esc still pops
  exactly one drill level (list/album/album-list/playlist-list/artist → Root).
  The current (bold) segment is NOT clickable; a failed drill fill toasts and
  pops back one level — Loading is never stuck.
  overlays (capture ALL keys, incl. q): DownloadConfirmModal (O1) │ Help (?) │ ErrorLog (e)
T0 too-small card: < 10 rows or < 40 cols → only the card, skip all normal rendering
```

- Esc pops exactly ONE level (Album→Artist→search-root→with a query: clear query→dashboard→noop); never crosses tabs.
- Re-auth is invisible: restores the exact prior screen + selection (`resume` Screen).
- Modal `y` = confirm; `n`, `Esc`, `Enter` = cancel. **Enter NEVER confirms.**

## Layout ladder (pure function + snapshot tests)

```
rows ≥ 20: header(1)+gap(1)+tabs(3, active tab is a pill box)+content+hint(1)+gap(1)+bar(2)
rows ≥ 16: drop hint          rows ≥ 12: fold tabs into header       rows ≥ 10: minimal header
wide ≥ 100 cols: content 60/40 list|detail (Search + Artist contexts only)
```
Chrome lines are borderless Paragraphs; the content pane, the bordered search/filter box (Full-size
panes only) and popups get `Block::bordered()` with a theme-dependent `BorderType` (Rounded
default/tokyo-night, Plain nord). The search/filter box borders light up in accent while focused.
Use `Rect::layout(&Layout::vertical([Length(1), Length(1), Fill(1), Length(1), Length(2)]))`.

## Event architecture

Main thread (sync, sole owner of crossterm):
```
loop { draw();
  if !event::poll(250ms)? { app.tick(); }        // timeout IS the player tick
  while event::poll(ZERO)? { read() → app.update(...) }   // drain, coalesce, render once
}
```
- Keys via `as_key_press_event()`; ignore Release/Repeat. Paste = sanitized first-class
  event (strip control chars/CR/LF, cap ~200 chars). Mouse ok (wheel + click).
- Resize IGNORED — `Terminal::draw` autoresizes.
- Inbound: one `tokio::sync::mpsc::unbounded_channel<AppEvent>` (send infallible from
  async; `try_recv` from sync thread). `app.tick()` issues `PlayerCommand::GetStatus`
  (poll playback-time/duration/paused — do NOT observe time-pos) + stale record-stream-time.
- Outbound `PlayerCommand` / `ApiCommand` channels: bounded cap 16, `try_send`, drop+toast on full.
- Coalesce same-kind status events (keep last). One render per loop iteration.

Tokio on a dedicated std::thread (`Builder::new_multi_thread().enable_all().block_on`):
mpv-writer task · mpv-reader task (request_id demux, reconnect w/ backoff, degraded flag) ·
API task · search-debounce task (300 ms trailing, watch channel, gen-tagged).
Shutdown: drop senders → tasks exit → kill mpv → `restore()` → `runtime.shutdown_background()`.

## Search staleness

`search_gen++` per keystroke → debounce task snapshots `(input, gen)` → single-in-flight
guard → results arrive tagged → UI applies only `result.gen == app.search_gen`.
One request per settled input; race-free by construction.

## Feedback tiers

1. Toasts bottom-right above bar (max 3, 4 s, info/success/error, `responseMessage` verbatim; each renders over a cleared row — never blended into list text).
2. Every error also → ring buffer (100 entries: ts, code, msg) → `e` overlay (windowed to the popup height, newest at bottom; ↑↓/g/G scroll the rest).
3. Persistent state lives in the bar, not in toasts.

Search/list panes render three states in order of precedence: `loading…` /
`Error: <responseMessage> — R to retry` / the list. A non-empty query with every fetch Done
and zero rows renders the centered `No results found for "<query>"` (spelling/fewer-keywords
hint) with a blank status line; empty input renders the Home dashboard instead of the
list — search only exists while a query is armed or the box is focused, all on the merged
Home tab (`R` with an empty query refetches the feeds).

Degraded mode: one-line strip above the bar — `playback unavailable (mpv: <reason>) —
browsing & downloads still work`; play keys disabled with a toast.

## Bar

L1 `▶ PLAYING Title — Artist` + right `queue 3 · Vol 80%`; state as text (icon + word).
L2 progress: elapsed `m:ss / m:ss` pinned LEFT (fixed 12 cols, always visible) + gauge `━━━●───` RIGHT (fills the rest; thumb ●); the click-to-seek zone covers the gauge sub-rect only; `Option<duration>` → indeterminate bar (no NaN panic).
Header: `♪ freegal` left (library name deliberately not shown — screenshots stay private), right-aligned `[Downloads: 2/3 · Resets Mon | Stream: ∞]`
(+ `Stream:` chip only when a streaming budget is known). Header shows step x/4 during the wizard.

## Keymap (static consts; hint line and Help are separate tables)

Focus-scoped dispatch: whatever pane has focus captures printables; globally active keys
are only Enter/Esc/arrows-to-leave/Ctrl-C/Tab. vi keys and arrows coexist; no mode toggle.

| Key | Context | Action |
|---|---|---|
| ↑↓ / j k | lists | move selection |
| Mouse | lists | scroll = ↑↓; click = select row / focus box / pick tab; double-click = open; click the gauge row to seek (zones are rebuilt each frame by the pane renderers) |
| g / G | lists | top / bottom |
| Ctrl-d / Ctrl-u | lists | half page |
| Tab / Shift-Tab / 1-5 / s m q d h | any view | switch tab (`1` = Home, `2` = My Music; the Screen is Home on all tabs once signed in) |
| l / → / Enter | artist or album row | drill in |
| h / Esc | stacked ctx | pop one level |
| Enter | song row | play now; in album/library lists the rest of the list is queued after it |
| a / A | song row | append / play-next |
| Space | not-in-input | play-pause |
| n / p | not-in-input | next / previous |
| , / . | not-in-input | seek ±5 s |
| - / = | not-in-input | volume − / + |
| y | song row | download confirm modal |
| y | modal | confirm (spends credit) |
| n / Esc / Enter | modal | cancel — Enter is deliberately NOT confirm |
| R | errored list | retry |
| e | Home | error log overlay |
| ? | Home | help overlay |
| q | not-in-input | quit |
| Ctrl-q / Ctrl-C | anywhere | quit |
| Enter / Esc / ↑↓ | search input | submit / blur-clear / jump to results |

Input via tui-input 0.15 (emacs standard: C-a/e/u/w/k). PIN masked `'•'`.
Login wizard accepts pasted full URLs (`https://<sub>.freegalmusic.com/`) with
normalization + live preview. Inline per-step errors in red, input never cleared.

## Terminal realities

- `ratatui::try_init()` / `restore()`; NOT `run()`, NOT bare raw-mode plumbing.
- Panic hook chain: `take_hook()` → install `{kill mpv child; orig_hook}` → `try_init()`
  (ratatui's hook restores terminal first, then ours kills mpv).
- `EnableMouseCapture` + `EnableBracketedPaste` together (paste needs both).
- No kitty keyboard-protocol flags in v1. Respect NO_COLOR (use BOLD/DIM too).
- Icons: sparse unicode symbols only (`♪ ▶ ‖ ● ━ ─ ✕ ◌ 🔍`), never in width-budgeted
  lines and never in hint/status lines; titles trusted to unicode-width for truncation.
  The network spinner in the header-right is the one exception: a single braille glyph
  (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, one 120 ms frame per clock tick) shown only while the api task
  is mid-command, then hidden.
- Graceful quit: kill mpv BEFORE `restore()`.

## Theme

Palette roles come from `ui/theme.rs` (single source for accent/muted/border/card/selection/error/success).
The theme boots from `~/.local/state/omarchy/current/theme.name` (trim + case-fold); `nord` maps to
the Nord palette (plain borders, title-in-border at accent), every other name falls back to the built-in
tokyo-night. NO_COLOR ignores the palette entirely — theme affects colors only
  (genre dots keep the ● glyph and drop the hue).

## Tests

1. Pure reducers (`App::update(AppEvent)`, injected `Instant`) — logic.
2. `TestBackend` + insta snapshots at 80×24 / 80×18 / 60×14 / 40×10 (layout ladder rungs),
   including one centered zero-results state (query set, three empty Done pages) and three
   Home tab pins: root two-pane, a drill song list, and the 40×10 floor.
3. Interaction: synthesized KeyEvents.
4. Fake-mpv: tokio UnixListener on a tempfile socket, scripted interleaved JSON, chaos
   (close → reconnect → degraded).
5. Manual matrix: alacritty/foot/kitty/gnome-terminal/tmux/ssh, CJK paste, NO_COLOR,
   quit mid-song, panic leaves clean shell, `kill -9` mpv mid-song.
No PTY e2e in v1. No test ever touches `downloads/song`.