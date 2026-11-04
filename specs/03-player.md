# Player: mpv via IPC

mpv 0.41 spawned as a subprocess with `kill_on_drop(true)`:

```
mpv --no-terminal --idle=yes --no-config --no-video \
    --input-ipc-server=<short unix socket path, ≤ 108 bytes> \
    --force-media-title="<Artist> - <Title>" \
    --http-header-fields="Referer: https://<sub>.freegalmusic.com/",
                         "Origin: https://<sub>.freegalmusic.com/" \
    --user-agent="<browser UA>"
```
stderr → ring buffer / log file. `--no-terminal` mandatory (status line would corrupt TUI;
mpv must never touch the tty or stdin).

## IPC protocol

- Newline-delimited JSON; minify (no `\n` inside messages).
- ONE persistent connection. `observe_property` registrations die on reconnect (mpv #4410)
  — avoid relying on observation; re-register explicitly if ever needed.
- Events and responses interleave on the same socket → every request carries a unique
  `request_id`; the reader task demuxes request_id → response channel, `event` field → events.
- `loadfile` 3rd arg = insertion **index** since mpv 0.38; pass `-1` as 3rd when supplying
  a 4th per-file-options arg. `append-play` is deprecated as of 0.42 → use `append/play` flags.
- Poll `playback-time` / `duration` / `pause` on every UI tick (250 ms). Do NOT observe
  time-pos (event flood). Never extrapolate position locally (no NaN math: duration is Option).
- `playback-restart` event ends the "buffering" state; `end-file` (reason eof/error/quit)
  drives track advance.

## Queue model

**App-side queue is authoritative.** Stream URLs are per-request signed (HLS
manifests with `?token=…`, see fixtures/capture/media-hls.md) and expire while
sitting in mpv's playlist — never pre-enqueue. mpv's playlist stays a 1–2 item buffer:

- `Play now` for a track: `PUT check-streaming-availability {songId, provider}`
  (rc 202) → `data.streamUrl` (signed `manifest.m3u8`) → `loadfile <url> replace`.
  mpv plays HLS natively (ffmpeg demuxer) — no hls.js-equivalent needed.
- Success + rc 46 → toast `responseMessage` verbatim + NO playback (streaming
  blocked/limit for this track).
- On track change (end-file): advance app queue index → same JIT fetch for the new
  track (its own fresh check-availability + record-stream-time for the ended one).
  A manual skip (PlayNow) flushes the ended track's pending streamed seconds
  first — skips still record listening time.
- Stream-ticket reuse: a cached ticket's TTL is fixed from issuance (1800 s); a
  cache hit never refreshes the timestamp, so an expired server URL can't linger
  longer than one ticket lifetime (the EndFile{error} eviction is the backstop).
- `append` / `play-next`: add to app-side Vec; the JIT fetch happens when the track
  is actually coming up.
- Queue rows carry status: `played | playing | queued (JIT) | failed (expired)` —
  failures toast with the API `responseMessage`; user can retry with Enter.

Downloaded local MP3s flow through the same app-side queue mechanism (uniform behavior).

## record-stream-time

POST `{songId, provider, playlistId: null, queueType: 2, eventFired: 13,
userStreamedTime: <DELTA seconds since last post>, token: "WEB_<utc-ts>_<rand>"}`
(bundle-verified format; generateToken = unpadded UTC Y+M0based+D+H+mm+s+ms +
"_" + rand 1..10000). Send on ~10 s playback interval, on pause, and on track-end
(delta batching on the next tick when nothing is playing). Server response carries
authoritative `timeRemaining` (`Seconds | "UNLIMITED"`); display in header, never
computed locally. Streaming-family responseCodes on any streaming call → stop
playback + toast verbatim + error log (rc 46 semantics pinned: success:true + 46).

## Degraded mode

If mpv can't be reached (spawn fail, socket dead): 15 s reconnect backoff, immediate
retry on play attempt, play keys disabled with a toast, browsng/search/downloads unaffected.