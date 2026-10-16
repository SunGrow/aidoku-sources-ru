#![no_std]
//! Aidoku source for com-x.life.
//!
//! The site runs DataLife Engine behind a Vue front end, but the pages
//! themselves are server rendered: catalog cards are plain HTML, and the
//! chapter and page lists are embedded in the markup as a `window.__DATA__`
//! JSON object. Parsing therefore only ever looks at served markup.
//!
//! Reaching that markup is the hard part. Every page sits behind a
//! JavaScript challenge, so the fetching here runs it in a web view and
//! reuses the cookies it wins — see `fetch`.

use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, HashMap, Listing,
	ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source,
	UpdateStrategy, WebLoginHandler,
	alloc::{String, Vec, string::ToString},
	helpers::uri::encode_uri_component,
	imports::{
		defaults::{DefaultValue, defaults_get, defaults_set},
		html::{Document, Html},
		js::{Cookie, WebView},
		net::Request,
		std::{parse_date, sleep},
	},
	prelude::*,
};
use serde::{Deserialize, de::DeserializeOwned};

const BASE_URL: &str = "https://com-x.life";

/// Chapter images live on a separate CDN host. The reader payload names it,
/// and this is the value it carries today, kept as a fallback.
const IMAGE_HOST: &str = "img.com-x.life";

/// Every chapter image path is relative to this directory on the CDN.
const IMAGE_PREFIX: &str = "comix";

/// Chapter dates, e.g. `15.10.2026`.
const DATE_FORMAT: &str = "dd.MM.yyyy";

// MARK: - Embedded page data

#[derive(Deserialize)]
struct DetailData {
	news_id: i64,
	#[serde(default)]
	chapters: Vec<ChapterEntry>,
}

#[derive(Deserialize)]
struct ChapterEntry {
	id: i64,
	/// Position in the series, counting up from the first chapter.
	#[serde(default)]
	posi: i32,
	#[serde(default)]
	volume: f32,
	#[serde(default)]
	number: f32,
	#[serde(default)]
	date: String,
}

#[derive(Deserialize)]
struct ReaderData {
	#[serde(default)]
	images: Vec<String>,
	#[serde(default)]
	host: String,
}

// MARK: - Networking

/// Marker present on any catalog or search page the site actually serves.
const LIST_MARKER: &str = "readed__title";

/// Marker present on every detail and reader page.
const DATA_MARKER: &str = "window.__DATA__";

/// Defaults key holding the cookies the challenge hands out once it has been
/// solved, so later requests can skip the web view.
const COOKIE_KEY: &str = "gate.cookies";

/// How many one second polls the web view is given to work through the
/// challenge before giving up on it.
const CHALLENGE_ATTEMPTS: usize = 20;

/// Reads the document as it currently stands in the web view.
const READ_DOCUMENT: &str = "document.documentElement.outerHTML";

fn stored_cookies() -> Option<String> {
	defaults_get::<String>(COOKIE_KEY).filter(|value| !value.is_empty())
}

/// Fetches a page.
///
/// Every page is behind a JavaScript challenge: a plain client is answered
/// with a 302 to a `/_c?t=…` interstitial whose script has to run before the
/// real page is served. Repeating the request does not help, because each
/// attempt is issued a fresh challenge, and neither does replaying the cookie
/// the redirect sets. The only way through is to run the script, so the page
/// is loaded in a web view.
///
/// Solving it yields cookies a plain request can reuse, and those are kept,
/// so the web view is only needed again once they stop being accepted. A
/// direct request is tried first for that reason — it is far cheaper, and
/// once the cookies are warm it is what serves almost every page.
fn fetch(url: &str, marker: &str) -> Result<String> {
	let mut request = Request::get(url)?;
	if let Some(cookies) = stored_cookies() {
		request = request.header("Cookie", cookies.as_str());
	}

	if let Ok(body) = request.string() {
		if body.contains(marker) {
			return Ok(body);
		}
	}

	solve_challenge(url, marker)
}

