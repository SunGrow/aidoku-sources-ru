#![no_std]
//! Aidoku source for wamanga.ru.
//!
//! The site exposes a documented REST API (`/api/docs-json`), so everything here
//! talks JSON rather than scraping HTML.

use aidoku::{
	AidokuError, BasicLoginHandler, Chapter, ContentRating, DeepLinkHandler, DeepLinkResult,
	FilterValue, Home, HomeComponent, HomeComponentValue, HomeLayout, Link, LinkValue, Listing,
	ListingKind, ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent, Result,
	Source, UpdateStrategy, Viewer,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{
		defaults::{DefaultValue, defaults_get, defaults_set},
		net::Request,
		std::parse_date,
	},
	prelude::*,
};
use core::cmp::Ordering;
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://wamanga.ru";
const API_URL: &str = "https://wamanga.ru/api/v1";
const PAGE_LIMIT: i32 = 30;

/// How many of the top titles get the large treatment on the home page.
const FEATURED_COUNT: usize = 5;

fn listing(id: &str, name: &str) -> Listing {
	Listing {
		id: String::from(id),
		name: String::from(name),
		kind: ListingKind::Default,
	}
}

fn manga_links(entries: Vec<Manga>) -> Vec<Link> {
	entries
		.into_iter()
		.map(|manga| Link {
			title: manga.title.clone(),
			image_url: manga.cover.clone(),
			value: Some(LinkValue::Manga(manga)),
			..Default::default()
		})
		.collect()
}

/// Turns one listing's first page into a home row: the first gets the
/// large carousel, the rest a plain scroller that opens the listing.
fn home_row(title: &str, id: &str, entries: Vec<Manga>, big: bool) -> HomeComponent {
	if big {
		return HomeComponent {
			value: HomeComponentValue::BigScroller {
				entries: entries.into_iter().take(FEATURED_COUNT).collect(),
				auto_scroll_interval: Some(8.0),
			},
			..Default::default()
		};
	}

	HomeComponent {
		title: Some(String::from(title)),
		value: HomeComponentValue::Scroller {
			entries: manga_links(entries),
			listing: Some(listing(id, title)),
		},
		..Default::default()
	}
}

/// Defaults keys holding the JWT pair issued by `/auth/login`.
const ACCESS_TOKEN_KEY: &str = "login.accessToken";
const REFRESH_TOKEN_KEY: &str = "login.refreshToken";

/// Date format used by every timestamp the API returns, e.g.
/// `2026-08-03T19:11:08.633Z`. The fractional part and the trailing `Z` are
/// trimmed before parsing, so only the stable prefix is described here.
const DATE_FORMAT: &str = "yyyy-MM-dd'T'HH:mm:ss";

