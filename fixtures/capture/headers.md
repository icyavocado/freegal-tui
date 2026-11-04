# Captured request header sets (api.freegalmusic.com/v1/*)

Verified 2026-11-02 on a patron-library subdomain (demopubliclibrary.freegalmusic.com) (Chromium 155 capture).

## Anonymous GET (library?subdomain=…)
- authorization: Bearer <fresh homepage accessToken>
- referer: https://<sub>.freegalmusic.com/
- origin: https://<sub>.freegalmusic.com
- content-type: application/json
- accept: application/json, text/plain, */*
- (browser fingerprint headers: no extra auth state required)

## Logged-in GET (user/details)
- identical to above, PLUS:
- authenticationtoken: <authenticationToken>
- CRITICAL: authorization must be the SAME bootstrap accessToken that existed at
  login time. A fresh bootstrap + old authenticationtoken → success:false, rc 200,
  "Sorry, you are currently not logged into …".
- All calls include &language=en (site adds it; optional server-side).

## Response envelope
{success, responseCode, responseMessage, href, data}
- rc 9  = library info (success)
- rc 17 = featured songs list (success)
- rc 169 = patron detail (success)
- rc 250/251/252 = artist/song/album search (success)
- rc 200 = "not logged into" FAILURE (success:false)
- CORS: access-control-allow-origin = exact library origin (credentials honored).

## Known list containers
- search song   → data.songs.{freegalUrl,items[],limit,offset,next,previous}
- search artist → data.artists.{...}
- search album  → data.albums.{...}
- library?subdomain → data.library.items[0]  (double wrap)
- user/details  → data.items (single object, NOT array)
