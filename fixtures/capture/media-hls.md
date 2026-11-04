# HLS media chain (live capture, demopubliclibrary)

Site playback is **HLS**, not progressive audio — the earlier "plain `<audio>` with a
direct URL" note was wrong for this library. Evidence (all HTTP 200):

1. `GET https://streaming.libraryideas.com/171/032/821/001/271/600/934/TateMcRae_Housewife_Housewife_1.mp4/manifest.m3u8?token=REDACTED`
2. `GET …/<same>/sc-gaFEAQ/a0_index.m3u8` (media playlist)
3. `GET …/sc-gaFEAQ/a0_F0000.ts`, `a0_F0001.ts`, … segment fetches (~1s chunks)

Browser request headers on the manifest: standard browser UA, `Origin` +
`Referer` = library domain (browser-generated). **Necessity of Origin/Referer for
mpv unverified** — we pass them anyway via `--http-header-fields`.

Player consequence:
- mpv streams the `manifest.m3u8?token=…` URL directly (ffmpeg HLS demuxer).
- Signed token lives in the URL → still NO pre-enqueue; JIT per track.
- `check-streaming-availability` (PUT) returns the per-song streamUrl (rc 202).
- Album flow uses `album/streaming-songs` (rc 46 with success:true = blocked →
  toast + abort; otherwise `data.album.items[0].songs[]` with per-track streamUrl).