/// Loads a page in a web view so the challenge script runs, then keeps the
/// cookies it produced.
fn solve_challenge(url: &str, marker: &str) -> Result<String> {
	let webview = WebView::new();
	webview
		.load_blocking(Request::get(url)?)
		.map_err(|_| error!("Could not open the page"))?;

	// The interstitial is what finishes loading first; it only navigates on
	// to the real page once its script has run. So rather than reading the
	// document straight away, poll it until the marker shows up.
	//
	// The polling is done out here with `sleep` and a plain `eval` rather
	// than handed to `eval_async` as one promise, because the asynchronous
	// call returns nothing at all on device.
	let mut html = String::new();
	for attempt in 0..CHALLENGE_ATTEMPTS {
		if attempt > 0 {
			sleep(1);
		}
		// Reading can fail while a navigation is in flight, which is not
		// fatal here: the next poll picks it up.
		if let Ok(current) = webview.eval(READ_DOCUMENT) {
			html = current;
			if html.contains(marker) {
				break;
			}
		}
	}

	// Keep whatever grant the challenge issued even if the document could
	// not be read, since the cookies alone may be enough to fetch the page
	// over plain HTTP.
	let mut grant = None;
	if let Ok(cookies) = webview.get_cookies() {
		let header = cookie_header(&cookies);
		if !header.is_empty() {
			defaults_set(COOKIE_KEY, DefaultValue::String(header.clone()));
			grant = Some(header);
		}
	}

	if html.contains(marker) {
		return Ok(html);
	}

	// Last resort: ask for the page again carrying the new cookies.
	if let Some(grant) = grant {
		if let Ok(body) = Request::get(url)?.header("Cookie", grant.as_str()).string() {
			if body.contains(marker) {
				return Ok(body);
			}
		}
	}

	bail!("The site's challenge did not complete")
}

/// Builds a `Cookie` header out of the web view's store, keeping only this
/// site's cookies so nothing else leaks into the request.
fn cookie_header(cookies: &[Cookie]) -> String {
	let mut header = String::new();
	for cookie in cookies {
		if !cookie.domain.contains("com-x.life") {
			continue;
		}
		if !header.is_empty() {
			header.push_str("; ");
		}
		header.push_str(&cookie.name);
		header.push('=');
		header.push_str(&cookie.value);
	}
	header
}

fn fetch_document(url: &str) -> Result<Document> {
	Ok(Html::parse(fetch(url, LIST_MARKER)?)?)
}

/// Finds the `window.__DATA__ = {…}` object literal a page embeds.
///
/// This walks the braces while tracking string state rather than matching a
/// pattern: the payload holds titles that contain braces and escaped quotes,
/// which a naive search would truncate on.
fn embedded_data(html: &str) -> Option<&str> {
	let start = html.find("window.__DATA__")?;
	let rest = &html[start..];
	let open = rest.find('{')?;

	let bytes = rest.as_bytes();
	let mut depth = 0usize;
	let mut in_string = false;
	let mut escaped = false;

	for index in open..bytes.len() {
		let byte = bytes[index];

		if in_string {
			if escaped {
				escaped = false;
			} else if byte == b'\\' {
				escaped = true;
			} else if byte == b'"' {
				in_string = false;
			}
			continue;
		}

		match byte {
			b'"' => in_string = true,
			b'{' => depth += 1,
			b'}' => {
				depth -= 1;
				if depth == 0 {
					// Both delimiters are ASCII, so this stays on a character
					// boundary even though the payload is full of Cyrillic.
					return Some(&rest[open..=index]);
				}
			}
			_ => {}
		}
	}
	None
}

fn parse_embedded<T: DeserializeOwned>(html: &str) -> Result<T> {
	let json = embedded_data(html).ok_or_else(|| error!("Page did not embed any data"))?;
	serde_json::from_str(json).map_err(|_| error!("Could not decode the embedded page data"))
}

// MARK: - Parsing helpers

/// Turns `https://com-x.life/34147-priemnaja-doch.html` into
/// `34147-priemnaja-doch`, which is used as the manga key.
fn manga_key(href: &str) -> Option<String> {
	let slug = href.trim_end_matches('/').rsplit('/').next()?;
	let slug = slug.strip_suffix(".html").unwrap_or(slug);

	// Every entry is prefixed with its numeric id; anything else is a nav link.
	if slug.is_empty() || !slug.starts_with(|c: char| c.is_ascii_digit()) {
		return None;
	}
	Some(slug.to_string())
}

