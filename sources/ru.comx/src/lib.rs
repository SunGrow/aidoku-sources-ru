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
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue, HashMap,
	Home, HomeComponent, HomeComponentValue, HomeLayout, ImageRequestProvider, Link, LinkValue,
	Listing, ListingKind,
	ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent, PageContext, Result,
	Source,
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

/// Western publisher sections: everything the site files outside manga,
/// manhwa and manhua, which is most of what it carries.
///
/// `p.cat` unions the values it is given, so these can be asked for in a
/// single request. Note this is the opposite of the genre filter, which
/// intersects them.
const COMIC_SECTIONS: &[&str] = &[
	"2", "14", "7", "6", "4", "5", "8", "9", "10", "11", "12", "13", "15", "16", "18", "50",
	"51",
];

/// Every section the site files a title under.
///
/// Asking for all of them is how the full catalog is requested. The bare
/// `/ComicList/` is not: it serves a trimmed landing page of ten entries
/// whose own pagination links point somewhere else, so `/ComicList/page/2/`
/// hands back the first page again and scrolling never advances.
const ALL_SECTIONS: &[&str] = &[
	"50", "11", "12", "7", "14", "10", "16", "6", "4", "2", "13", "9", "15", "8", "5", "51",
	"3", "44", "45", "18",
];

/// The sort fields the site accepts, in the order the sort filter lists
/// them. The index the filter reports is an index into this.
const SORT_FIELDS: &[&str] = &[
	"date",
	"editdate",
	"rating",
	"news_read",
	"comm_num",
	"title",
	"year",
];

/// How many of the newest titles get the large treatment on the home page.
const FEATURED_COUNT: usize = 5;

/// A popular slice of the genre list, shown as chips on the home page. All
/// 194 of them belong in the filter sheet rather than in a row.
const HOME_GENRES: &[(&str, &str)] = &[
	("17", "боевик"),
	("76", "романтика"),
	("106", "фэнтези"),
	("29", "драма"),
	("42", "комедия"),
	("67", "приключения"),
	("105", "фантастика"),
	("102", "ужасы"),
	("25", "детектив"),
	("111", "школа"),
	("235", "исекай"),
	("87", "супергероика"),
];

// MARK: - Embedded page data

/// A scalar as the payload actually delivers it.
///
/// The embedded JSON is not consistent about shape: a number can arrive
/// quoted, and a field the site has nothing for arrives as `null` rather
/// than being left out. A typed field makes any one of those fail the
/// whole document, and because the document carries the chapter list the
/// failure used to take the title, cover and description down with it. So
/// every scalar is read loosely here and interpreted afterwards.
#[derive(Deserialize)]
#[serde(untagged)]
enum Loose {
	Number(f64),
	Text(String),
	/// Anything else, kept only so that deserializing cannot fail.
	Other(#[allow(dead_code)] serde_json::Value),
}

impl Loose {
	fn as_i64(&self) -> Option<i64> {
		match self {
			Loose::Number(value) => Some(*value as i64),
			Loose::Text(text) => text.trim().parse::<i64>().ok(),
			Loose::Other(_) => None,
		}
	}

	fn as_f32(&self) -> Option<f32> {
		match self {
			Loose::Number(value) => Some(*value as f32),
			Loose::Text(text) => text.trim().parse::<f32>().ok(),
			Loose::Other(_) => None,
		}
	}

