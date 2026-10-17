#![no_std]
//! Aidoku source for the Grouple network's manga library — `readmanga.me`,
//! mirrored at `a.zazaza.me`.
//!
//! Three things about this site decide how the source is written.
//!
//! The first is that every request needs a browser `User-Agent`. Without one
//! the edge answers `402` with a 24-byte `NOT FOUND IN THE ADDRESS` body, to
//! the API and the pages alike, which reads like a hard block and is only a
//! filter on non-browser clients. Nothing else is needed: no cookies, no
//! `Referer`, no sign-in.
//!
//! The second is that the catalogue is a JSON API, `/api/catalog/search`,
//! even though the pages around it are server-rendered HTML. It is the same
//! endpoint the site's own filter sheet calls, it pages with `offset`, and
//! it reports a real `total`. Titles and chapters still come from the HTML,
//! because the site exposes no detail endpoint to ask instead.
//!
//! The third is the one that kept this source from existing. Page images are
//! declared in the reader's own payload with a signed query —
//! `?t=…&u=…&h=…` — and fetching one *with* that query returns `300` and a
//! 77-byte body holding a decoy url pointing at an unrelated image on
//! Wikimedia. The browser is served the real image because it never sends
//! the signature: the reader strips the query before assigning `img.src`.
//! Dropping everything from the `?` turns the same url into a `200` and the
//! real page, with no headers at all. See `page_urls`.

use aidoku::{
	AidokuError, Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue,
	Home, HomeComponent, HomeComponentValue, HomeLayout, ImageRequestProvider, Link, LinkValue,
	Listing, ListingKind, ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent,
	PageContext, Result, Source, UpdateStrategy, Viewer,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{
		defaults::defaults_get,
		html::{Document, Html},
		net::Request,
		std::parse_date,
	},
	prelude::*,
};
use serde::Deserialize;

/// The canonical domain. `a.zazaza.me` serves the same library — the two
/// agree on all but a couple of the 31,000 titles — and is offered through
/// the app's base-url picker for connections that prefer it.
const DEFAULT_URL: &str = "https://1.selfmanga.live";

/// Defaults key the app writes the chosen base url into.
const BASE_URL_KEY: &str = "url";

/// Any browser string will do; what the edge rejects is the absence of one.
const USER_AGENT: &str =
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
	 Chrome/131.0.0.0 Safari/537.36";

/// Fixed by the API: it answers 50 rows whatever is asked of it.
const PAGE_SIZE: i32 = 50;

/// The bounds the site's own year slider carries. A range filter with only
/// one end set is completed with these, because the API takes the pair as a
/// single `min,max` parameter and cannot be given half of one.
const YEAR_MIN: i32 = 2007;
const YEAR_MAX: i32 = 2026;

/// How many of the top titles get the large treatment on the home page.
const FEATURED_COUNT: usize = 5;

/// `2024-09-28 18:53:22.568` — the fractional part is trimmed before
/// parsing, so only the stable prefix is described here.
const DATE_FORMAT: &str = "yyyy-MM-dd HH:mm:ss";

/// A readable slice of the genre list, shown as chips on the home page. The
/// ids and the spelling are the site's own.
const HOME_GENRES: &[(&str, &str)] = &[
	("2155", "боевик"),
	("2131", "фэнтези"),
	("2121", "романтика"),
	("2118", "драма"),
	("2136", "комедия"),
	("2130", "приключения"),
	("2159", "сверхъестественное"),
	("2125", "ужасы"),
	("2152", "детектив"),
	("2127", "школа"),
	("2133", "научная фантастика"),
	("2135", "повседневность"),
];

fn base_url() -> String {
	defaults_get::<String>(BASE_URL_KEY)
		.filter(|value| !value.is_empty())
		.unwrap_or_else(|| String::from(DEFAULT_URL))
}

// MARK: - API models

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElementId {
	link_name: Option<String>,
}

/// One classifying element: a genre, a tag, the category, the year, the age
/// limitation, a publisher. They share one id space, which is what lets a
/// single query parameter carry any mixture of them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiElement {
	id: i64,
	type_name: Option<String>,
	name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiItem {
	element_id: ElementId,
	category: Option<ApiElement>,
	name: Option<String>,
	pic_url: Option<String>,
	elements: Option<Vec<ApiElement>>,
	production_status: Option<String>,
}

#[derive(Deserialize)]
struct CatalogPage {
	total: Option<i32>,
	list: Option<Vec<ApiItem>>,
}

// MARK: - Helpers