/// The numeric id leading a manga key, which the reader URL is built from.
fn news_id(key: &str) -> Option<&str> {
	let id = key.split('-').next()?;
	if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
		return None;
	}
	Some(id)
}

/// Cards label titles as `English name / Русское название`. Readers of this
/// source want the Russian one, so the segment that actually carries Cyrillic
/// is preferred rather than assuming a fixed order — and titles like
/// `Spider-Man/Deadpool` mean only ` / ` can be used as the separator.
fn primary_title(raw: &str) -> String {
	// `rsplit` rather than `split(..).rev()`: splitting on a string pattern
	// does not give a double ended iterator.
	raw.rsplit(" / ")
		.map(str::trim)
		.find(|part| part.chars().any(is_cyrillic))
		.unwrap_or(raw.trim())
		.to_string()
}

fn is_cyrillic(c: char) -> bool {
	matches!(c, '\u{0400}'..='\u{04FF}')
}

/// Reads one of the `<li>Год: 2026</li>` rows from a detail page.
fn labelled_value(html: &Document, label: &str) -> Option<String> {
	let rows = html.select("div.page__list li")?;
	for row in rows {
		let text = row.text()?;
		let trimmed = text.trim();
		if let Some(rest) = trimmed.strip_prefix(label) {
			let value = rest.trim().to_string();
			if !value.is_empty() {
				return Some(value);
			}
		}
	}
	None
}

/// Parses the cards shared by the catalog and the search results.
fn parse_cards(html: &Document) -> Vec<Manga> {
	let Some(cards) = html.select("div.readed.d-flex.short") else {
		return Vec::new();
	};

	cards
		.into_iter()
		.filter_map(|card| {
			let link = card.select_first("a.readed__img")?;
			let key = manga_key(&link.attr("href")?)?;

			// The heading is the usual source of the title; the cover's alt
			// text carries the same string and stands in if the markup moves.
			let title = card
				.select_first("h3.readed__title a")
				.and_then(|heading| heading.text())
				.or_else(|| card.select_first("a.readed__img img").and_then(|i| i.attr("alt")))
				.map(|raw| primary_title(&raw))
				.filter(|title| !title.is_empty())?;

			// Covers are lazy-loaded, so the real path sits in `data-src`.
			let cover = card.select_first("a.readed__img img").and_then(|img| {
				img.attr("data-src")
					.or_else(|| img.attr("data-original"))
					.or_else(|| img.attr("src"))
			});

			Some(Manga {
				key: key.clone(),
				title,
				cover,
				url: Some(format!("{BASE_URL}/{key}.html")),
				update_strategy: UpdateStrategy::Always,
				..Default::default()
			})
		})
		.collect()
}

/// The pagination block links every reachable page, so looking for the next
/// one is a dependable end-of-list check. Counting cards is not: the catalog
/// returns twenty per page while search returns ten.
fn has_next_page(html: &Document, page: i32) -> bool {
	let Some(links) = html.select("div.pagination a") else {
		return false;
	};
	let needle = format!("/page/{}/", page + 1);
	links
		.into_iter()
		.filter_map(|link| link.attr("href"))
		.any(|href| href.trim_end_matches('/').ends_with(needle.trim_end_matches('/')))
}

// MARK: - Source

struct ComX;

impl ComX {
	fn fetch_cards(&self, url: String, page: i32) -> Result<MangaPageResult> {
		let html = fetch_document(&url)?;
		Ok(MangaPageResult {
			entries: parse_cards(&html),
			has_next_page: has_next_page(&html, page),
		})
	}

