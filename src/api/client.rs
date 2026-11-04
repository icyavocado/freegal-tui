use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use serde::Deserialize;
use serde_json::Value;

use super::bootstrap::{self, BootstrapToken};
use super::cache::RequestCache;
use super::creds::{self, StoredCreds};
use super::models::{page_from_container, Album, Artist, Genre, Page, PlaylistCard, Song};
use super::{ApiError, ApiResult, StreamTime, CODE_INVALID_SESSION};

pub const API_BASE: &str = "https://api.freegalmusic.com/v1/";

pub const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:142.0) Gecko/20100101 Firefox/142.0";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Library {
    #[serde(default, rename = "libraryId", alias = "id")]
    pub id: i64,
    #[serde(default, rename = "libraryName", alias = "name")]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct UserDetails {
    pub library: Option<Library>,
    #[serde(rename = "patronId")]
    pub patron_id: Option<String>,
    #[serde(rename = "downloadLimit")]
    pub download_limit: Option<i64>,
    #[serde(rename = "availabledownload", alias = "availableDownload")]
    pub available_download: Option<i64>,
    #[serde(rename = "streamingTimeLimit")]
    pub streaming_time_limit: Option<StreamTime>,
    #[serde(rename = "streamingTimeRemaining")]
    pub streaming_time_remaining: Option<StreamTime>,
}

pub struct ApiClient {
    http: reqwest::Client,
    subdomain: String,
    creds: StoredCreds,
    creds_path: Option<PathBuf>,
    cache: RequestCache,
    cache_path: Option<PathBuf>,
}

