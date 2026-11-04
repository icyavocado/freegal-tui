# Freegal API Reference

Base: `https://api.freegalmusic.com/v1/`

## Envelope

Every JSON response: `{success, responseCode, responseMessage, data}`.
Failures frequently arrive as HTTP 200 with `success: false` — never trust the HTTP
status alone. Typed error: `ApiError { http_status, response_code, response_message }`.
Surface `responseMessage` verbatim in the UI; it is what the web app toasts.

| responseCode | Meaning | Client action |
|---|---|---|
| 61  | invalid/Expired authenticationToken | drop session, silent re-login; fail → wizard |
| 401 | bootstrap accessToken expired | re-bootstrap from library domain page, retry once |
| 46/227/283/316 | streaming-related | stop playback, toast message. Semantics from Phase 0 capture |

Serde rules: no `deny_unknown_fields`; `#[serde(default)]` on optional collections;
`timeRemaining`/`streamingTimeRemaining` is an untagged enum `Seconds(u64) | Unlimited`.

## Auth model — two independent tokens

1. **Bootstrap token** (24 h TTL): `GET https://<sub>.freegalmusic.com` returns server-side
   HTML containing `<input type="hidden" id="authToken" name="token" value="{...}">` where
   `{...}` is **HTML-escaped JSON** (`&quot;` etc.) with
   `accessToken / refreshToken / expiresIn(86400) / tokenType`.
   Parse with `scraper` (selector `input#authToken`), HTML-unescape the value.
   Expired → re-fetch the domain page; this does NOT require re-login.
2. **Session**: `POST login {identifier1: cardNumber, identifier2: pin, libraryId}`
   → `data.authenticationToken`. Requires `libraryId` (below).
   Invalid (rc 61) → silent re-login from cached creds; exhausted → wizard.

`libraryId` is **not** in the HTML. Resolve it: `GET library?subdomain=<sub>`
→ library record including `libraryId` + `libraryName`.
Wizard step 2 shows the resolved name; lookup failure → manual `libraryId` modal (S0f).

## Headers — set on EVERY API call

```
Authorization: Bearer <accessToken>          # bootstrap token, even post-login
authenticationtoken: <authenticationToken>   # only after login (lowercase header name)
Origin: https://<sub>.freegalmusic.com
Referer:  https://<sub>.freegalmusic.com/
Content-Type: application/json              # even on GETs
User-Agent: <modern browser UA>
```

Plausibly load-bearing (gateway filtering). Also send on mpv stream requests.
Site also appends `&language=en` to all endpoint calls (harmless either way; we send it).

## Response codes — CAPTURED (live, patron library)

Non-HTTP semantics; per-resource SUCCESS codes:
`9` library, `17` featured songs, `166` artist details, `169` patron details,
`250/251/252` artist/song/album search, `202` check-streaming-availability,
`204` record-stream-time.
**`200` = FAILURE "not logged into the application"** (success:false) — recovery:
re-fetch bootstrap → retry once → only then re-login (see Auth model).
`61` invalid session → re-login. `46` (with success:true) = streaming blocked for
this album → toast `responseMessage` verbatim + abort. `174` = downloaded/songsIds
has ZERO entries → treat as empty list (success shape uncaptured).

## Bootstrap/session pairing — CAPTURED