	/// Builds a catalog URL.
	///
	/// Filters are path segments of the form `key=value`. A key takes several
	/// values as a comma separated list, and the site returns the titles
	/// carrying all of them. The other separators the site accepts are traps:
	/// `g=17/g=76` silently keeps only the last value and `g=17;76` only the
	/// first, both without any error, so the comma is the only usable form.
	fn catalog_url(page: i32, filters: &[FilterValue]) -> String {
		let mut url = String::from(BASE_URL);
		url.push_str("/ComicList");

		for filter in filters {
			match filter {
				FilterValue::MultiSelect { id, included, .. } if !included.is_empty() => {
					let mut joined = String::new();
					for (index, value) in included.iter().enumerate() {
						if index > 0 {
							joined.push(',');
						}
						joined.push_str(value);
					}
					push_segment(&mut url, id, &joined);
				}
				FilterValue::Select { id, value } if !value.is_empty() => {
					push_segment(&mut url, id, value);
				}
				FilterValue::Text { id, value } if !value.trim().is_empty() => {
					push_segment(&mut url, id, &encode_uri_component(value.trim()));
				}
				_ => {}
			}
		}

		url.push('/');
		if page > 1 {
			url.push_str(&format!("page/{page}/"));
		}
		url
	}
}

fn push_segment(url: &mut String, key: &str, value: &str) {
	url.push('/');
	url.push_str(key);
	url.push('=');
	url.push_str(value);
}

impl Source for ComX {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		if let Some(query) = query.filter(|q| !q.trim().is_empty()) {
			let mut url = format!("{BASE_URL}/search/{}", encode_uri_component(query.trim()));
			url.push('/');
			if page > 1 {
				url.push_str(&format!("page/{page}/"));
			}
			return self.fetch_cards(url, page);
		}

		self.fetch_cards(Self::catalog_url(page, &filters), page)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let body = fetch(&format!("{BASE_URL}/{}.html", manga.key), DATA_MARKER)?;
		let html = Html::parse(&body)?;
		let data: DetailData = parse_embedded(&body)?;

		if needs_details {
			manga.url = Some(format!("{BASE_URL}/{}.html", manga.key));

			if let Some(title) = html.select_first("h1").and_then(|h| h.text()) {
				let title = primary_title(&title);
				if !title.is_empty() {
					manga.title = title;
				}
			}

			if let Some(cover) = html
				.select_first("div.page__poster img")
				.and_then(|img| img.attr("src"))
			{
				manga.cover = Some(cover);
			}

			manga.description = html
				.select_first("div.page__text.full-text.clearfix")
				.and_then(|text| text.text());

			if let Some(author) = labelled_value(&html, "Автор:") {
				manga.authors = Some(Vec::from([author]));
			}
			if let Some(artist) = labelled_value(&html, "Художник:") {
				manga.artists = Some(Vec::from([artist]));
			}

			// `Завершен, перевод продолжается` has to be tested before the
			// plain completed label, which is a prefix of it.
			manga.status = match labelled_value(&html, "Статус:").as_deref() {
				Some(status) if status.starts_with("Завершен, перевод") => MangaStatus::Ongoing,
				Some("Завершён") | Some("Завершен") => MangaStatus::Completed,
				Some("Заморожен") | Some("Приостановлен") => MangaStatus::Hiatus,
				Some("Продолжается") => MangaStatus::Ongoing,
				_ => MangaStatus::Unknown,
			};

			if manga.status == MangaStatus::Completed {
				manga.update_strategy = UpdateStrategy::Never;
			}

			// Genres are the catalog links carrying a `g=` segment. The same
			// genre is linked more than once on the page, so repeats are
			// dropped while the original order is kept.
			if let Some(links) = html.select("a[href*=\"/ComicList/\"]") {
				let mut tags: Vec<String> = Vec::new();
				for link in links {
					let Some(href) = link.attr("href") else {
						continue;
					};
					if !href.contains("/g=") {
						continue;
					}
					let Some(name) = link.text() else {
						continue;
					};
					let name = name.trim().to_string();
					if !name.is_empty() && !tags.contains(&name) {
						tags.push(name);
					}
				}
				if !tags.is_empty() {
					manga.tags = Some(tags);
				}
			}

			// Covers carry an age badge such as `13+` or `18+`.
			manga.content_rating = match html
				.select_first("div.poster__label--age")
				.and_then(|label| label.text())
				.and_then(|label| {
					label
						.trim()
						.chars()
						.take_while(|c| c.is_ascii_digit())
						.collect::<String>()
						.parse::<i32>()
						.ok()
				}) {
				Some(age) if age >= 18 => ContentRating::NSFW,
				Some(age) if age >= 16 => ContentRating::Suggestive,
				_ => ContentRating::Safe,
			};
		}

