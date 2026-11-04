# freegal — Project Overview

A personal terminal client for Freegal Music: log in with your library card, search the
catalog, stream to mpv with a queue, and spend your weekly download credits deliberately.

## Scope (Core, v1)

In   — login wizard (subdomain → library confirm → card → PIN), search songs/artists/albums,
        artist → album → tracks browsing, mpv streaming with app-side queue + now-playing bar,
        credit-aware downloads with explicit confirm modal, redownload of already-claimed songs.
Out   — wishlist, audiobooks, videos, keymap config overrides, OSC52/kitty extras.

## Stack (pinned)

| Crate        | Ver   | Notes |
|--------------|-------|-------|
| ratatui      | 0.30.2| MSRV 1.88; write from 0.30 docs, not 0.28-era tutorials |
| crossterm    | 0.29  | must unify with ratatui's (`cargo tree -i crossterm`) |
| tui-input    | 0.15  | 0.15.5 aligns with ratatui 0.30.2 + crossterm 0.29 |
| tokio        | 1     | features: rt-multi-thread, macros, net, process, sync, time, io-util |
| reqwest      | 0.12  | rustls-tls (no OpenSSL); connect + total timeouts manually set |
| serde/serde_json | 1 | no deny_unknown_fields anywhere |
| scraper      | 0.x   | bootstrap-token HTML parse (`&quot;` entity handling) |
| tracing      | 0.1   | file subscriber only — stdout is forbidden while TUI runs |
| dirs         | 6     | config/state paths |
| anyhow       | 1     | app boundary only; typed ApiError inside |

Dev: insta (snapshots), tempfile (fake-mpv sockets). rust-toolchain.toml pins 1.88.

## Phases

- **0 — Browser capture** (needs the user's logged-in browser + chrome-devtools tools):
  harvest live request/response pairs as JSON fixtures in `fixtures/`:
  `library?subdomain=`, login, search/artist/album envelopes, `album/streaming-songs`
  → streamUrl, `record-stream-time` payload, `downloads/song` response + credit fields,
  responseCode semantics (46/227/283/316, 61).
- **1 — Skeleton**: git + cargo init, event loop, chrome rendering, graceful teardown,
  fixture tests, CI (fmt/clippy/test).
- **2 — Auth**: state machine (bootstrap vs session), wizard, config 0600, credits in header.
- **3 — Search & browse**: gen-tagged debounce, pagination, typed envelope errors.
- **4 — Player**: mpv IPC, app-side queue, JIT insert-next, now-playing bar.
- **5 — Streaming**: album/streaming-songs → streamUrl → mpv; record-stream-time posting.
- **6 — Downloads**: confirm modal, progress, redownload eligibility.
- **7 — Polish**: scrollbars, README, release build.

Degraded-mode fallback (decided up front): if streaming RE fails, ship browse +
credit-gated download + play-local-MP3s; the UI says so in a one-line strip.

## Guardrails (absolute)

1. `downloads/song` **never** fires implicitly — always behind the confirm modal showing
   remaining credits. No automated test ever calls it.
2. Personal use, own credentials, human pace. The site sets `tdm-reservation: disallowed`,
   `noai`, `noimageai` — no batch loops, sequential requests only. Read-only catalog
   GETs may be served locally from the persistent request cache (see 01-api;
   `~/.local/state/freegal/cache.json`); user/credit/stream
   endpoints always reach the server.
3. The server is always authoritative on credit VALUES; never compute counts or
   remaining quota locally. A STATIC reset-day label ("Resets Mon") is allowed —
   Freegal resets weekly downloads Monday 12:01am CT (public support documentation);
   the label displays no server-derived number and must stay in sync with that
   documented schedule, but no local arithmetic on quota may ever drive UI state.
4. Never log the PIN.