fn non_empty(value: Option<String>) -> Option<String> {
	value.filter(|s| !s.trim().is_empty())
}

fn trimmed(value: Option<String>) -> Option<String> {
	non_empty(value).map(|s| String::from(s.trim()))
}

fn parse_timestamp(value: Option<String>) -> Option<i64> {
	let value = non_empty(value)?;
	parse_date(value.get(..19)?, DATE_FORMAT)
}

/// Maps the site's production statuses, as its own filter vocabulary names
/// them. `NON_FINISHED` is the catch-all the catalogue uses for anything
/// still open, so it reads as ongoing rather than unknown.
fn parse_status(status: Option<&str>) -> MangaStatus {
	match status {
		Some("FINISHED") => MangaStatus::Completed,
		Some("PROGRESS") | Some("NON_FINISHED") => MangaStatus::Ongoing,
		Some("POSTPONED") => MangaStatus::Hiatus,
		Some("CANCELED") => MangaStatus::Cancelled,
		// PLANNED is a title announced but not yet drawn.
		_ => MangaStatus::Unknown,
	}
}

/// Maps the category element onto a reading direction. The ids are
/// this site's own: the sibling libraries number the same formats
/// differently, so none of them can be shared.
fn parse_viewer(category: Option<i64>) -> Viewer {
	match category {
		// Manga, yonkoma and doujinshi read right to left.
		// 6010 Манга, 2141 Додзинси.
		Some(2141) | Some(6010) => Viewer::RightToLeft,
		// Manhwa, manhua and webtoons are long-strip.
		// 2160 Вебтун.
		Some(2160) => Viewer::Webtoon,
		// Comics and OEL manga read the western way.
		// 6011 Комикс.
		Some(6011) => Viewer::LeftToRight,
		// Арт, Журнал, Сборник carries no direction of its own.
		_ => Viewer::Unknown,
	}
}

/// This site publishes no age-rating vocabulary at all — its filter
/// sheet has no such control and its titles carry no `limitation`
/// element — so there is nothing to read a rating from, and saying
/// `Unknown` is more honest than guessing `Safe`.
fn parse_content_rating(_limitation: Option<i64>) -> ContentRating {
	ContentRating::Unknown
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
				id: String::from("genre"),
				included: Vec::from([String::from(*id)]),
				excluded: Vec::new(),
			}])),
		})
		.collect()
}

impl ApiItem {
	/// Genres and tags, in the site's own spelling.
	///
	/// Publishers, journals, the year and the age limitation are elements
	/// too and are left out: they classify the release rather than the work,
	/// and a tag pressed in the app is matched against the filter sheet,
	/// which only carries the ones below.
	fn tags(&self) -> Option<Vec<String>> {
		let mut tags: Vec<String> = Vec::new();
		for element in self.elements.iter().flatten() {
			let keep = matches!(
				element.type_name.as_deref(),
				Some("genre") | Some("tag") | Some("another")
			);
			let Some(name) = element.name.clone().filter(|_| keep) else {
				continue;
			};
			if !tags.contains(&name) {
				tags.push(name);
			}
		}
		(!tags.is_empty()).then_some(tags)
	}

	fn element_of(&self, type_name: &str) -> Option<i64> {
		self.elements
			.iter()
			.flatten()
			.find(|element| element.type_name.as_deref() == Some(type_name))
			.map(|element| element.id)
	}

	fn into_manga(self) -> Option<Manga> {
		let key = non_empty(self.element_id.link_name.clone())?;
		let tags = self.tags();
		let limitation = self.element_of("limitation");

		Some(Manga {
			url: Some(format!("{}/{key}", base_url())),
			key,
			title: non_empty(self.name.clone()).unwrap_or_default(),
			cover: non_empty(self.pic_url.clone()),
			artists: None,
			authors: None,
			description: None,
			tags,
			status: parse_status(self.production_status.as_deref()),
			content_rating: parse_content_rating(limitation),
			viewer: parse_viewer(self.category.as_ref().map(|c| c.id)),
			update_strategy: UpdateStrategy::Always,
			next_update_time: None,
			chapters: None,
		})
	}
}

// MARK: - Source

struct SelfManga;

impl SelfManga {
	/// Every request goes through here. The user agent is the whole of what
	/// the site asks for, and leaving it off is the `402` described above.
	fn request(url: String) -> Result<Request> {
		Ok(Request::get(url)?.header("User-Agent", USER_AGENT))
	}