		if needs_chapters {
			let news_id = data.news_id;
			let mut entries = data.chapters;

			// The payload already arrives newest first, but ordering it here
			// means a change in that habit cannot quietly reverse the list.
			entries.sort_by(|a, b| b.posi.cmp(&a.posi));

			manga.chapters = Some(
				entries
					.into_iter()
					.map(|entry| Chapter {
						key: entry.id.to_string(),
						chapter_number: Some(entry.number),
						volume_number: (entry.volume > 0.0).then_some(entry.volume),
						date_uploaded: parse_date(&entry.date, DATE_FORMAT),
						url: Some(format!("{BASE_URL}/reader/{news_id}/{}", entry.id)),
						language: Some(String::from("ru")),
						..Default::default()
					})
					.collect(),
			);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let news_id =
			news_id(&manga.key).ok_or_else(|| error!("Manga key has no id to read from"))?;

		let body = fetch(
			&format!("{BASE_URL}/reader/{news_id}/{}", chapter.key),
			DATA_MARKER,
		)?;
		let ReaderData { images, host } = parse_embedded(&body)?;

		if images.is_empty() {
			bail!("Chapter has no pages");
		}

		let host = if host.is_empty() {
			String::from(IMAGE_HOST)
		} else {
			host
		};

		Ok(images
			.into_iter()
			.map(|path| Page {
				content: PageContent::url(format!("https://{host}/{IMAGE_PREFIX}/{path}")),
				..Default::default()
			})
			.collect())
	}
}

impl ListingProvider for ComX {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		// The catalog's own order is by date, newest first. The site changes
		// it through a form post that stores the choice in the session rather
		// than through the URL, so the listings here select sections instead.
		let filters: Vec<FilterValue> = match listing.id.as_str() {
			"manga" => section("3"),
			"manhwa" => section("44"),
			"manhua" => section("45"),
			_ => Vec::new(),
		};

		self.fetch_cards(Self::catalog_url(page, &filters), page)
	}
}

fn section(id: &str) -> Vec<FilterValue> {
	Vec::from([FilterValue::Select {
		id: String::from("p.cat"),
		value: String::from(id),
	}])
}

impl DeepLinkHandler for ComX {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		let path = path.trim_start_matches('/');

		// `/reader/<news id>/<chapter id>` names the chapter by id only, so
		// the manga key is read back from the reader payload, which links to
		// the title it belongs to.
		if let Some(rest) = path.strip_prefix("reader/") {
			let mut parts = rest.trim_end_matches('/').split('/');
			let (Some(_), Some(chapter)) = (parts.next(), parts.next()) else {
				return Ok(None);
			};

			let body = fetch(&url, DATA_MARKER)?;
			let Some(link) = embedded_string(&body, "post_link") else {
				return Ok(None);
			};
			let Some(manga_key) = manga_key(&link) else {
				return Ok(None);
			};

			return Ok(Some(DeepLinkResult::Chapter {
				manga_key,
				key: chapter.to_string(),
			}));
		}

		Ok(manga_key(path).map(|key| DeepLinkResult::Manga { key }))
	}
}

/// Pulls a single string field out of the embedded payload without decoding
/// all of it, used where only one value is needed.
fn embedded_string(html: &str, field: &str) -> Option<String> {
	let data = embedded_data(html)?;
	let needle = format!("\"{field}\":\"");
	let start = data.find(&needle)? + needle.len();
	let rest = &data[start..];
	let end = rest.find('"')?;
	Some(rest[..end].replace("\\/", "/"))
}

impl WebLoginHandler for ComX {
	fn handle_web_login(&self, _key: String, _cookies: HashMap<String, String>) -> Result<bool> {
		// The session cookie's name is not documented, so rather than guess at
		// one, ask the site: every catalog page states the signed-in state in
		// the filter payload it embeds.
		let body = fetch(&format!("{BASE_URL}/ComicList/"), LIST_MARKER)?;
		Ok(body.contains("\"is_logged\":true"))
	}
}

register_source!(ComX, ListingProvider, DeepLinkHandler, WebLoginHandler);