// MARK: - API models

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiManga {
	id: String,
	slug: Option<String>,
	title: Option<String>,
	title_english: Option<String>,
	cover_url: Option<String>,
	description: Option<String>,
	genres: Option<Vec<String>>,
	authors: Option<Vec<String>>,
	artists: Option<Vec<String>>,
	status_title: Option<String>,
	#[serde(rename = "type")]
	kind: Option<String>,
	is_adult: Option<bool>,
	pegi_rating: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiChapter {
	id: String,
	title: Option<String>,
	position: Option<f32>,
	created_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiFile {
	disk_file: Option<String>,
	position: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiChapterDetail {
	files: Option<Vec<ApiFile>>,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
	username: &'a str,
	password: &'a str,
}

/// `/auth/login` answers with a JWT pair in snake_case, unlike the rest of the
/// API.
#[derive(Deserialize)]
struct LoginResponse {
	access_token: String,
	refresh_token: String,
}

// MARK: - Helpers

/// Treats blank strings as absent. The API returns `""` rather than `null` for
/// missing chapter titles and descriptions.
fn non_empty(value: Option<String>) -> Option<String> {
	value.filter(|s| !s.trim().is_empty())
}

fn access_token() -> Option<String> {
	defaults_get::<String>(ACCESS_TOKEN_KEY).filter(|token| !token.is_empty())
}

/// Builds a GET request, carrying the bearer token when the reader is signed
/// in. Browsing works fine signed out, so a missing token is not an error.
fn api_get(url: String) -> Result<Request> {
	let request = Request::get(url)?;
	Ok(match access_token() {
		Some(token) => request.header("Authorization", format!("Bearer {token}").as_str()),
		None => request,
	})
}

/// Resolves the `/app/uploads/...` paths the API returns into absolute urls.
fn absolute_url(path: &str) -> String {
	if path.starts_with("http") {
		path.to_string()
	} else {
		format!("{BASE_URL}{path}")
	}
}

/// Parses an ISO-8601 timestamp by trimming the fractional seconds and zone
/// suffix, which `DateFormatter` would otherwise reject.
fn parse_timestamp(value: Option<String>) -> Option<i64> {
	let value = non_empty(value)?;
	let trimmed = value.get(..19)?;
	parse_date(trimmed, DATE_FORMAT)
}

fn parse_status(value: Option<&String>) -> MangaStatus {
	match value.map(|s| s.as_str()) {
		Some("ongoing") => MangaStatus::Ongoing,
		Some("completed") => MangaStatus::Completed,
		Some("hiatus") => MangaStatus::Hiatus,
		Some("cancelled") | Some("abandoned") => MangaStatus::Cancelled,
		_ => MangaStatus::Unknown,
	}
}

/// Picks the reading direction from the title's type. Korean and Chinese
/// releases on this site are long-strip, Japanese ones are right-to-left.
fn parse_viewer(kind: Option<&String>) -> Viewer {
	match kind.map(|s| s.as_str()) {
		Some("manga") => Viewer::RightToLeft,
		Some("manhwa") | Some("manhua") => Viewer::Webtoon,
		Some("comic") => Viewer::LeftToRight,
		_ => Viewer::Unknown,
	}
}

fn parse_content_rating(is_adult: Option<bool>, pegi: Option<&String>) -> ContentRating {
	if is_adult.unwrap_or(false) {
		return ContentRating::NSFW;
	}
	match pegi.map(|s| s.as_str()) {
		Some("18+") => ContentRating::NSFW,
		Some("16+") => ContentRating::Suggestive,
		_ => ContentRating::Safe,
	}
}

impl ApiManga {
	/// Builds the public site url, used for "open in browser" and deep links.
	fn public_url(&self) -> Option<String> {
		let kind = self.kind.as_ref()?;
		let slug = self.slug.as_ref()?;
		Some(format!("{BASE_URL}/{kind}/{slug}"))
	}

	fn into_manga(self) -> Manga {
		let title = non_empty(self.title.clone())
			.or_else(|| non_empty(self.title_english.clone()))
			.unwrap_or_default();

		Manga {
			key: self.id.clone(),
			title,
			cover: self.cover_url.as_deref().map(absolute_url),
			artists: self.artists.clone(),
			authors: self.authors.clone(),
			description: non_empty(self.description.clone()),
			url: self.public_url(),
			tags: self.genres.clone(),
			status: parse_status(self.status_title.as_ref()),
			content_rating: parse_content_rating(self.is_adult, self.pegi_rating.as_ref()),
			viewer: parse_viewer(self.kind.as_ref()),
			update_strategy: UpdateStrategy::Always,
			next_update_time: None,
			chapters: None,
		}
	}
}

impl ApiChapter {
	fn into_chapter(self) -> Chapter {
		Chapter {
			key: self.id,
			title: non_empty(self.title),
			chapter_number: self.position,
			volume_number: None,
			date_uploaded: parse_timestamp(self.created_at),
			scanlators: None,
			url: None,
			language: Some("ru".into()),
			thumbnail: None,
			locked: false,
		}
	}
}

// MARK: - Source

struct WaManga;

impl WaManga {
	/// Runs a catalog query and wraps it into a page result. The API returns a
	/// bare array with no total count, so a full page is taken to mean there
	/// may be another one.
	fn fetch_list(&self, query: QueryParameters) -> Result<MangaPageResult> {
		let url = format!("{API_URL}/manga?{query}");
		let entries: Vec<ApiManga> = api_get(url)?.json_owned()?;
		let has_next_page = entries.len() as i32 >= PAGE_LIMIT;

		Ok(MangaPageResult {
			entries: entries.into_iter().map(ApiManga::into_manga).collect(),
			has_next_page,
		})
	}

	fn base_query(page: i32) -> QueryParameters {
		let mut query = QueryParameters::new();
		query.push("limit", Some(&PAGE_LIMIT.to_string()));
		let offset = (page.max(1) - 1) * PAGE_LIMIT;
		query.push("offset", Some(&offset.to_string()));
		query
	}

	/// Maps the index of the `sort` filter onto the API's `sortKey` values, in
	/// the same order the options appear in `filters.json`.
	fn sort_key(index: i32) -> &'static str {
		match index {
			1 => "createdAt",
			2 => "publicDate",
			3 => "views",
			4 => "likes",
			5 => "alphabetical",
			_ => "updatedAt",
		}
	}
}

impl Source for WaManga {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let mut params = Self::base_query(page);

		if let Some(query) = non_empty(query) {
			params.push("query", Some(&query));
		}

		for filter in filters {
			match filter {
				FilterValue::Sort {
					index, ascending, ..
				} => {
					params.push("sortKey", Some(Self::sort_key(index)));
					params.push("sortDescending", Some(if ascending { "false" } else { "true" }));
				}
				// Filter ids double as the API's parameter names, and the API
				// expects array values as repeated parameters.
				FilterValue::MultiSelect { id, included, .. } => {
					for value in included {
						params.push(&id, Some(&value));
					}
				}
				FilterValue::Select { id, value } => {
					params.push(&id, Some(&value));
				}
				FilterValue::Range { id, from, to } if id == "year" => {
					if let Some(from) = from {
						params.push("releaseYearFrom", Some(&(from as i32).to_string()));
					}
					if let Some(to) = to {
						params.push("releaseYearTo", Some(&(to as i32).to_string()));
					}
				}
				_ => {}
			}
		}

		self.fetch_list(params)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if needs_details {
			let url = format!("{API_URL}/manga/{}", manga.key);
			let details: ApiManga = api_get(url)?.json_owned()?;
			manga.copy_from(details.into_manga());
		}

		if needs_chapters {
			let url = format!("{API_URL}/manga/{}/chapters", manga.key);
			let chapters: Vec<ApiChapter> = api_get(url)?.json_owned()?;
			let mut chapters: Vec<Chapter> =
				chapters.into_iter().map(ApiChapter::into_chapter).collect();

			// The API lists chapters oldest first; Aidoku shows them newest first.
			chapters.sort_by(|a, b| {
				b.chapter_number
					.partial_cmp(&a.chapter_number)
					.unwrap_or(Ordering::Equal)
			});
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{API_URL}/chapters/{}", chapter.key);
		let detail: ApiChapterDetail = api_get(url)?.json_owned()?;

		let Some(mut files) = detail.files else {
			bail!("Chapter contains no pages");
		};

		// `position` is a decimal string such as "1.002000000" and the files
		// come back unordered.
		files.sort_by(|a, b| {
			let parse = |value: &Option<String>| {
				value
					.as_ref()
					.and_then(|v| v.parse::<f32>().ok())
					.unwrap_or(0.0)
			};
			parse(&a.position)
				.partial_cmp(&parse(&b.position))
				.unwrap_or(Ordering::Equal)
		});

		Ok(files
			.into_iter()
			.filter_map(|file| {
				let path = non_empty(file.disk_file)?;
				Some(Page {
					content: PageContent::url(absolute_url(&path)),
					thumbnail: None,
					has_description: false,
					description: None,
				})
			})
			.collect())
	}
}

impl ListingProvider for WaManga {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let mut params = Self::base_query(page);

		let sort_key = match listing.id.as_str() {
			"popular" => "views",
			"new" => "createdAt",
			"latest" => "updatedAt",
			_ => return Err(AidokuError::Unimplemented),
		};
		params.push("sortKey", Some(sort_key));
		params.push("sortDescending", Some("true"));

		self.fetch_list(params)
	}
}

impl Home for WaManga {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components = Vec::new();

		// The API answers quickly and is not rate limited in any way the
		// source has run into, so the page is built from the same three
		// listings the source already offers. A row that fails is left
		// out rather than taking the whole page down with it.
		for (title, id, sort_key, big) in [
			("Популярное", "popular", "views", true),
			("Обновления", "latest", "updatedAt", false),
			("Новинки", "new", "createdAt", false),
		] {
			let mut params = Self::base_query(1);
			params.push("sortKey", Some(sort_key));
			params.push("sortDescending", Some("true"));

			let Ok(result) = self.fetch_list(params) else {
				continue;
			};
			if result.entries.is_empty() {
				continue;
			}
			components.push(home_row(title, id, result.entries, big));
		}

		Ok(HomeLayout { components })
	}
}