	fn as_text(&self) -> Option<&str> {
		match self {
			Loose::Text(text) => Some(text.as_str()),
			_ => None,
		}
	}
}

#[derive(Deserialize)]
struct DetailData {
	#[serde(default)]
	news_id: Option<Loose>,
	/// Left undecoded on purpose: entries are read one at a time below, so
	/// a single chapter the payload describes oddly is dropped instead of
	/// emptying the list.
	#[serde(default)]
	chapters: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ChapterEntry {
	#[serde(default)]
	id: Option<Loose>,
	/// Position in the series, counting up from the first chapter.
	#[serde(default)]
	posi: Option<Loose>,
	#[serde(default)]
	volume: Option<Loose>,
	#[serde(default)]
	number: Option<Loose>,
	/// Usually just restates the number, but a collected edition is
	/// labelled with the range it covers, e.g. `1 - 13`.
	#[serde(default)]
	title: Option<Loose>,
	#[serde(default)]
	date: Option<Loose>,
}

/// One chapter, once the payload has been made sense of.
struct ChapterRow {
	id: i64,
	posi: f32,
	volume: f32,
	number: Option<f32>,
	title: Option<String>,
	date: Option<String>,
}

#[derive(Deserialize)]
struct ReaderData {
	#[serde(default)]
	images: Vec<Loose>,
	#[serde(default)]
	host: Option<Loose>,
}

// MARK: - Networking

/// Marker present on any catalog or search page the site actually serves.
const LIST_MARKER: &str = "readed__title";

/// Marker present on every detail and reader page.
const DATA_MARKER: &str = "window.__DATA__";

/// Defaults key holding the cookies the challenge hands out once it has been
/// solved, so later requests can skip the web view.
const COOKIE_KEY: &str = "gate.cookies";

/// Where the cookies the web view login produced are kept. Deliberately
/// separate from the challenge grant, so that re-solving the challenge
/// cannot quietly sign the user back out.
const LOGIN_COOKIE_KEY: &str = "login.cookies";

/// How many one second polls the web view is given to work through the
/// challenge before giving up on it. The page load is no longer waited
/// for separately, so this budget has to cover it as well.
const CHALLENGE_ATTEMPTS: usize = 30;

/// Reads the document as it currently stands in the web view.
const READ_DOCUMENT: &str = "document.documentElement.outerHTML";

fn stored_cookies() -> Option<String> {
	defaults_get::<String>(COOKIE_KEY).filter(|value| !value.is_empty())
}

fn stored_login() -> Option<String> {
	defaults_get::<String>(LOGIN_COOKIE_KEY).filter(|value| !value.is_empty())
}

/// The cookie header a request goes out with.
///
/// Two jars are merged. The grant is rewritten every time the challenge
/// is solved, while the login jar is a snapshot taken when the reader
/// signed in, so on a name they share the grant is the fresher of the two
/// and wins. Letting the snapshot win, as this did, meant an expired
/// grant went out on every request for good: each solve stored a new one
/// and the snapshot covered it straight back up, so the source could
/// never recover on its own and only signing in again ever fixed it.
///
/// The exception is the cookies carrying the sign-in itself. The web view
/// that solves the challenge need not be signed in, so taking its copy of
/// those could quietly sign the reader out.
fn request_cookies() -> Option<String> {
	let mut merged: Vec<(String, String)> = Vec::new();

	for (jar, is_grant) in [(stored_login(), false), (stored_cookies(), true)] {
		let Some(jar) = jar else {
			continue;
		};
		for pair in jar.split("; ") {
			let Some((name, value)) = pair.split_once('=') else {
				continue;
			};
			match merged.iter_mut().find(|(seen, _)| seen == name) {
				Some(slot) => {
					if !(is_grant && is_session_cookie(name)) {
						slot.1 = String::from(value);
					}
				}
				None => merged.push((String::from(name), String::from(value))),
			}
		}
	}

	if merged.is_empty() {
		return None;
	}

	let mut header = String::new();
	for (index, (name, value)) in merged.iter().enumerate() {
		if index > 0 {
			header.push_str("; ");
		}
		header.push_str(name);
		header.push('=');
		header.push_str(value);
	}
	Some(header)
}

/// Whether a cookie name carries the sign-in rather than the challenge
/// grant. `PHPSESSID` is the site's session and the `dle_` family is the
/// engine's own login state; a solve must never overwrite either.
fn is_session_cookie(name: &str) -> bool {
	name.eq_ignore_ascii_case("PHPSESSID") || name.starts_with("dle_")
}

/// Attaches whatever cookies are on hand to a request.
fn with_cookies(request: Request) -> Request {
	match request_cookies() {
		Some(cookies) => request.header("Cookie", cookies.as_str()),
		None => request,
	}
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
	if let Ok(body) = with_cookies(Request::get(url)?).string() {
		// The marker says this is the page that was asked for. A body that
		// carries neither the marker nor the interstitial is still a real
		// answer — a search with no hits, a title that is gone — and asking
		// the web view about it only bought thirty seconds and an error
		// blaming the sign-in for an empty result.
		if !body.is_empty() && (body.contains(marker) || !is_gate(&body)) {
			return Ok(body);
		}
	}

	solve_challenge(url, marker)
}

/// Whether a body is the challenge's interstitial rather than a page.
///
/// Asking instead whether a page carries cards cannot tell an empty
/// search from a blocked request. The interstitial is recognisable on its
/// own terms: it posts its proof of work to `/_v` and stands in for the
/// page named in the url it was handed.
fn is_gate(body: &str) -> bool {
	body.contains("\"POST\", \"/_v\"") || body.contains("var targetUrl = decodeURIComponent(")
}

/// Loads a page in a web view so the challenge script runs, then keeps the
/// cookies it produced.
fn solve_challenge(url: &str, marker: &str) -> Result<String> {
	let webview = WebView::new();

	// `load` rather than `load_blocking`. Waiting for the load to finish
	// can never return, because the interstitial navigates away the moment
	// its script runs and the wait is left pending on a navigation that no
	// longer exists; on an iPad that hangs the listing forever, while the
	// same build happens to get away with it on a phone. Nothing below
	// blocks without a bound, so the poll does the waiting instead.
	webview
		.load(Request::get(url)?)
		.map_err(|_| error!("Could not open the page"))?;

	// The interstitial is what loads first; it only navigates on to the
	// real page once its script has run. So the document is polled until
	// the marker shows up, which also covers the page not existing yet.
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

	// Saying so plainly: a background web view cannot always clear this
	// gate, and the reader has no way to guess that signing in once
	// through the web view is what refreshes the grant that unblocks
	// every plain request after it.
	bail!(
		"Com-X did not let this request through. Open the source's web view sign-in once to refresh its cookies, then try again."
	)
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

/// Builds the form the catalog is asked for.
///
/// Sorting is not expressible in the URL. The site takes it as a form post
/// and then remembers the choice server side, which means a plain GET comes
/// back in whatever order was last asked for rather than in a known one. So
/// every catalog request states its sort, and this is what it states when
/// the user has not picked one.
fn sort_body(filters: &[FilterValue]) -> String {
	let mut field = SORT_FIELDS[0];
	let mut direction = "desc";

	for filter in filters {
		if let FilterValue::Sort {
			id,
			index,
			ascending,
		} = filter
		{
			if id != "sort" {
				continue;
			}
			if let Some(name) = usize::try_from(*index).ok().and_then(|i| SORT_FIELDS.get(i)) {
				field = name;
			}
			direction = if *ascending { "asc" } else { "desc" };
		}
	}

	// The two `set_` fields name the slot the site stores the choice in. It
	// ignores which slot is named when deciding what to return, but the form
	// it ships always sends them, so they are sent here too.
	format!(
		"dlenewssortby={field}&dledirection={direction}\
		&set_new_sort=dle_sort_xfilter&set_direction_sort=dle_direction_xfilter"
	)
}

/// Fetches a catalog page, stating the order it wants it in.
fn catalog_document(url: &str, body: &str) -> Result<Document> {
	let sorted = |request: Request| {
		with_cookies(request)
			.header("Content-Type", "application/x-www-form-urlencoded")
			.header("Referer", BASE_URL)
			.body(body)
			.string()
			.ok()
			.filter(|page| !is_gate(page))
	};

	// Only worth trying while there is something to send: without cookies
	// this can only come back as the interstitial, and the challenge has to
	// be solved before anything else can work anyway.
	if request_cookies().is_some()
		&& let Ok(request) = Request::post(url)
		&& let Some(page) = sorted(request)
	{
		return Ok(Html::parse(page)?);
	}

	// Either there were no usable cookies yet or they have gone stale, so
	// fall back to the ordinary fetch, which can re-solve the challenge.
	// That is a GET and so arrives in an unknown order; asking once more
	// with the fresh cookies is what puts the sort back.
	let page = fetch(url, LIST_MARKER)?;
	let resorted = Request::post(url).ok().and_then(sorted);
	Ok(Html::parse(resorted.unwrap_or(page))?)
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
	// A shared link can carry either of these, and neither belongs in a
	// key that is about to have `.html` put back on the end of it.
	let href = href.split('#').next().unwrap_or(href);
	let href = href.split('?').next().unwrap_or(href);
	let last = href.trim_end_matches('/').rsplit('/').next()?;

	// A title is a `.html` page. Accepting any segment that merely began
	// with a digit turned `/ComicList/page/2/` into a manga keyed `2`,
	// which then fetched a `/2.html` that does not exist.
	let slug = match last.strip_suffix(".html") {
		Some(slug) => slug,
		// Without the extension the segment still has to look like
		// `<id>-<slug>`; a bare number is pagination, not a title.
		None => match last.split_once('-') {
			Some((id, rest))
				if !id.is_empty()
				&& !rest.is_empty()
				&& id.chars().all(|c| c.is_ascii_digit()) =>
			{
				last
			}
			_ => return None,
		},
	};

	// The reader has its own page for a title under the same id.
	let slug = slug.strip_suffix("-read").unwrap_or(slug);

	if slug.is_empty() || !slug.starts_with(|c: char| c.is_ascii_digit()) {
		return None;
	}
	Some(String::from(slug))
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
	let raw = raw.trim();
	let parts: Vec<&str> = raw.split(" / ").map(str::trim).collect();

	// The Russian name is the tail of `English name / Русское название`.
	// Taking only the last Cyrillic part threw away half of a title that
	// is Cyrillic throughout: «Бэтмен / Супермен» came out as «Супермен».
	// So the whole run of Cyrillic parts at the end is kept.
	let start = parts
		.iter()
		.rposition(|part| !part.chars().any(is_cyrillic))
		.map(|index| index + 1)
		.unwrap_or(0);

	// Nothing Cyrillic anywhere, so there is no Russian name to prefer.
	if start >= parts.len() {
		return String::from(raw);
	}

	let mut title = String::new();
	for part in &parts[start..] {
		if !title.is_empty() {
			title.push_str(" / ");
		}
		title.push_str(part);
	}
	title
}

fn is_cyrillic(c: char) -> bool {
	matches!(c, '\u{0400}'..='\u{04FF}')
}

/// Reads the `<li>Год: 2021</li>` rows of a detail page, all in one pass.
///
/// The list is a `<ul class="page__list">`, so the `div.page__list` this
/// once looked for matched nothing, which left every title without an
/// author, an artist or a status. Reading every row once and handing back
/// the pairs costs the document one query rather than one per label.
fn labelled_rows(html: &Document) -> Vec<(String, String)> {
	let Some(rows) = html.select("ul.page__list li") else {
		return Vec::new();
	};

	let mut out: Vec<(String, String)> = Vec::new();
	for row in rows {
		// A row that will not give up its text is skipped rather than
		// ending the scan, which used to hide every row behind it.
		let Some(text) = row.text() else {
			continue;
		};
		let Some((label, value)) = text.trim().split_once(':') else {
			continue;
		};
		let (label, value) = (label.trim(), value.trim());
		if !label.is_empty() && !value.is_empty() {
			out.push((label.into(), value.into()));
		}
	}
	out
}

/// The value of one labelled row, by the label's own spelling without its
/// trailing colon.
fn labelled_value<'a>(rows: &'a [(String, String)], label: &str) -> Option<&'a str> {
	rows.iter()
		.find(|(name, _)| name == label)
		.map(|(_, value)| value.as_str())
}

/// Parses the cards shared by the catalog and the search results.
/// Builds the url of one chapter image.
///
/// The reader payload names a bare CDN host and a path relative to one
/// directory on it, and that is all it has ever been seen to carry. None of
/// it is guaranteed though, and a stray slash or scheme used to produce a
/// url with a doubled separator that fetches nothing, so each part is
/// trimmed back to the shape the join expects.
fn image_url(host: &str, path: &str) -> String {
	let path = path.trim();
	if path.starts_with("https://") || path.starts_with("http://") {
		return String::from(path);
	}

	let host = host.trim().trim_end_matches('/');
	let host = host
		.strip_prefix("https://")
		.or_else(|| host.strip_prefix("http://"))
		.unwrap_or(host);
	let host = if host.is_empty() { IMAGE_HOST } else { host };

	let path = path.trim_start_matches('/');
	let carries_prefix = path
		.strip_prefix(IMAGE_PREFIX)
		.is_some_and(|rest| rest.starts_with('/'));

	if carries_prefix {
		format!("https://{host}/{path}")
	} else {
		format!("https://{host}/{IMAGE_PREFIX}/{path}")
	}
}

/// Picks the chapter title worth showing.
///
/// Most entries only restate the number the app already prints, as `# 4`
/// or `4`, and repeating it beside the number is noise. A collected
/// edition is labelled with the range it covers instead, e.g. `1 - 13`,
/// and that is the one thing telling it apart from chapter 1.
fn chapter_title(raw: Option<&str>, number: Option<f32>) -> Option<String> {
	let title = raw.map(str::trim).filter(|title| !title.is_empty())?;

	if let (Ok(value), Some(number)) = (title.trim_start_matches('#').trim().parse::<f32>(), number)
		&& (value - number).abs() < f32::EPSILON
	{
		return None;
	}

	Some(String::from(title))
}

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
		let mut segments = String::new();

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
					push_segment(&mut segments, id, &joined);
				}
				FilterValue::Select { id, value } if !value.is_empty() => {
					push_segment(&mut segments, id, value);
				}
				FilterValue::Text { id, value } if !value.trim().is_empty() => {
					push_segment(&mut segments, id, &encode_uri_component(value.trim()));
				}
				_ => {}
			}
		}

		// Nothing to narrow it by means the whole catalog, which is asked for
		// by naming every section rather than by leaving the URL bare.
		if segments.is_empty() {
			let mut all = String::new();
			for (index, id) in ALL_SECTIONS.iter().enumerate() {
				if index > 0 {
					all.push(',');
				}
				all.push_str(id);
			}
			push_segment(&mut segments, "p.cat", &all);
		}

		let mut url = format!("{BASE_URL}/ComicList{segments}");

		url.push('/');
		if page > 1 {
			url.push_str(&format!("page/{page}/"));
		}
		url
	}

	/// Fetches a page of the catalog.
	fn fetch_catalog(&self, filters: &[FilterValue], page: i32) -> Result<MangaPageResult> {
		let html = catalog_document(&Self::catalog_url(page, filters), &sort_body(filters))?;
		Ok(MangaPageResult {
			entries: parse_cards(&html),
			has_next_page: has_next_page(&html, page),
		})
	}

	/// Fetches the first page of a section, for the home page.
	fn home_section(&self, filters: &[FilterValue]) -> Result<Vec<Manga>> {
		let html = catalog_document(&Self::catalog_url(1, filters), &sort_body(filters))?;
		Ok(parse_cards(&html))
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

		self.fetch_catalog(&filters, page)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let body = fetch(&format!("{BASE_URL}/{}.html", manga.key), DATA_MARKER)?;
		let html = Html::parse(&body)?;

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

			// Assigned only when there is something to assign: writing the
			// miss straight through cleared a description the entry already
			// had whenever the block was absent.
			if let Some(description) = html
				.select_first("div.page__text.full-text.clearfix")
				.and_then(|text| text.text())
				.map(|text| String::from(text.trim()))
				.filter(|text| !text.is_empty())
			{
				manga.description = Some(description);
			}

			let rows = labelled_rows(&html);

			if let Some(author) = labelled_value(&rows, "Автор") {
				manga.authors = Some(Vec::from([String::from(author)]));
			}
			if let Some(artist) = labelled_value(&rows, "Художник") {
				manga.artists = Some(Vec::from([String::from(artist)]));
			}

			// The site writes «Завершён» with ё but is not consistent about
			// it, and matching a single spelling turned the other into
			// Unknown, so both are folded to е first. The
			// translation-continues wording has to be tested before the
			// plain completed label, which is a prefix of it.
			let status = labelled_value(&rows, "Статус").map(|value| value.replace('ё', "е"));
			manga.status = match status.as_deref() {
				Some(status) if status.starts_with("Завершен, перевод") => MangaStatus::Ongoing,
				Some("Завершен") => MangaStatus::Completed,
				Some("Заморожен") | Some("Приостановлен") => MangaStatus::Hiatus,
				Some("Продолжается") => MangaStatus::Ongoing,
				_ => MangaStatus::Unknown,
			};

			// A completed comic is deliberately NOT marked
			// `UpdateStrategy::Never` here. The line existed before this
			// source could read a status at all, so it never once ran; v7
			// made the status readable and thereby switched it on for most
			// comics, which is the one change in that version that altered
			// how the app treats an entry rather than what it displays. It
			// stays off until there is a reason to believe it is harmless.

			// The year and the publisher come first. A long run is filed as one
			// title per volume under near-identical names, and these are what
			// tell a 1963 run from a 2018 one at a glance.
			let mut tags: Vec<String> = Vec::new();
			if let Some(year) = labelled_value(&rows, "Год") {
				tags.push(String::from(year));
			}
			if let Some(publisher) = labelled_value(&rows, "Издатель") {
				tags.push(String::from(publisher));
			}

			// Genres are the catalog links carrying a `g=` segment. The same
			// genre is linked more than once on the page, so repeats are
			// dropped while the original order is kept.
			if let Some(links) = html.select("a[href*=\"/ComicList/\"]") {
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
			}

			if !tags.is_empty() {
				manga.tags = Some(tags);
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
			// Parsed here rather than above: a details-only refresh has no
			// use for this and should not fail when the chapter payload is
			// the part that is malformed.
			let data: DetailData = parse_embedded(&body)?;

			// The id leading the manga key is the same id the payload names,
			// and taking it from the key first means a payload that omits or
			// retypes `news_id` no longer costs the whole chapter list.
			let news_id = match news_id(&manga.key) {
				Some(id) => String::from(id),
				None => data
					.news_id
					.as_ref()
					.and_then(Loose::as_i64)
					.ok_or_else(|| error!("Page has no id to build chapter urls from"))?
					.to_string(),
			};

			// One entry at a time, so a chapter the payload describes oddly
			// is dropped instead of emptying the list. An entry without a
			// usable id is the only one that has to go: without it there is
			// no url to open.
			let mut rows: Vec<ChapterRow> = Vec::new();
			for value in data.chapters {
				let Ok(entry) = serde_json::from_value::<ChapterEntry>(value) else {
					continue;
				};
				let Some(id) = entry.id.as_ref().and_then(Loose::as_i64) else {
					continue;
				};
				rows.push(ChapterRow {
					id,
					posi: entry.posi.as_ref().and_then(Loose::as_f32).unwrap_or(0.0),
					volume: entry.volume.as_ref().and_then(Loose::as_f32).unwrap_or(0.0),
					number: entry.number.as_ref().and_then(Loose::as_f32),
					title: entry.title.as_ref().and_then(Loose::as_text).map(String::from),
					date: entry.date.as_ref().and_then(Loose::as_text).map(String::from),
				});
			}

			// The payload already arrives newest first, but ordering it here
			// means a change in that habit cannot quietly reverse the list.
			rows.sort_by(|a, b| b.posi.total_cmp(&a.posi));

			manga.chapters = Some(
				rows.into_iter()
					.map(|row| {
						// Some series leave `number` at zero on every entry and
						// carry the ordinal in `posi` instead, which showed up as
						// a shelf of chapters all called "Chapter 0".
						let number = row
							.number
							.filter(|number| *number > 0.0)
							.or(Some(row.posi).filter(|posi| *posi > 0.0));

						Chapter {
							key: row.id.to_string(),
							title: chapter_title(row.title.as_deref(), number),
							chapter_number: number,
							volume_number: (row.volume > 0.0).then_some(row.volume),
							date_uploaded: row
								.date
								.as_deref()
								.and_then(|date| parse_date(date, DATE_FORMAT)),
							url: Some(format!("{BASE_URL}/reader/{news_id}/{}", row.id)),
							language: Some(String::from("ru")),
							..Default::default()
						}
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
		let host = host.as_ref().and_then(Loose::as_text).unwrap_or(IMAGE_HOST);

		let pages: Vec<Page> = images
			.iter()
			.filter_map(Loose::as_text)
			.map(|path| Page {
				content: PageContent::url(image_url(host, path)),
				..Default::default()
			})
			.collect();

		if pages.is_empty() {
			bail!("Chapter has no pages");
		}

		Ok(pages)
	}
}

impl ListingProvider for ComX {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		// The catalog's own order is by date, newest first. The site changes
		// it through a form post that stores the choice in the session rather
		// than through the URL, so the listings here select sections instead.
		let filters: Vec<FilterValue> = match listing.id.as_str() {
			"comics" => sections(COMIC_SECTIONS),
			"marvel" => sections(&["2"]),
			"dc" => sections(&["14"]),
			"manga" => sections(&["3"]),
			"manhwa" => sections(&["44"]),
			"manhua" => sections(&["45"]),
			_ => Vec::new(),
		};

		self.fetch_catalog(&filters, page)
	}
}

/// Builds the section filter a catalog URL is assembled from.
fn sections(ids: &[&str]) -> Vec<FilterValue> {
	Vec::from([FilterValue::MultiSelect {
		id: String::from("p.cat"),
		included: ids.iter().map(|id| String::from(*id)).collect(),
		excluded: Vec::new(),
	}])
}

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

fn genre_chips() -> Vec<FilterItem> {
	HOME_GENRES
		.iter()
		.map(|(id, name)| FilterItem {
			title: String::from(*name),
			values: Some(Vec::from([FilterValue::MultiSelect {
				id: String::from("g"),
				included: Vec::from([String::from(*id)]),
				excluded: Vec::new(),
			}])),
		})
		.collect()
}

impl Home for ComX {
	fn get_home(&self) -> Result<HomeLayout> {
		let latest = self.home_section(&[])?;
		let mut components = Vec::new();

		// The newest few are the hero, and the remainder of that same request
		// fills the row beneath it, so the two together cost one fetch.
		let featured: Vec<Manga> = latest.iter().take(FEATURED_COUNT).cloned().collect();
		if !featured.is_empty() {
			components.push(HomeComponent {
				value: HomeComponentValue::BigScroller {
					entries: featured,
					auto_scroll_interval: Some(8.0),
				},
				..Default::default()
			});
		}

		if latest.len() > FEATURED_COUNT {
			components.push(HomeComponent {
				title: Some(String::from("Обновления")),
				value: HomeComponentValue::Scroller {
					entries: manga_links(latest[FEATURED_COUNT..].to_vec()),
					listing: Some(listing("latest", "Обновления")),
				},
				..Default::default()
			});
		}

		// Chips cost nothing: they are links, not results.
		components.push(HomeComponent {
			title: Some(String::from("Жанры")),
			value: HomeComponentValue::Filters(genre_chips()),
			..Default::default()
		});

		// Each of these is another trip through the challenge, so the page
		// stops at two, and a section that fails is left out rather than
		// taking the whole home down with it.
		for (title, id, ids) in [
			("Комиксы", "comics", COMIC_SECTIONS),
			("Манхва", "manhwa", &["44"] as &[&str]),
		] {
			let entries = self.home_section(&sections(ids)).unwrap_or_default();
			if entries.is_empty() {
				continue;
			}
			components.push(HomeComponent {
				title: Some(String::from(title)),
				value: HomeComponentValue::Scroller {
					entries: manga_links(entries),
					listing: Some(listing(id, title)),
				},
				..Default::default()
			});
		}

		Ok(HomeLayout { components })
	}
}

impl ImageRequestProvider for ComX {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		// Aidoku asks for covers, thumbnails and pages with no headers at
		// all. The image hosts answer a request carrying no `Referer` with
		// an intermittent 403 — a 1.5KB HTML body in place of the image —
		// which is what leaves a grid with some covers missing and others
		// present, differing from one load to the next. The same request
		// with a `Referer` was served normally.
		//
		// Only the one header: a user agent on its own made no difference,
		// and the cookies are not needed to be served an image.
		Ok(Request::get(url)?.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for ComX {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		// A shared link arrives over either scheme and with or without
		// `www.`; only the canonical spelling used to be recognised, so
		// every other form opened nothing at all.
		let rest = url
			.strip_prefix("https://")
			.or_else(|| url.strip_prefix("http://"))
			.unwrap_or(url.as_str());
		let rest = rest.strip_prefix("www.").unwrap_or(rest);
		let Some(path) = rest.strip_prefix("com-x.life") else {
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
	fn handle_web_login(&self, _key: String, cookies: HashMap<String, String>) -> Result<bool> {
		let mut header = String::new();
		for (name, value) in cookies.iter() {
			if !header.is_empty() {
				header.push_str("; ");
			}
			header.push_str(name);
			header.push('=');
			header.push_str(value);
		}

		if !header.is_empty() {
			// Whatever the reader opened this sheet to do, it is a visible web
			// view that has just cleared the challenge, so its cookies are the
			// freshest grant the source is going to get. Storing them as the
			// grant as well is what makes the advice in the fetch error true:
			// opening this sheet refreshes the grant even if nobody signs in.
			defaults_set(COOKIE_KEY, DefaultValue::String(header.clone()));
			// These are the whole point of the exercise. Dropping them leaves
			// the source requesting every page as a guest, which is what makes
			// a login that plainly succeeded look as though it never happened.
			defaults_set(LOGIN_COOKIE_KEY, DefaultValue::String(header));
		}

		// The session cookie's name is not documented, so rather than guess at
		// one, ask the site: every catalog page states the signed-in state in
		// the filter payload it embeds. This runs through the ordinary fetch,
		// so it is answered with the cookies just stored and reports what the
		// source will actually get from here on rather than what the web view
		// alone can see.
		let Ok(body) = fetch(&format!("{BASE_URL}/ComicList/"), LIST_MARKER) else {
			// The question could not be put. The cookies above are already
			// stored, and throwing them away over one failed request would
			// undo a sign-in that may well have worked.
			return Ok(false);
		};
		let logged_in = body.contains("\"is_logged\":true");

		// Not signed in means either that the login did not take or that this
		// call is a logout; which of the two it is cannot be told apart here,
		// and in both cases the login cookies are not worth sending again.
		// The grant stored above is, so it stays.
		if !logged_in {
			defaults_set(LOGIN_COOKIE_KEY, DefaultValue::String(String::new()));
		}

		Ok(logged_in)
	}
}

register_source!(
	ComX,
	ListingProvider,
	DeepLinkHandler,
	WebLoginHandler,
	Home,
	ImageRequestProvider
);