	fn fetch_document(path: &str) -> Result<Document> {
		let body = Self::request(format!("{}/{path}", base_url()))?.string()?;
		Ok(Html::parse(body)?)
	}

	/// Runs a catalogue query. `total` is reported honestly, so whether a
	/// further page exists is arithmetic rather than a guess.
	fn catalog(query: QueryParameters, offset: i32) -> Result<MangaPageResult> {
		let url = format!("{}/api/catalog/search?{query}", base_url());
		let page: CatalogPage = Self::request(url)?.json_owned()?;

		let entries: Vec<Manga> = page
			.list
			.unwrap_or_default()
			.into_iter()
			.filter_map(ApiItem::into_manga)
			.collect();

		Ok(MangaPageResult {
			has_next_page: match page.total {
				Some(total) => offset + (entries.len() as i32) < total,
				None => entries.len() as i32 >= PAGE_SIZE,
			},
			entries,
		})
	}

	fn offset_of(page: i32) -> i32 {
		(page.max(1) - 1) * PAGE_SIZE
	}

	fn base_query(offset: i32) -> QueryParameters {
		let mut query = QueryParameters::new();
		query.push("offset", Some(&offset.to_string()));
		query
	}

	/// Maps the index of the sort filter onto the API's values, in the same
	/// order the options appear in `filters.json`. An unknown value is
	/// silently read as `RATING` rather than rejected, so the fallback here
	/// matches what the site would do with it anyway.
	fn sort_field(index: i32) -> &'static str {
		match index {
			1 => "POPULARITY",
			2 => "USER_RATING",
			3 => "DATE_UPDATE",
			4 => "DATE_CREATE",
			5 => "YEAR",
			6 => "NAME",
			_ => "RATING",
		}
	}

	/// The pair of parameters a multi-select writes into.
	///
	/// Genres, categories, features and age ratings are all elements sharing
	/// one id space, so they all go through `includeElementIds`; repeating
	/// it intersects. The statuses and the site's own canned filters have
	/// parameters of their own.
	fn params_for(id: &str) -> (&'static str, &'static str) {
		match id {
			"productionStatus" => ("includeProductionStatuses", "excludeProductionStatuses"),
			"translationStatus" => ("includeTranslationStatuses", "excludeTranslationStatuses"),
			"searchFilters" => ("includeSearchFilter", "excludeSearchFilter"),
			_ => ("includeElementIds", "excludeElementIds"),
		}
	}

	fn push_filters(query: &mut QueryParameters, filters: Vec<FilterValue>) {
		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => {
					// The API takes a field and no direction.
					query.push("sortType", Some(Self::sort_field(index)));
				}
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} => {
					let (include, exclude) = Self::params_for(&id);
					for value in included {
						query.push(include, Some(&value));
					}
					for value in excluded {
						query.push(exclude, Some(&value));
					}
				}
				// One parameter holds both ends, so an open end is filled
				// with the site's own bound rather than left out.
				FilterValue::Range { id, from, to } if id == "years" => {
					if from.is_none() && to.is_none() {
						continue;
					}
					let from = from.map(|v| v as i32).unwrap_or(YEAR_MIN);
					let to = to.map(|v| v as i32).unwrap_or(YEAR_MAX);
					query.push("years", Some(&format!("{from},{to}")));
				}
				_ => {}
			}
		}
	}

	fn fetch_items(query: &QueryParameters) -> Result<Vec<Manga>> {
		let url = format!("{}/api/catalog/search?{query}", base_url());
		let page: CatalogPage = Self::request(url)?.json_owned()?;
		Ok(page
			.list
			.unwrap_or_default()
			.into_iter()
			.filter_map(ApiItem::into_manga)
			.collect())
	}

	fn sorted_page(field: &str, offset: i32) -> Result<MangaPageResult> {
		let mut query = Self::base_query(offset);
		query.push("sortType", Some(field));
		Self::catalog(query, offset)
	}

	/// Reads the parts of a title that only the HTML page carries.
	///
	/// The catalogue already supplies the cover, the tags, the status and
	/// the reading direction, so this fills in what it cannot: the
	/// description, the credits and the alternative names.
	fn parse_details(html: &Document, manga: &mut Manga) {
		if let Some(title) = html
			.select_first("h1.names .name")
			.and_then(|node| node.text())
			.and_then(|text| non_empty(Some(text)))
		{
			manga.title = String::from(title.trim());
		}

		if let Some(cover) = html.select_first("img.cr-hero-poster__img").and_then(|img| {
			img.attr("src")
				.or_else(|| img.attr("data-src"))
				.filter(|url| !url.is_empty())
		}) {
			manga.cover = Some(cover);
		}

		if let Some(description) = html
			.select_first(".cr-description__content")
			.and_then(|node| node.text())
			.and_then(|text| trimmed(Some(text)))
		{
			manga.description = Some(description);
		}

		let mut authors: Vec<String> = Vec::new();
		let mut artists: Vec<String> = Vec::new();
		let Some(items) = html.select(".cr-main-person-item") else {
			return;
		};
		for item in items {
			let role = item
				.select_first(".cr-main-person-item__role")
				.and_then(|node| node.text())
				.unwrap_or_default();
			// «Сценаристы» are the writers and «Художники» the artists;
			// the rest of the roles are translators and publishers, which
			// belong on the chapters rather than on the title.
			let into = if role.contains("Сценарист") || role.contains("Автор") {
				&mut authors
			} else if role.contains("Художник") {
				&mut artists
			} else {
				continue;
			};
			let Some(names) = item.select(".cr-main-person-item__name a") else {
				continue;
			};
			for name in names {
				let Some(name) = name.text().and_then(|text| trimmed(Some(text))) else {
					continue;
				};
				if !into.contains(&name) {
					into.push(name);
				}
			}
		}
		if !authors.is_empty() {
			manga.authors = Some(authors);
		}
		if !artists.is_empty() {
			manga.artists = Some(artists);
		}

		let related = Self::parse_related(html);
		if !related.is_empty() {
			let mut note = String::from("Связанное: ");
			for (index, name) in related.iter().enumerate() {
				if index > 0 {
					note.push_str(" · ");
				}
				note.push_str(name);
			}
			manga.description = Some(match manga.description.take() {
				Some(text) if !text.trim().is_empty() => format!("{text}

{note}"),
				_ => note,
			});
		}
	}

	/// Names the titles the page files alongside this one.
	///
	/// This is the volume navigation a long run otherwise lacks: the block
	/// names the prequel, the spin-off and the colour edition, each with its
	/// relation spelled out. Aidoku's `Manga` has no field for related
	/// titles, so the description is the only place they can be shown.
	///
	/// Two things have to be filtered out. The carousel renders the same
	/// entries several times over for its breakpoints, so names repeat; and
	/// many entries link to the network's anime, drama and novel sites
	/// rather than to manga, which a manga reader cannot open. Only
	/// same-site links survive, deduplicated by name.
	fn parse_related(html: &Document) -> Vec<String> {
		let mut names: Vec<String> = Vec::new();
		let Some(items) = html.select(".related-item") else {
			return names;
		};

		for item in items {
			let Some(link) = item.select_first("a.inline-tile__title") else {
				continue;
			};
			// A cross-site entry points at the network's anime, drama or
			// novel sites, which a manga reader cannot open.
			if !link.attr("href").is_some_and(|href| href.starts_with('/')) {
				continue;
			}
			let Some(title) = link.text().and_then(|text| trimmed(Some(text))) else {
				continue;
			};

			let entry = match item
				.select_first(".related-item__type")
				.and_then(|node| node.text())
				.and_then(|text| trimmed(Some(text)))
			{
				Some(relation) => format!("{title} ({relation})"),
				None => title,
			};
			if !names.contains(&entry) {
				names.push(entry);
			}
		}

		names
	}

	/// Builds the chapter list from the title page's table.
	///
	/// Rows arrive newest first, which is the order Aidoku shows them in.
	/// The row's own `data-num` is the chapter number times ten, so the
	/// number is taken from the link instead, where it is written plainly
	/// and keeps halves like `99.5` intact.
	fn parse_chapters(html: &Document) -> Vec<Chapter> {
		let mut chapters: Vec<Chapter> = Vec::new();

		let Some(rows) = html.select("tr.item-row") else {
			return chapters;
		};
		for row in rows {
			let Some(link) = row.select_first("td.item-title a.chapter-link") else {
				continue;
			};
			let Some(href) = link.attr("href").filter(|href| href.contains("/vol")) else {
				continue;
			};

			// `/slug/vol30/271` — the last two segments are the key, which
			// keeps working if the title's own slug is ever rewritten.
			let mut segments = href.rsplit('/');
			let Some(number) = segments.next().filter(|s| !s.is_empty()) else {
				continue;
			};
			let Some(volume) = segments.next().filter(|s| s.starts_with("vol")) else {
				continue;
			};

			let chapter_number = number.parse::<f32>().ok();
			let volume_number = volume.trim_start_matches("vol").parse::<f32>().ok();

			// The link reads «30 - 271 Конец мечты»: the leading numbers
			// repeat the volume and chapter, and a name may follow them.
			let text = link.text().unwrap_or_default();
			let title = non_empty(Some(String::from(
				text.trim_matches(|c: char| {
					c.is_ascii_digit() || c.is_whitespace() || c == '-' || c == '.'
				}),
			)));

			let scanlators = link
				.attr("title")
				.and_then(|name| trimmed(Some(name)))
				.map(|name| Vec::from([name]));

			let date_uploaded = row
				.select_first("td.date")
				.and_then(|cell| cell.attr("data-date-raw"))
				.and_then(|raw| parse_timestamp(Some(raw)));

			chapters.push(Chapter {
				key: format!("{volume}/{number}"),
				title,
				chapter_number,
				volume_number,
				date_uploaded,
				scanlators,
				url: Some(format!("{}{href}", base_url())),
				language: Some(String::from("ru")),
				thumbnail: None,
				locked: false,
			});
		}

		chapters
	}

	/// Pulls the page urls out of the reader's own payload.
	///
	/// The reader is initialised with `rm_h.readerInit(chapterInfo, [[host,
	/// dir, "path?t=…&u=…&h=…", width, height, ''], …])`. That signed query
	/// is the trap this source exists to step around: sent back to the image
	/// host it answers `300` with a 77-byte decoy url, while the same path
	/// without it answers `200` with the real page — which is exactly what
	/// the site's own reader does, since it strips the query before setting
	/// `img.src`. Verified across three titles: the images that come back
	/// match the dimensions the payload declares.
	fn page_urls(body: &str) -> Vec<String> {
		let Some(start) = body.find("readerInit(") else {
			return Vec::new();
		};
		let tail = &body[start..];
		let data = match tail.find(");") {
			Some(end) => &tail[..end],
			None => tail,
		};

		let mut urls: Vec<String> = Vec::new();
		// Entries open with `['`, which the leading `chapterInfo` object
		// cannot contain, so the first chunk is dropped and the rest are
		// one entry each.
		for entry in data.split("['").skip(1) {
			let Some((host, rest)) = entry.split_once('\'') else {
				continue;
			};
			// The directory sits between the host and the path and is
			// usually empty; it is read rather than assumed.
			let Some(rest) = rest.split_once('\'').map(|(_, rest)| rest) else {
				continue;
			};
			let Some((dir, rest)) = rest.split_once('\'') else {
				continue;
			};
			let Some(rest) = rest.split_once('"').map(|(_, rest)| rest) else {
				continue;
			};
			let Some((path, _)) = rest.split_once('"') else {
				continue;
			};

			let path = path.split('?').next().unwrap_or(path);
			if path.is_empty() {
				continue;
			}
			// A chapter whose pages were taken down declares no host and
			// points at the site's own `/static/deleted1.png`, which the
			// site serves. Resolving it against the base url shows that
			// placeholder instead of a relative url nothing can fetch.
			if host.is_empty() {
				urls.push(format!("{}{dir}{path}", base_url()));
			} else {
				urls.push(format!("{host}{dir}{path}"));
			}
		}

		urls
	}
}