A *stale* bootstrap Bearer + live session token → rc 200 failure on user/* and
streaming endpoints. Fresh bootstrap + the same session token → rc 202/204 success.
So on rc 200 not-logged-in: **re-bootstrap FIRST, keep the session, retry once**;
re-login only if that still fails. Catalog endpoints (search/artist/albums) work
with any valid bootstrap.

## Endpoints (verified against freegal-api PyPI v0.4.5 + live site Nov 2026)

- `GET user/details` → `data.items` = SINGLE OBJECT: patronId, libraryId,
  availabledownload, downloadLimit, streamingTimeLimit, streamingTimeRemaining
  (string "UNLIMITED" or seconds), library{libraryId, libraryName, domainName,
  libraryScope, libraryStatus, libraryLanguage, libraryCountry, shareUrl}. Proof-of-life.
- `GET search?q=&type=song|artist|album&offset=&limit=&sortOrder=DESC&language=en`
  → `data.songs|artists|albums` = {items[], limit, offset, next, previous} —
  pagination (next/previous ABSOLUTE URLs) lives INSIDE the container.
  Song item: songId, albumId, title, provider, streamStatus, downloadStatus,
  sampleDurationSec, **fullDurationSec**, sortId, embedded
  album{artist{artistId base64, title}, title, genre, label, copyright,
  **copyrightDate (year source — no 'year' field)**, imageUrl}. streamURL may be "".
- `GET artist?artist=<id>` → rc 166, `data.artist.items[0]` (triple wrap).
- `GET artist/albums?artist=&offset=&limit=&sortOrder=DESC&sortBy=sortId` → rc 252,
  data.albums{...container}.
- Album tracks: **NOT songs?albumId** (404s, distinct {error:{code,message}} body).
  Real endpoint (CAPTURED, fixtures/capture/album-songs.json):
  `GET album/songs?albumId=<id>&provider=1&language=en` → rc **17** "List of album's
  songs." → `data.album.items[0]` = ONE album object: artist{artistId,name}, albumId,
  title, provider, explicit, genre, label, copyright, copyrightDate, imageUrl,
  streamStatus + **`songs[]` DIRECT array** (no `{items}` wrapper). Song objects have NO
  streamUrl here — playback URLs come from album/streaming-songs / check-availability.
  Ordering key: `trackNumber`. Year source: `copyrightDate`.
- Discover feeds (all CAPTURED Nov 2026 via logged-in patron probes): GET
  `songs/recommended?offset=0&limit=&provider=1` → rc 17, `data.songs` = Page<Song>
  container (same Song shape as search, stable editorial list);
  `albums/recommended?offset=0&limit=&provider=1` → rc 17, `data.albums` = Page<Album>
  (flat album subset, NO songs[] key — Album's default-Vec tolerates);
  `playlists/recommended?offset=0&limit=&provider=1` → rc 17, `data.playlists` =
  Page<PlaylistCard> (playlistId NUMERIC, songsCount, imageUrl); `genres/recommended`
  (no params) → rc 235, `data.genres` = Page<GenreCard> ~496 items (captured,
  UNUSED — the TUI browses bare `genres` A→Z instead) ({genreId=string
  title, title, image}); drill-downs: `playlist/songs?playlistId=<id>&provider=1` →
  rc 17 Page<Song>; `genre/song?genreName=<title>` (NOTE: genreName, NOT genre) →
  rc 17 Page<Song>. All ride api_headers like the other catalog GETs (envelope
  self-heals rc 61/200); fetched sequentially, never bubbled.
- Home-feed slugs (all CAPTURED Nov 2026 via logged-in probes; one client method
  `home_feed(HomeFeed, limit, fresh)` maps each slug to its container):
  `songs/featured`, `songs/top` (+`sortBy=sortId&sortOrder=DESC` → rc 17
  `data.songs` Page<Song>); `albums/recommended` ("more of what you'd like" —
  an ALBUMS list on the real site), `albums/featured`, `albums/new` →
  `data.albums` Page<Album>; `playlists/recommended`, `playlists/featured`,
  `playlists/library-playlists` (the patron library's curated lists),
  `playlists/top` → `data.playlists` Page<PlaylistCard>;
  `artists/featured` → `data.artists` Page<Artist>;
  `user/songs/recently-played` (user-state; cached 30 min like the rest) →
  `data.songs` Page<Song>; routed through the lenient wrapper (empty codes
  46/247) so a fresh patron's empty list renders empty, not error. Drill-downs: `playlist/songs` and
  `genre/song?genreName=...` stay;   Bare `genres` (A→Z 50) rc 235, `genres/top?rank=true` rc 233 — data parses
  normally; treat those codes as success-alternates.
- My-Music web surfaces (CAPTURED; read-only in the TUI): `user/playlists?offset=0&
  limit=&sortOrder=DESC` → rc 17 `data.playlists` Page<PlaylistCard> or **rc 115**
  (no playlists); `user/downloaded/songs?offset=0&limit=&sortOrder=DESC` → rc 17
  `data.songs` Page<Song> or **rc 46** (empty); `wishlist/songs?contentType=`
  favorite|wishlist`&offset=0&limit=&sortOrder=DESC` → rc 17 `data.songs` Page<Song>
  or **rc 46**; `wishlist/books` → **rc 640** (audiobooks unavailable per library —
  section omitted); `user/recently-stream-playlist` → **rc 247** (none). EMPTY-CODE
  MAP (no message field — the rc is the meaning): 46 no songs · 115 no playlists ·
  174 no downloaded ids · 247 nothing streamed · 640 unavailable. Client
  `cached_envelope_lenient(...)` returns an empty Page for these instead of error.
- Genre detail (CAPTURED for Pop): `subgenres?genreName=&offset=0&limit=&sortOrder=`
  DESC&sortBy=sortId` → rc 17 `data.subGenres[{genreId,title,image}]`;
  `genre/song|genre/album|genre/trending-album|genre/playlist|genre/artist?genreName=`
  &subGenreName=&offset=0&limit=&sortOrder=DESC&sortBy=sortId` → rc 17
  `data.songs|albums|playlists|artists` Pages (Song/Album/PlaylistCard/Artist
  shapes as elsewhere). TUI detail = songs(15)+playlists(8)+albums(8)+artists(12)
  +subgenres(6, top-level only); `genre/trending-album` unused for now. Per-section tolerance: each of
  songs/albums/playlists/artists failing solo → warn + empty Page; the detail
  page is always Ok (matches "never bubbled").
- Streaming: `PUT check-streaming-availability {songId, provider}` → rc 202 →
  `data{timeRemaining, streamUrl, timerCallTime}` — THE per-song play path.
  streamUrl is **HLS**: signed `https://streaming.libraryideas.com/...mp4/manifest.m3u8?token=<32hex>`
  (see fixtures/capture/media-hls.md). mpv plays it natively; still JIT (token in URL).
  `album/streaming-songs` (success → `data.album.items[0].songs[]` w/ per-track
  streamUrl; success+46 → toast + abort). `record-stream-time` POST:
  `{songId, provider, playlistId: null, queueType: 2, eventFired: 13,
  userStreamedTime: <DELTA seconds since last post>, token: generateToken()}` →
  rc 204 → `data.timeRemaining`. generateToken (client-side, bundle-verified):
  `"WEB_" + Y + getUTCMonth() + D + H + mm + s + ms + "_" + rand(1..10000)` — unpadded,
  UTC (e.g. WEB_2026102213826198_5550 = 2026-11-02 21:38:26.198). Post on ~10s interval,
  pause, and track-end. Send to `<name>.part`-style streams? No—that's downloads.
- Downloads: `POST downloads/song {songId, provider: 1}` — **consumes 1 weekly credit**.
  `POST redownload/song` — no credit, for already-claimed tracks.
  Both → `data.downloadUrl` (percent-encoded) → percent-decode → plain GET. The URL is
  **self-authorizing** (no headers needed); stream to `<name>.part` then rename.
  `GET user/downloaded/songs` and `user/downloaded/songsIds` → eligibility list
  (empty patron → rc 174, see above). The ids list is (re)fetched on login and
  session-restore success too, so redownload flags are fresh before the first
  download.

## HTTP client rules

- reqwest defaults to NO timeouts: set connect + total on every client/request.
- Retry with backoff on 5xx/network errors (1×500ms; IMPLEMENTED); exactly one retry after re-bootstrap on 401.
- rustls-tls, no TLS pinning. Sequential calls; no request bursts.
- Read-only catalog GETs (search, artist/albums, album/songs, the six discover
  feeds, playlist/genre drills) are served from a 30-min request cache that
  persists across restarts at `~/.local/state/freegal/cache.json` (0600 via
  mode'd OpenOptions — no 0666 save window; the state dir itself is 0700-enforced;
  atomic rename; cap 64, FIFO eviction; key = path + query pairs, value = raw envelope
  data; failures and null payloads never cached; expired entries dropped on
  load). `R` sends those requests with
  `fresh: true` to bypass the cache; user/session-bound paths (user/details,
  downloaded ids, stream tickets, downloads, record-stream-time) always hit the
  network; login and library switches flush (and re-persist) the cache.

## Fixtures (Phase 0 outputs → `fixtures/capture/*.json`)

CAPTURED: `capture/bootstrap-html.txt` (at the fixtures root, not in capture/),
`library-subdomain.json`, `user-details.json`,
`search-songs.json`, `search-artists.json`, `search-albums.json`, `album-songs.json`,
`check-streaming-availability.json`, `media-hls.md`, `record-stream-time.json`,
`user-downloaded-songsids-empty.json`, `album-streaming-songs-failed-auth.json`,
`headers.md`, `songs-recommended.json`, `albums-recommended.json`,
`playlists-recommended.json`, `genres-recommended.json`, `playlist-songs.json`,
`genre-song.json` (all six from the logged-in patron probe run, Nov 2026).

Still open (need user consent / action):
- `login.json` — POST body/resp/rc (logout → re-login in browser; user re-types card).
- `downloads-song.json` — spends 1 of 5 weekly credits; explicit consent required.
- `downloaded-songsids.json` — SUCCESS shape (current capture is the rc-174 empty case).