impl DeepLinkHandler for WaManga {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};

		// Title pages are `/{type}/{slug}`, and the id needed for a manga key
		// is only available through the slug lookup endpoint.
		let mut segments = path.trim_matches('/').split('/');
		let Some(kind) = segments.next().filter(|s| !s.is_empty()) else {
			return Ok(None);
		};
		let Some(slug) = segments.next().filter(|s| !s.is_empty()) else {
			return Ok(None);
		};

		let lookup = format!("{API_URL}/manga/get/{kind}/{slug}?chapters=false");
		let manga: ApiManga = api_get(lookup)?.json_owned()?;

		Ok(Some(DeepLinkResult::Manga { key: manga.id }))
	}
}

impl BasicLoginHandler for WaManga {
	fn handle_basic_login(&self, _key: String, username: String, password: String) -> Result<bool> {
		// Aidoku sends empty credentials when the reader logs out.
		if username.is_empty() && password.is_empty() {
			defaults_set(ACCESS_TOKEN_KEY, DefaultValue::Null);
			defaults_set(REFRESH_TOKEN_KEY, DefaultValue::Null);
			return Ok(false);
		}

		let body = serde_json::to_string(&LoginRequest {
			username: &username,
			password: &password,
		})
		.map_err(|_| error!("Could not encode the login request"))?;

		let response = Request::post(format!("{API_URL}/auth/login"))?
			.header("Content-Type", "application/json")
			.body(body)
			.send()?;

		// 401 is a wrong password and 404 an unknown user; both just mean the
		// login did not succeed.
		if response.status_code() != 200 {
			return Ok(false);
		}

		let tokens: LoginResponse = response.get_json_owned()?;
		defaults_set(ACCESS_TOKEN_KEY, DefaultValue::String(tokens.access_token));
		defaults_set(
			REFRESH_TOKEN_KEY,
			DefaultValue::String(tokens.refresh_token),
		);

		Ok(true)
	}
}

register_source!(
	WaManga,
	ListingProvider,
	DeepLinkHandler,
	BasicLoginHandler,
	Home
);