impl Source for SelfManga {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let offset = Self::offset_of(page);
		let mut params = Self::base_query(offset);
		// One endpoint serves both, and a query narrows the filtered set
		// rather than replacing it, so the filter sheet stays usable while
		// searching — `hidesFiltersWhileSearching` is off for that reason.
		if let Some(query) = non_empty(query) {
			params.push("q", Some(&query));
		}
		Self::push_filters(&mut params, filters);
		Self::catalog(params, offset)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if !needs_details && !needs_chapters {
			return Ok(manga);
		}

		// The details and the chapter table are on the same page, so one
		// request serves both halves.
		let html = Self::fetch_document(&manga.key)?;

		if needs_details {
			Self::parse_details(&html, &mut manga);
		}
		if needs_chapters {
			manga.chapters = Some(Self::parse_chapters(&html));
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// `mtr=1` is the site's own age gate being answered. Without it a
		// chapter of any of the 1,232 titles flagged mature renders the
		// warning instead of the reader, and the payload this parses is
		// simply absent — the title page is unaffected, so only the reader
		// asks for it. On a title that is not gated it changes nothing.
		let url = format!("{}/{}/{}?mtr=1", base_url(), manga.key, chapter.key);
		let body = Self::request(url)?.string()?;

		let pages: Vec<Page> = Self::page_urls(&body)
			.into_iter()
			.map(|url| Page {
				content: PageContent::url(url),
				thumbnail: None,
				has_description: false,
				description: None,
			})
			.collect();

		if pages.is_empty() {
			// A licensed title keeps its chapter list and loses its pages,
			// which is the one way a chapter that opened fine yesterday
			// comes back empty.
			bail!("This chapter has no pages — the title may have been licensed");
		}

		Ok(pages)
	}
}