enum Method<'a> {
    Get(&'a str),
    Post(&'a str),
    Put(&'a str),
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn sanitize_subdomain(input: &str) -> Option<String> {
    let cleaned = input.trim().trim_end_matches('/');
    let host = cleaned
        .strip_prefix("https://")
        .or_else(|| cleaned.strip_prefix("http://"))
        .unwrap_or(cleaned)
        .split('/')
        .next()?;
    let sub = if host.contains('.') {
        host.split('.').next()?.to_string()
    } else {
        host.to_string()
    };
    let sub = sub.trim().to_lowercase();
    if sub.is_empty()
        || sub == "www"
        || !sub
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(sub)
}

impl ApiClient {
    pub fn new(
        subdomain: impl Into<String>,
        creds: StoredCreds,
        creds_path: Option<PathBuf>,
    ) -> ApiResult<Self> {
        let http = reqwest::Client::builder()
            .user_agent(BROWSER_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ApiError::Network(format!("http client: {e}")))?;
        let subdomain = subdomain.into();
        let cache_path = RequestCache::cache_path();
        let cache = match &cache_path {
            Some(path) => RequestCache::load(path, super::cache::now_epoch()),
            None => RequestCache::default(),
        };
        Ok(Self {
            http,
            subdomain: sanitize_subdomain(&subdomain).unwrap_or(subdomain),
            creds,
            creds_path,
            cache,
            cache_path,
        })
    }

    #[allow(dead_code)]
    pub fn subdomain(&self) -> &str {
        &self.subdomain
    }

    #[allow(dead_code)]
    pub fn creds(&self) -> &StoredCreds {
        &self.creds
    }

    pub fn has_cached_login(&self) -> bool {
        self.creds.has_card_and_pin() && self.creds.library_id.is_some()
    }

    fn persist(&self) {
        if let Some(path) = &self.creds_path {
            let mut creds = self.creds.clone();
            // The subdomain only reaches disk through creds.subdomain; the login
            // wizard sets it on the client, never on the struct stored here. If
            // it is left empty on disk, App::new refuses to restore and every
            // relaunch strands the user in the login wizard with a silent
            // player (mpv gets empty Referer/Origin).
            if creds.subdomain.is_empty() {
                creds.subdomain = self.subdomain.clone();
            }
            if let Err(e) = creds::save(path, &creds) {
                tracing::error!("persisting creds failed: {e:#}");
            }
        }
    }

    fn library_domain(&self) -> String {
        format!("https://{}.freegalmusic.com", self.subdomain)
    }

    fn bootstrap_expired(&self) -> bool {
        match (
            self.creds.bootstrap_access_token.as_ref(),
            self.creds.bootstrap_expires_at_unix,
        ) {
            (Some(_), Some(exp)) => unix_now() + 300 >= exp,
            _ => true,
        }
    }

    pub async fn ensure_bootstrap(&mut self) -> ApiResult<()> {
        if !self.bootstrap_expired() {
            return Ok(());
        }
        self.invalidate_bootstrap();
        let resp = self
            .http
            .get(self.library_domain())
            .send()
            .await
            .map_err(|e| ApiError::Network(format!("library page: {e}")))?;
        let html = resp
            .text()
            .await
            .map_err(|e| ApiError::Network(format!("library page body: {e}")))?;
        if html.trim().starts_with('<') && !html.contains("authToken") {
            return Err(ApiError::Api {
                code: None,
                message: "library page did not provide a bootstrap token".into(),
            });
        }
        let token: BootstrapToken =
            bootstrap::parse_bootstrap_html(&html).map_err(|e| ApiError::Api {
                code: None,
                message: format!("{e:#}"),
            })?;
        self.creds.bootstrap_access_token = Some(token.access_token);
        self.creds.refresh_token = token.refresh_token;
        self.creds.bootstrap_expires_at_unix = Some(unix_now() + token.expires_in as i64);
        self.persist();
        Ok(())
    }

    pub fn invalidate_bootstrap(&mut self) {
        self.creds.bootstrap_access_token = None;
        self.creds.bootstrap_expires_at_unix = None;
    }

    pub fn has_session(&self) -> bool {
        self.creds.authentication_token.is_some() && self.creds.library_id.is_some()
    }

    pub async fn ensure_session(&mut self) -> ApiResult<()> {
        if self.has_session() {
            return Ok(());
        }
        if !self.has_cached_login() {
            return Err(ApiError::Api {
                code: None,
                message: "no stored credentials — login required".into(),
            });
        }
        self.login().await
    }

    pub async fn login_with(&mut self, card: String, pin: String) -> ApiResult<()> {
        self.creds.card_number = card;
        self.creds.pin = pin;
        self.flush_cache();
        self.login().await
    }

    fn flush_cache(&mut self) {
        self.cache.clear();
        if let Some(path) = &self.cache_path {
            self.cache.save(path);
        }
    }

    pub fn set_library_id(&mut self, id: i64) {
        self.creds.library_id = Some(id);
        self.flush_cache();
        self.persist();
    }

    pub async fn login(&mut self) -> ApiResult<()> {
        self.ensure_bootstrap().await?;
        let Some(library_id) = self.creds.library_id else {
            return Err(ApiError::Api {
                code: None,
                message: "library not resolved — run the library lookup first".into(),
            });
        };
        let body = serde_json::json!({
            "identifier1": self.creds.card_number,
            "identifier2": self.creds.pin,
            "libraryId": library_id,
        });
        let data = self
            .fetch_json(Method::Post("login"), &[], Some(body))
            .await?;
        let token = data
            .get("authenticationToken")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::Api {
                code: None,
                message: "login response missing authenticationToken".into(),
            })?
            .to_owned();
        self.creds.authentication_token = Some(token);
        self.persist();
        Ok(())
    }

    pub async fn resolve_library(&mut self, subdomain: &str) -> ApiResult<Library> {
        self.subdomain = sanitize_subdomain(subdomain).ok_or_else(|| ApiError::Api {
            code: None,
            message: "cannot derive subdomain — enter e.g. demo or a full URL".into(),
        })?;
        self.ensure_bootstrap().await?;
        let sub = self.subdomain.clone();
        let data = self
            .fetch_json(Method::Get("library"), &[("subdomain", sub.as_str())], None)
            .await?;
        let record = single_list(&data, "library")?;
        let library: Library = serde_json::from_value(record)
            .context("library record shape")
            .map_err(|e| ApiError::Api {
                code: None,
                message: format!("{e:#}"),
            })?;
        self.creds.library_id = Some(library.id);
        self.creds.library_name = Some(library.name.clone());
        self.flush_cache();
        self.persist();
        Ok(library)
    }

    pub async fn user_details(&mut self) -> ApiResult<UserDetails> {
        self.ensure_session().await?;
        let data = self
            .fetch_json(Method::Get("user/details"), &[], None)
            .await?;
        let details: UserDetails = serde_json::from_value(single_object(&data, "items"))
            .context("user details shape")
            .map_err(|e| ApiError::Api {
                code: None,
                message: format!("{e:#}"),
            })?;
        Ok(details)
    }

    pub async fn search(
        &mut self,
        query: &str,
        kind: SearchKind,
        offset: u64,
        limit: u64,
        fresh: bool,
    ) -> ApiResult<ContainerKind> {
        let data = self
            .cached_json(
                "search",
                fresh,
                &[
                    ("q", query),
                    ("type", kind.as_str()),
                    ("offset", &offset.to_string()),
                    ("limit", &limit.to_string()),
                    ("sortOrder", "DESC"),
                ],
            )
            .await?;
        container_kind(kind, &data)
    }

    pub async fn artist_albums(
        &mut self,
        artist_id: &str,
        offset: u64,
        limit: u64,
    ) -> ApiResult<Page<Album>> {
        let data = self
            .cached_json(
                "artist/albums",
                false,
                &[
                    ("artist", artist_id),
                    ("offset", &offset.to_string()),
                    ("limit", &limit.to_string()),
                    ("sortOrder", "DESC"),
                    ("sortBy", "sortId"),
                ],
            )
            .await?;
        let container = data.get("albums").cloned().ok_or_else(|| ApiError::Api {
            code: None,
            message: "artist/albums response missing 'albums' container".into(),
        })?;
        page_from_container(&container).map_err(err)
    }

    pub async fn home_feed(
        &mut self,
        feed: crate::home::HomeFeed,
        limit: u64,
        fresh: bool,
    ) -> ApiResult<crate::home::HomeFeedData> {
        use crate::home::{HomeFeed, HomeFeedData};
        let (path, container) = match feed {
            HomeFeed::FeaturedSongs => ("songs/featured", "songs"),
            HomeFeed::TopSongs => ("songs/top", "songs"),
            HomeFeed::MoreForYou => ("albums/recommended", "albums"),
            HomeFeed::FeaturedAlbums => ("albums/featured", "albums"),
            HomeFeed::NewAlbums => ("albums/new", "albums"),
            HomeFeed::RecommendedPlaylists => ("playlists/recommended", "playlists"),
            HomeFeed::FeaturedPlaylists => ("playlists/featured", "playlists"),
            HomeFeed::LibraryPlaylists => ("playlists/library-playlists", "playlists"),
            HomeFeed::TrendingPlaylists => ("playlists/top", "playlists"),
            HomeFeed::FeaturedArtists => ("artists/featured", "artists"),
            HomeFeed::RecentlyPlayed => ("user/songs/recently-played", "songs"),
        };
        let limit_s = limit.to_string();
        let pairs: Vec<(&str, &str)> = if feed == HomeFeed::RecentlyPlayed {
            vec![("offset", "0"), ("limit", &limit_s)]
        } else if feed == HomeFeed::TopSongs || feed == HomeFeed::TrendingPlaylists {
            vec![
                ("offset", "0"),
                ("limit", &limit_s),
                ("sortBy", "sortId"),
                ("sortOrder", "DESC"),
                ("provider", "1"),
            ]
        } else {
            vec![("offset", "0"), ("limit", &limit_s), ("provider", "1")]
        };
        let data: Option<Value> = if feed == HomeFeed::RecentlyPlayed {
            self.cached_envelope_lenient(path, fresh, &pairs, &[46, 247])
                .await?
                .data
        } else {
            Some(self.cached_json(path, fresh, &pairs).await?)
        };
        let container_value = data
            .as_ref()
            .and_then(|d| d.get(container))
            .cloned()
            .ok_or_else(|| ApiError::Api {
                code: None,
                message: format!("{path} response missing '{container}' container"),
            })?;
        let parsed = match container {
            "songs" => {
                HomeFeedData::Songs(page_from_container::<Song>(&container_value).map_err(err)?)
            }
            "albums" => {
                HomeFeedData::Albums(page_from_container::<Album>(&container_value).map_err(err)?)
            }
            "playlists" => HomeFeedData::Playlists(
                page_from_container::<PlaylistCard>(&container_value).map_err(err)?,
            ),
            _ => {
                HomeFeedData::Artists(page_from_container::<Artist>(&container_value).map_err(err)?)
            }
        };
        Ok(parsed)
    }

    async fn cached_envelope_lenient(
        &mut self,
        path: &str,
        fresh: bool,
        query: &[(&str, &str)],
        empty_codes: &[i64],
    ) -> ApiResult<super::Envelope<Value>> {
        match self.cached_json(path, fresh, query).await {
            Ok(data) => Ok(super::Envelope {
                success: true,
                response_code: None,
                response_message: None,
                data: Some(data),
            }),
            Err(ApiError::Api {
                code: Some(c),
                message,
                ..
            }) if empty_codes.contains(&c) => Ok(super::Envelope {
                success: false,
                response_code: Some(c),
                response_message: Some(message),
                data: None,
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn genres(&mut self, fresh: bool) -> ApiResult<Page<Genre>> {
        let env = self
            .cached_envelope_lenient("genres", fresh, &[], &[233, 235])
            .await?;
        parsed_page::<Genre>(&env.data, "genres")
    }

    pub async fn subgenres(&mut self, genre: &str) -> ApiResult<Page<Genre>> {
        let env = self
            .cached_envelope_lenient(
                "subgenres",
                false,
                &[
                    ("genreName", genre),
                    ("offset", "0"),
                    ("limit", "6"),
                    ("sortOrder", "DESC"),
                    ("sortBy", "sortId"),
                ],
                &[],
            )
            .await?;
        parsed_page::<Genre>(&env.data, "subGenres")
    }

    pub async fn genre_songs(
        &mut self,
        genre: &str,
        sub: Option<&str>,
        limit: u64,
    ) -> ApiResult<Page<Song>> {
        let limit_s = limit.to_string();
        let sub_s = sub.unwrap_or("");
        let pairs: Vec<(&str, &str)> = vec![
            ("offset", "0"),
            ("limit", &limit_s),
            ("sortOrder", "DESC"),
            ("sortBy", "sortId"),
            ("genreName", genre),
            ("subGenreName", sub_s),
        ];
        let env = self
            .cached_envelope_lenient("genre/song", false, &pairs, &[])
            .await?;
        parsed_page::<Song>(&env.data, "songs")
    }

    pub async fn genre_albums(
        &mut self,
        genre: &str,
        sub: Option<&str>,
        limit: u64,
    ) -> ApiResult<Page<Album>> {
        let limit_s = limit.to_string();
        let sub_s = sub.unwrap_or("");
        let pairs: Vec<(&str, &str)> = vec![
            ("offset", "0"),
            ("limit", &limit_s),
            ("sortOrder", "DESC"),
            ("sortBy", "sortId"),
            ("genreName", genre),
            ("subGenreName", sub_s),
        ];
        let env = self
            .cached_envelope_lenient("genre/album", false, &pairs, &[])
            .await?;
        parsed_page::<Album>(&env.data, "albums")
    }

    pub async fn genre_playlists(
        &mut self,
        genre: &str,
        sub: Option<&str>,
        limit: u64,
    ) -> ApiResult<Page<PlaylistCard>> {
        let limit_s = limit.to_string();
        let sub_s = sub.unwrap_or("");
        let pairs: Vec<(&str, &str)> = vec![
            ("offset", "0"),
            ("limit", &limit_s),
            ("sortOrder", "DESC"),
            ("sortBy", "sortId"),
            ("genreName", genre),
            ("subGenreName", sub_s),
        ];
        let env = self
            .cached_envelope_lenient("genre/playlist", false, &pairs, &[])
            .await?;
        parsed_page::<PlaylistCard>(&env.data, "playlists")
    }

    pub async fn genre_artists(
        &mut self,
        genre: &str,
        sub: Option<&str>,
        limit: u64,
    ) -> ApiResult<Page<Artist>> {
        let limit_s = limit.to_string();
        let sub_s = sub.unwrap_or("");
        let pairs: Vec<(&str, &str)> = vec![
            ("offset", "0"),
            ("limit", &limit_s),
            ("sortOrder", "DESC"),
            ("sortBy", "sortId"),
            ("genreName", genre),
            ("subGenreName", sub_s),
        ];
        let env = self
            .cached_envelope_lenient("genre/artist", false, &pairs, &[])
            .await?;
        parsed_page::<Artist>(&env.data, "artists")
    }

    pub async fn user_playlists(
        &mut self,
        limit: u64,
        fresh: bool,
    ) -> ApiResult<Page<PlaylistCard>> {
        let limit_s = limit.to_string();
        let env = self
            .cached_envelope_lenient(
                "user/playlists",
                fresh,
                &[("offset", "0"), ("limit", &limit_s), ("sortOrder", "DESC")],
                &[115],
            )
            .await?;
        parsed_page::<PlaylistCard>(&env.data, "playlists")
    }

    pub async fn wishlist_songs(
        &mut self,
        kind: &str,
        limit: u64,
        fresh: bool,
    ) -> ApiResult<Page<Song>> {
        let limit_s = limit.to_string();
        let env = self
            .cached_envelope_lenient(
                "wishlist/songs",
                fresh,
                &[
                    ("contentType", kind),
                    ("offset", "0"),
                    ("limit", &limit_s),
                    ("sortOrder", "DESC"),
                ],
                &[46],
            )
            .await?;
        parsed_page::<Song>(&env.data, "songs")
    }

    pub async fn downloaded_songs(&mut self, limit: u64, fresh: bool) -> ApiResult<Page<Song>> {
        let limit_s = limit.to_string();
        let env = self
            .cached_envelope_lenient(
                "user/downloaded/songs",
                fresh,
                &[("offset", "0"), ("limit", &limit_s), ("sortOrder", "DESC")],
                &[46],
            )
            .await?;
        parsed_page::<Song>(&env.data, "songs")
    }

    pub async fn playlist_songs(&mut self, playlist_id: u64) -> ApiResult<Page<Song>> {
        let data = self
            .cached_json(
                "playlist/songs",
                false,
                &[
                    ("playlistId", &playlist_id.to_string()),
                    ("provider", "1"),
                    ("offset", "0"),
                    ("limit", "50"),
                ],
            )
            .await?;
        page_from_container(&data.get("songs").cloned().ok_or_else(|| ApiError::Api {
            code: None,
            message: "playlist/songs response missing 'songs' container".into(),
        })?)
        .map_err(err)
    }

    #[allow(dead_code)]
    pub async fn page_get(
        &mut self,
        path: &str,
        pairs: &[(String, String)],
    ) -> ApiResult<ContainerKind> {
        let query: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let data = self.fetch_json(Method::Get(path), &query, None).await?;
        let kind = pairs
            .iter()
            .find(|(k, _)| k == "type")
            .and_then(|(_, v)| match v.as_str() {
                "song" => Some(SearchKind::Song),
                "artist" => Some(SearchKind::Artist),
                "album" => Some(SearchKind::Album),
                _ => None,
            })
            .ok_or_else(|| ApiError::Api {
                code: None,
                message: "pagination URL missing type param".into(),
            })?;
        container_kind(kind, &data)
    }

    pub async fn album_songs(&mut self, album: &Album) -> ApiResult<Album> {
        let album_id = album.id.to_string();
        let provider = album.provider.unwrap_or(1).to_string();
        let data = self
            .cached_json(
                "album/songs",
                false,
                &[
                    ("albumId", album_id.as_str()),
                    ("provider", provider.as_str()),
                ],
            )
            .await?;
        let record = single_list(&data, "album")?;
        let mut detail: Album = serde_json::from_value(record)
            .context("album record shape")
            .map_err(|e| ApiError::Api {
                code: None,
                message: format!("{e:#}"),
            })?;
        if detail.image_url.is_none() {
            detail.image_url = album.image_url.clone();
        }
        if detail.year.is_none() {
            detail.year = album.year;
        }
        if detail.artist.is_none() {
            detail.artist = album.artist.clone();
        }
        Ok(detail)
    }

    async fn cached_json(
        &mut self,
        path: &str,
        fresh: bool,
        query: &[(&str, &str)],
    ) -> ApiResult<Value> {
        let now = super::cache::now_epoch();
        if !fresh {
            if let Some(cached) = self.cache.get(path, query, now) {
                return Ok(cached);
            }
        }
        let data = self.fetch_json(Method::Get(path), query, None).await?;
        self.cache.put(path, query, data.clone(), now);
        if let Some(path) = &self.cache_path {
            self.cache.save(path);
        }
        Ok(data)
    }

    fn api_headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{
            HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, ORIGIN, REFERER,
        };
        let mut headers = HeaderMap::new();
        if let Some(token) = self.creds.bootstrap_access_token.as_ref() {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert(AUTHORIZATION, v);
            }
        }
        if let Some(session) = self.creds.authentication_token.as_ref() {
            if let Ok(v) = HeaderValue::from_str(session) {
                headers.insert("authenticationtoken", v);
            }
        }
        let domain = self.library_domain();
        if let Ok(v) = HeaderValue::from_str(&domain) {
            headers.insert(ORIGIN, v);
            if let Ok(r) = HeaderValue::from_str(&format!("{domain}/")) {
                headers.insert(REFERER, r);
            }
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    async fn fetch_json(
        &mut self,
        method: Method<'_>,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> ApiResult<Value> {
        let env = self.fetch_envelope(method, query, body).await?;
        Ok(env.data.unwrap_or(Value::Null))
    }

    async fn fetch_envelope(
        &mut self,
        method: Method<'_>,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> ApiResult<super::Envelope<Value>> {
        let mut retried_network = false;
        let mut retried_rebootstrap = false;
        let mut retried_relogin = false;
        loop {
            self.ensure_bootstrap().await?;
            let url = match method {
                Method::Get(p) | Method::Post(p) | Method::Put(p) => format!("{API_BASE}{p}"),
            };
            let mut req = match method {
                Method::Get(_) => self.http.get(&url),
                Method::Post(_) => self.http.post(&url),
                Method::Put(_) => self.http.put(&url),
            };
            let mut full_query = query.to_vec();
            if !full_query.iter().any(|(k, _)| *k == "language") {
                full_query.push(("language", "en"));
            }
            req = req.headers(self.api_headers()).query(&full_query);
            if let Some(b) = body.as_ref() {
                req = req.json(b);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    if !retried_network {
                        tracing::warn!("network error {e}; retrying after backoff");
                        retried_network = true;
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                    return Err(ApiError::Network(e.to_string()));
                }
            };
            let status = resp.status();
            if status.is_server_error() && !retried_network {
                tracing::warn!("HTTP {status} — retrying after backoff");
                retried_network = true;
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            if status.as_u16() == 401 && !retried_rebootstrap {
                tracing::warn!("401 — re-bootstrap and retry once");
                retried_rebootstrap = true;
                self.invalidate_bootstrap();
                continue;
            }
            if status.as_u16() == 401 {
                return Err(ApiError::Api {
                    code: Some(401),
                    message: "authentication rejected (401)".into(),
                });
            }
            let text = resp
                .text()
                .await
                .map_err(|e| ApiError::Network(format!("response body: {e}")))?;
            let env: super::Envelope<Value> = match serde_json::from_str(&text) {
                Ok(env) => env,
                Err(_) => {
                    let snippet: String = text.chars().take(120).collect();
                    return Err(ApiError::Api {
                        code: Some(status.as_u16() as i64),
                        message: format!("non-JSON response (HTTP {status}): {snippet}"),
                    });
                }
            };
            if !env.success {
                let not_authed = env.response_code == Some(200)
                    && env
                        .response_message
                        .as_deref()
                        .is_some_and(|m| m.to_lowercase().contains("not logged in"));
                if not_authed && !retried_rebootstrap {
                    tracing::warn!("rc 200 (pairing) — re-bootstrap, keep session, retry");
                    retried_rebootstrap = true;
                    self.invalidate_bootstrap();
                    continue;
                }
                if not_authed && !retried_relogin {
                    tracing::warn!("rc 200 persists — silent re-login and retry");
                    retried_relogin = true;
                    self.creds.authentication_token = None;
                    Box::pin(self.login()).await?;
                    continue;
                }
                if env.response_code == Some(CODE_INVALID_SESSION) && !retried_relogin {
                    tracing::warn!("rc 61 (invalid session) — silent re-login");
                    retried_relogin = true;
                    self.creds.authentication_token = None;
                    Box::pin(self.login()).await?;
                    continue;
                }
                return Err(ApiError::Api {
                    code: env.response_code,
                    message: env
                        .response_message
                        .unwrap_or_else(|| "request failed".into()),
                });
            }
            return Ok(env);
        }
    }

    pub async fn check_streaming_availability(
        &mut self,
        song_id: u64,
        provider: u8,
    ) -> ApiResult<StreamTicket> {
        self.ensure_session().await?;
        let body = serde_json::json!({ "songId": song_id, "provider": provider });
        let env = self
            .fetch_envelope(Method::Put("check-streaming-availability"), &[], Some(body))
            .await?;
        let rc = env.response_code;
        let message = env.message().to_string();
        let data = env.data.unwrap_or(Value::Null);
        if rc == Some(46) {
            return Err(ApiError::Api { code: rc, message });
        }
        let stream_url = data
            .get("streamUrl")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(ApiError::Api { code: rc, message })?
            .to_string();
        let time_remaining = data
            .get("timeRemaining")
            .cloned()
            .and_then(|v| serde_json::from_value::<StreamTime>(v).ok());
        Ok(StreamTicket {
            stream_url,
            time_remaining,
        })
    }

    pub async fn record_stream_time(
        &mut self,
        song_id: u64,
        provider: u8,
        user_streamed_secs: f64,
    ) -> ApiResult<Option<StreamTime>> {
        self.ensure_session().await?;
        let body = serde_json::json!({
            "songId": song_id,
            "provider": provider,
            "playlistId": Value::Null,
            "queueType": 2,
            "eventFired": 13,
            "userStreamedTime": user_streamed_secs,
            "token": web_token(),
        });
        let env = self
            .fetch_envelope(Method::Post("record-stream-time"), &[], Some(body))
            .await?;
        let remaining = env
            .data
            .and_then(|d| d.get("timeRemaining").cloned())
            .and_then(|v| serde_json::from_value::<StreamTime>(v).ok());
        Ok(remaining)
    }

    pub async fn downloaded_song_ids(&mut self) -> ApiResult<Vec<u64>> {
        self.ensure_session().await?;
        match self
            .fetch_envelope(Method::Get("user/downloaded/songsIds"), &[], None)
            .await
        {
            Ok(env) => Ok(extract_song_ids(env.data.as_ref().unwrap_or(&Value::Null))),
            Err(ApiError::Api {
                code: Some(174), ..
            }) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    pub async fn download_song(&mut self, song_id: u64, provider: u8) -> ApiResult<String> {
        self.ensure_session().await?;
        self.claim_download(Method::Post("downloads/song"), song_id, provider)
            .await
    }

    pub async fn redownload_song(&mut self, song_id: u64, provider: u8) -> ApiResult<String> {
        self.ensure_session().await?;
        self.claim_download(Method::Post("redownload/song"), song_id, provider)
            .await
    }

    async fn claim_download(
        &mut self,
        method: Method<'_>,
        song_id: u64,
        provider: u8,
    ) -> ApiResult<String> {
        let body = serde_json::json!({ "songId": song_id, "provider": provider });
        let env = self.fetch_envelope(method, &[], Some(body)).await?;
        let raw = env
            .data
            .as_ref()
            .and_then(|d| d.get("downloadUrl"))
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::Api {
                code: env.response_code,
                message: env
                    .response_message
                    .unwrap_or_else(|| "no downloadUrl".into()),
            })?;
        let url = super::download::percent_decode(raw);
        if url.is_empty() {
            return Err(ApiError::Api {
                code: env.response_code,
                message: "empty downloadUrl".into(),
            });
        }
        Ok(url)
    }

    pub async fn download_to_file(&self, url: &str, dest: PathBuf) -> std::io::Result<PathBuf> {
        super::download::download_to_file(&self.http, url, dest).await
    }
}

fn extract_song_ids(data: &Value) -> Vec<u64> {
    fn push_from(value: &Value, out: &mut Vec<u64>) {
        match value {
            Value::Array(items) => items.iter().for_each(|v| push_from(v, out)),
            Value::Object(map) => {
                let id = map
                    .get("songId")
                    .or_else(|| map.get("id"))
                    .and_then(Value::as_u64);
                if let Some(id) = id {
                    out.push(id);
                }
            }
            Value::Number(n) => {
                if let Some(id) = n.as_u64() {
                    out.push(id);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    push_from(data, &mut out);
    out
}

const CIVIL_EPOCH_OFFSET: i64 = 719_468;

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + CIVIL_EPOCH_OFFSET;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, (m - 1) as u32, d as u32)
}

pub fn web_token() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let (year, month0, day) = civil_from_days((secs / 86_400) as i64);
    let sod = secs % 86_400;
    let (h, mi, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    let rnd = now.subsec_nanos() % 10_000 + 1;
    format!("WEB_{year}{month0}{day}{h}{mi}{s}{millis}_{rnd}")
}

#[allow(dead_code)]
fn unwrap_items(data: &Value) -> Value {
    data.get("items")
        .or_else(|| data.get("list"))
        .cloned()
        .unwrap_or_else(|| data.clone())
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StreamTicket {
    pub stream_url: String,
    pub time_remaining: Option<StreamTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    Song,
    Artist,
    Album,
}

impl SearchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Artist => "artist",
            Self::Album => "album",
        }
    }

    fn container_key(self) -> &'static str {
        match self {
            Self::Song => "songs",
            Self::Artist => "artists",
            Self::Album => "albums",
        }
    }
}

fn container_kind(kind: SearchKind, data: &Value) -> ApiResult<ContainerKind> {
    let key = kind.container_key();
    let container = data.get(key).cloned().ok_or_else(|| ApiError::Api {
        code: None,
        message: format!("search response missing '{key}' container"),
    })?;
    match kind {
        SearchKind::Song => Ok(ContainerKind::Songs(
            page_from_container(&container).map_err(err)?,
        )),
        SearchKind::Artist => Ok(ContainerKind::Artists(
            page_from_container(&container).map_err(err)?,
        )),
        SearchKind::Album => Ok(ContainerKind::Albums(
            page_from_container(&container).map_err(err)?,
        )),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerKind {
    Songs(Page<Song>),
    Artists(Page<Artist>),
    Albums(Page<Album>),
}

fn single_list(data: &Value, container: &str) -> ApiResult<Value> {
    let wrapped = data.get(container).ok_or_else(|| ApiError::Api {
        code: None,
        message: format!("response missing '{container}' record"),
    })?;
    let items = match wrapped.get("items") {
        Some(Value::Array(items)) => items,
        Some(other) => return Ok(other.clone()),
        None => return Ok(wrapped.clone()),
    };
    items.first().cloned().ok_or_else(|| ApiError::Api {
        code: None,
        message: format!("'{container}' record is empty"),
    })
}

fn single_object(data: &Value, container: &str) -> Value {
    data.get(container).cloned().unwrap_or(Value::Null)
}

fn err(e: String) -> ApiError {
    ApiError::Api {
        code: None,
        message: format!("response shape: {e}"),
    }
}

fn parsed_page<T>(data: &Option<Value>, container: &str) -> ApiResult<Page<T>>
where
    T: for<'de> serde::de::Deserialize<'de> + Default,
{
    let Some(data) = data else {
        return Ok(Page::default());
    };
    match data.get(container) {
        Some(value) => page_from_container::<T>(value).map_err(err),
        None => Ok(Page::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_token_shape() {
        let token = web_token();
        let parts: Vec<&str> = token.split('_').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "WEB");
        assert!(parts[1].chars().all(|c| c.is_ascii_digit()), "{token}");
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()), "{token}");
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(20_759), (2026, 10, 2));
    }

    #[test]
    fn subdomain_normalization() {
        assert_eq!(sanitize_subdomain("demo").as_deref(), Some("demo"));
        assert_eq!(
            sanitize_subdomain("https://demo.freegalmusic.com/").as_deref(),
            Some("demo")
        );
        assert_eq!(
            sanitize_subdomain("http://demo.freegalmusic.com").as_deref(),
            Some("demo")
        );
        assert_eq!(
            sanitize_subdomain("DEMO.FREEGALMUSIC.COM").as_deref(),
            Some("demo")
        );
        assert_eq!(
            sanitize_subdomain("https://demo.freegalmusic.com/freegal-music").as_deref(),
            Some("demo")
        );
        assert!(sanitize_subdomain("").is_none());
        assert!(sanitize_subdomain("   ").is_none());
        assert!(
            sanitize_subdomain("https://www.freegalmusic.com/").is_none(),
            "www is not a library subdomain"
        );
    }

    #[test]
    fn persist_syncs_subdomain_from_client() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("creds.toml");
        let mut client = ApiClient::new(
            "testlib",
            StoredCreds {
                subdomain: String::new(),
                ..StoredCreds::default()
            },
            Some(path.clone()),
        )
        .unwrap();
        client.set_library_id(42);
        let creds = creds::load(&path).unwrap().expect("creds saved");
        assert_eq!(creds.subdomain, "testlib");
        assert_eq!(creds.library_id, Some(42));
    }

    #[test]
    fn library_record_double_wrap_fixture() {
        let data: Value = serde_json::json!({
            "library": {"items": [
                {"libraryId": 1320, "name": "Demo Public Library",
                 "domainName": "demopubliclibrary", "status": "active"}
            ]}
        });
        let record = single_list(&data, "library").expect("library record");
        let library: Library = serde_json::from_value(record).expect("Library shape");
        assert_eq!(library.id, 1320);
        assert_eq!(library.name, "Demo Public Library");
        assert!(single_list(&serde_json::json!({"library": {"items": []}}), "library").is_err());
    }

    #[test]
    fn user_details_single_object_fixture() {
        let data: Value = serde_json::json!({
            "items": {"patronId": "24680135791234", "downloadLimit": 5,
                      "availabledownload": 5,
                      "streamingTimeRemaining": "UNLIMITED",
                      "library": {"libraryId": 1320, "libraryName": "Demo Public Library"}}
        });
        let details: UserDetails =
            serde_json::from_value(single_object(&data, "items")).expect("UserDetails shape");
        assert_eq!(details.download_limit, Some(5));
        assert_eq!(
            details.streaming_time_remaining,
            Some(StreamTime::Unlimited("UNLIMITED".into()))
        );
    }

    #[test]
    fn search_song_container_fixture() {
        let data: Value = serde_json::json!({
            "songs": {"items": [{"songId": 377926024, "albumId": 9999900403297i64,
                                 "title": "最後まで - Aqua Timez 20th Live -OLDROSE-",
                                 "provider": 1, "sampleDurationSec": 30,
                                 "fullDurationSec": 283, "streamURL": ""}],
                      "limit": 2, "offset": 0,
                      "next": "https://api.freegalmusic.com/v1/search?type=song&q=aqua&offset=2&limit=2&flag=1",
                      "previous": ""}
        });
        match container_kind(SearchKind::Song, &data).expect("song container") {
            ContainerKind::Songs(page) => {
                assert_eq!(page.items.len(), 1);
                assert_eq!(page.items[0].id, 377926024);
                assert!(page.next_request().is_some());
            }
            _ => panic!("wrong container kind"),
        }
    }

    #[test]
    fn album_songs_record_fixture() {
        let data: Value = serde_json::json!({
            "album": {"items": [{
                "artist": {"artistId": "QXF1YQ", "name": "Aqua"},
                "albumId": 9999900418547i64, "title": "Aquarium", "provider": 1,
                "copyrightDate": 2026, "imageUrl": null,
                "songs": [
                    {"songId": 1, "title": "T1", "trackNumber": 1,
                     "fullDurationSec": 184, "sampleDurationSec": 30, "provider": 1},
                    {"songId": 2, "title": "T2", "trackNumber": 2,
                     "fullDurationSec": 201, "sampleDurationSec": 30, "provider": 1}
                ]
            }]}
        });
        let record = single_list(&data, "album").expect("album record");
        let album: Album = serde_json::from_value(record).expect("Album shape");
        assert_eq!(album.songs.len(), 2);
        assert_eq!(album.songs[0].track_number, Some(1));
        assert_eq!(album.year, Some(2026));
    }
}