impl ListingProvider for SelfManga {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let field = match listing.id.as_str() {
			"popular" => "RATING",
			"trending" => "POPULARITY",
			"latest" => "DATE_UPDATE",
			"rating" => "USER_RATING",
			"new" => "DATE_CREATE",
			_ => return Err(AidokuError::Unimplemented),
		};

		Self::sorted_page(field, Self::offset_of(page))
	}
}

impl Home for SelfManga {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components = Vec::new();

		// The best-rated titles are the hero, and the rest of that same
		// response fills the row beneath it, so the two cost one request.
		let mut params = Self::base_query(0);
		params.push("sortType", Some("RATING"));
		let popular = Self::fetch_items(&params)?;

		if !popular.is_empty() {
			let featured: Vec<Manga> = popular.iter().take(FEATURED_COUNT).cloned().collect();
			components.push(HomeComponent {
				value: HomeComponentValue::BigScroller {
					entries: featured,
					auto_scroll_interval: Some(8.0),
				},
				..Default::default()
			});

			if popular.len() > FEATURED_COUNT {
				components.push(HomeComponent {
					title: Some(String::from("Популярное")),
					value: HomeComponentValue::Scroller {
						entries: manga_links(popular[FEATURED_COUNT..].to_vec()),
						listing: Some(listing("popular", "Популярное")),
					},
					..Default::default()
				});
			}
		}

		// Chips cost nothing: they are links, not results.
		components.push(HomeComponent {
			title: Some(String::from("Жанры")),
			value: HomeComponentValue::Filters(genre_chips()),
			..Default::default()
		});

		// A row that fails is left out rather than taking the whole page down.
		for (title, id, field) in [
			("Читают сейчас", "trending", "POPULARITY"),
			("Обновления", "latest", "DATE_UPDATE"),
			("Лучшее по оценке", "rating", "USER_RATING"),
			("Новинки", "new", "DATE_CREATE"),
		] {
			let mut params = Self::base_query(0);
			params.push("sortType", Some(field));

			let Ok(entries) = Self::fetch_items(&params) else {
				continue;
			};
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

impl ImageRequestProvider for SelfManga {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		// One of the three image hosts answers a request carrying no
		// `Referer` with a 403 and an HTML body — covers included, which
		// is what would leave the grid empty. The other two do not care,
		// so the one header is sent everywhere rather than guessed at.
		Ok(Request::get(url)?.header("Referer", &format!("{}/", base_url())))
	}
}

impl DeepLinkHandler for SelfManga {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		// Either domain can turn up in a shared link, whichever the reader
		// happens to be browsing with.
		let path = ["https://1.selfmanga.live", &base_url()]
			.into_iter()
			.find_map(|host| url.strip_prefix(host));
		let Some(path) = path else {
			return Ok(None);
		};

		// A title is `/{slug}` and a chapter `/{slug}/vol{N}/{M}`; the query
		// a reader link carries names a translation and is not part of
		// either key.
		let path = path.split('?').next().unwrap_or(path);
		let mut segments = path.trim_matches('/').split('/');
		let Some(slug) = segments.next().filter(|s| !s.is_empty()) else {
			return Ok(None);
		};
		// These are the site's own sections rather than titles.
		if matches!(slug, "list" | "search" | "internal" | "user" | "login") {
			return Ok(None);
		}

		let Some(volume) = segments.next().filter(|s| s.starts_with("vol")) else {
			return Ok(Some(DeepLinkResult::Manga {
				key: String::from(slug),
			}));
		};
		let Some(number) = segments.next().filter(|s| !s.is_empty()) else {
			return Ok(Some(DeepLinkResult::Manga {
				key: String::from(slug),
			}));
		};

		Ok(Some(DeepLinkResult::Chapter {
			manga_key: String::from(slug),
			key: format!("{volume}/{number}"),
		}))
	}
}

register_source!(
	SelfManga,
	ListingProvider,
	DeepLinkHandler,
	Home,
	ImageRequestProvider
);
