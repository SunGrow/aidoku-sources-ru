#![no_std]
//! Aidoku source for remanga.org.
//!
//! Everything here talks to the site's own JSON API, which needs no headers,
//! no cookies and no sign-in to browse.
//!
//! Two things about that API are worth knowing before changing anything.
//!
//! The first is that there are two generations of it living side by side, and
//! only the newer one is correct. `/api/search/catalog/` still answers, but it
//! drops every row the moment a `status` is named — `status=2` returns zero
//! items even for titles that plainly carry that status — and it unions
//! repeated genres where the site itself intersects them. `/api/v2/...` is
//! what the site's own pages call and is what this source uses throughout.
//!
//! The second is that the main domain is geo-blocked. `remanga.org` answers
//! 403 outside the CIS, while the punycode mirror below serves the full API to
//! a bare request. The mirror is therefore the default, with the main domain
//! offered as the alternative through the app's base-url picker.

use aidoku::{
	AidokuError, Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue,
	Home, HomeComponent, HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingKind,
	ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source,
	UpdateStrategy, Viewer,
	alloc::{String, Vec, string::ToString},
	helpers::uri::{QueryParameters, encode_uri_component},
	imports::{defaults::defaults_get, net::Request, std::parse_date},
	prelude::*,
};
use serde::Deserialize;

/// The punycode form of `реманга.орг`.
///
/// This is the default rather than `remanga.org` because the latter answers
/// 403 to anyone outside the CIS, where the mirror serves the same API
/// unauthenticated. Both are listed in `source.json`, so a reader whose
/// connection prefers the other one can switch without a new build.
const DEFAULT_URL: &str = "https://xn--80aaig9ahr.xn--c1avg";

/// Defaults key the app writes the chosen base url into.
///
/// Set by `allowsBaseUrlSelect` in the manifest; empty until the reader picks
/// one, which is why every read falls back to the default.
const BASE_URL_KEY: &str = "url";

/// Catalog and search reject anything above this outright, with a validation
/// error rather than a clamp.
const PAGE_LIMIT: i32 = 30;

/// The chapter endpoint has its own, larger ceiling, and it *does* clamp: ask
/// for 1000 and it quietly answers with 50. Asking for exactly 50 keeps the
/// request count down on long runs without relying on that silence.
const CHAPTER_LIMIT: i32 = 50;

/// How many of the top titles get the large treatment on the home page.
const FEATURED_COUNT: usize = 5;

/// How many similar titles to name in a description. The endpoint returns
/// twenty, ranked, and the tail of that is only loosely related.
const SIMILAR_COUNT: usize = 8;

/// `2023-11-08T11:37:28.977968` — the fractional part is trimmed before
/// parsing, so only the stable prefix is described here.
const DATE_FORMAT: &str = "yyyy-MM-dd'T'HH:mm:ss";

/// A readable slice of the genre list, shown as chips on the home page.
const HOME_GENRES: &[(&str, &str)] = &[
	("2", "Экшен"),
	("38", "Фэнтези"),
	("25", "Романтика"),
	("11", "Драма"),
	("50", "Комедия"),
	("23", "Приключения"),
	("3", "Боевые искусства"),
	("27", "Сверхъестественное"),
	("36", "Ужасы"),
	("8", "Детектив"),
	("39", "Школьники"),
	("20", "Научная фантастика"),
];

fn base_url() -> String {
	defaults_get::<String>(BASE_URL_KEY)
		.filter(|value| !value.is_empty())
		.unwrap_or_else(|| String::from(DEFAULT_URL))
}

// MARK: - API models

#[derive(Deserialize)]
struct Cover {
	high: Option<String>,
	mid: Option<String>,
	low: Option<String>,
}

impl Cover {
	/// The three sizes are the same image, so the largest is always worth
	/// taking; the smaller ones only matter when a title is missing it.
	fn best(&self) -> Option<&String> {
		self.high
			.as_ref()
			.or(self.mid.as_ref())
			.or(self.low.as_ref())
	}
}

#[derive(Deserialize)]
struct NamedId {
	id: i32,
	name: String,
}

/// A person or studio credited on a title.
///
/// `type` is an unlabelled integer and the API exposes no vocabulary for it,
/// so the mapping was read off titles whose credits are known independently:
/// on *The Breaker* the writer is type 1 and the artist type 4. Sampling the
/// rest showed type 2 to be art studios (REDICE studio, LICO) and type 3 to be
/// publishers and platforms (Naver, Webtoon, Bilibili Comics) rather than
/// people, so only 1, 2 and 4 become authors and artists.
#[derive(Deserialize)]
struct Creator {
	name: String,
	#[serde(rename = "type")]
	kind: Option<i32>,
}

/// One translation team's run of a title.
///
/// Chapters hang off a branch rather than the title, so one has to be chosen
/// before they can be listed at all.
#[derive(Deserialize)]
struct Branch {
	id: i64,
	count_chapters: Option<i32>,
}

#[derive(Deserialize)]
struct ApiTitle {
	dir: String,
	main_name: Option<String>,
	secondary_name: Option<String>,
	description: Option<String>,
	cover: Option<Cover>,
	#[serde(rename = "type")]
	kind: Option<NamedId>,
	status: Option<NamedId>,
	age_limit: Option<NamedId>,
	genres: Option<Vec<NamedId>>,
	categories: Option<Vec<NamedId>>,
	creators: Option<Vec<Creator>>,
	branches: Option<Vec<Branch>>,
	is_erotic: Option<bool>,
	is_yaoi: Option<bool>,
}

/// One row of the similar-titles endpoint, which wraps the title in a
/// relevance score rather than returning it bare.
#[derive(Deserialize)]
struct SimilarEntry {
	title: Option<ApiTitle>,
}

/// The catalog and chapter endpoints page with a `next` link rather than a
/// total, which is all `has_next_page` needs.
#[derive(Deserialize)]
struct CursorPage<T> {
	next: Option<String>,
	results: Option<Vec<T>>,
}

#[derive(Deserialize)]
struct SearchMeta {
	total_pages: Option<i32>,
	page: Option<i32>,
}

/// Search pages differently from the catalog, with a count instead of a
/// cursor.
#[derive(Deserialize)]
struct SearchPage {
	meta: Option<SearchMeta>,
	results: Option<Vec<ApiTitle>>,
}

#[derive(Deserialize)]
struct ApiChapter {
	id: i64,
	tome: Option<f32>,
	chapter: Option<String>,
	name: Option<String>,
	upload_date: Option<String>,
	is_paid: Option<bool>,
	is_bought: Option<bool>,
	publishers: Option<Vec<NamedId>>,
}

#[derive(Deserialize)]
struct PageImage {
	link: Option<String>,
}

/// `pages` is a list of *slides*, each holding several images, rather than a
/// flat list of images, so it has to be flattened before it means anything.
#[derive(Deserialize)]
struct ChapterDetail {
	pages: Option<Vec<Vec<PageImage>>>,
}

// MARK: - Helpers

fn non_empty(value: Option<String>) -> Option<String> {
	value.filter(|s| !s.trim().is_empty())
}

/// Resolves the `/media/...` paths the API returns into absolute urls. Page
/// images already come back absolute, pointing at a separate image host.
fn absolute_url(path: &str) -> String {
	if path.starts_with("http") {
		path.to_string()
	} else {
		format!("{}{}", base_url(), path)
	}
}

/// Titles are addressed by `dir`, which is usually a plain slug but sometimes
/// carries a licensing marker such as `<29.04.2026>omniscient-reader_`.
fn title_path(dir: &str) -> String {
	encode_uri_component(dir)
}

fn parse_timestamp(value: Option<String>) -> Option<i64> {
	let value = non_empty(value)?;
	parse_date(value.get(..19)?, DATE_FORMAT)
}

/// Maps the site's status ids, as listed by `/api/forms/titles/?get=status`.
///
/// «Лицензировано» means the translation was pulled after a licence was
/// granted, which leaves the entry permanently where it stopped; `Cancelled`
/// is the closest thing Aidoku has to say that.
fn parse_status(status: Option<&NamedId>) -> MangaStatus {
	match status.map(|s| s.id) {
		Some(1) => MangaStatus::Completed,
		Some(2) => MangaStatus::Ongoing,
		Some(3) => MangaStatus::Hiatus,
		Some(6) => MangaStatus::Cancelled,
		// 5 is «Анонс», a title announced but not yet released.
		_ => MangaStatus::Unknown,
	}
}

/// Maps the site's type ids, as listed by `/api/forms/titles/?get=types`.
fn parse_viewer(kind: Option<&NamedId>) -> Viewer {
	match kind.map(|k| k.id) {
		Some(1) => Viewer::RightToLeft,
		// Manhwa, manhua and Indonesian comics are all long-strip here.
		Some(2) | Some(3) | Some(6) => Viewer::Webtoon,
		Some(4) => Viewer::LeftToRight,
		// 5 «Рукомикс», 7 «Другое» and 22 «Оригиналы» are mixed formats.
		_ => Viewer::Unknown,
	}
}

/// Works out a rating from whatever the endpoint happened to include.
///
/// Catalog rows carry none of these fields, so a listing yields `Unknown`
/// rather than a guessed `Safe`; the real value arrives with the details.
fn parse_content_rating(title: &ApiTitle) -> ContentRating {
	if title.is_erotic.unwrap_or(false) || title.is_yaoi.unwrap_or(false) {
		return ContentRating::NSFW;
	}
	match title.age_limit.as_ref().map(|a| a.id) {
		Some(2) => ContentRating::NSFW,
		Some(1) => ContentRating::Suggestive,
		Some(0) => ContentRating::Safe,
		_ => ContentRating::Unknown,
	}
}

fn names_of(creators: &Option<Vec<Creator>>, kinds: &[i32]) -> Option<Vec<String>> {
	let creators = creators.as_ref()?;
	let names: Vec<String> = creators
		.iter()
		.filter(|creator| creator.kind.is_some_and(|kind| kinds.contains(&kind)))
		.map(|creator| creator.name.clone())
		.collect();
	(!names.is_empty()).then_some(names)
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
				id: String::from("genres"),
				included: Vec::from([String::from(*id)]),
				excluded: Vec::new(),
			}])),
		})
		.collect()
}

impl ApiTitle {
	fn tags(&self) -> Option<Vec<String>> {
		let mut tags: Vec<String> = Vec::new();
		for group in [self.genres.as_ref(), self.categories.as_ref()]
			.into_iter()
			.flatten()
		{
			for tag in group {
				if !tags.contains(&tag.name) {
					tags.push(tag.name.clone());
				}
			}
		}
		(!tags.is_empty()).then_some(tags)
	}

	fn into_manga(self) -> Manga {
		let title = non_empty(self.main_name.clone())
			.or_else(|| non_empty(self.secondary_name.clone()))
			.unwrap_or_default();

		Manga {
			key: self.dir.clone(),
			title,
			cover: self
				.cover
				.as_ref()
				.and_then(Cover::best)
				.map(|path| absolute_url(path)),
			artists: names_of(&self.creators, &[2, 4]),
			authors: names_of(&self.creators, &[1]),
			description: non_empty(self.description.clone()),
			url: Some(format!("{}/manga/{}", base_url(), title_path(&self.dir))),
			tags: self.tags(),
			status: parse_status(self.status.as_ref()),
			content_rating: parse_content_rating(&self),
			viewer: parse_viewer(self.kind.as_ref()),
			update_strategy: UpdateStrategy::Always,
			next_update_time: None,
			chapters: None,
		}
	}
}

impl ApiChapter {
	fn into_chapter(self, dir: &str) -> Chapter {
		let scanlators = self.publishers.as_ref().and_then(|publishers| {
			let names: Vec<String> = publishers.iter().map(|p| p.name.clone()).collect();
			(!names.is_empty()).then_some(names)
		});

		Chapter {
			key: self.id.to_string(),
			title: non_empty(self.name),
			// The API keeps this as a string so that halves like "53.5"
			// survive; anything it cannot parse is simply left unnumbered.
			chapter_number: self.chapter.and_then(|value| value.trim().parse::<f32>().ok()),
			volume_number: self.tome,
			date_uploaded: parse_timestamp(self.upload_date),
			scanlators,
			url: Some(format!(
				"{}/manga/{}/ch{}",
				base_url(),
				title_path(dir),
				self.id
			)),
			language: Some(String::from("ru")),
			thumbnail: None,
			// Bought chapters read normally, so only the unpaid ones are
			// marked; otherwise a reader's own purchases would look locked.
			locked: self.is_paid.unwrap_or(false) && !self.is_bought.unwrap_or(false),
		}
	}
}

// MARK: - Source

struct Remanga;

impl Remanga {
	fn fetch_titles(path: &str, query: &QueryParameters) -> Result<Vec<ApiTitle>> {
		let url = format!("{}/api/v2/{path}?{query}", base_url());
		let page: CursorPage<ApiTitle> = Request::get(url)?.json_owned()?;
		Ok(page.results.unwrap_or_default())
	}

	/// Runs a catalog query. The response carries a cursor rather than a
	/// total, so the presence of `next` is what decides whether to page on.
	fn fetch_catalog(query: QueryParameters) -> Result<MangaPageResult> {
		let url = format!("{}/api/v2/search/catalog/?{query}", base_url());
		let page: CursorPage<ApiTitle> = Request::get(url)?.json_owned()?;

		Ok(MangaPageResult {
			entries: page
				.results
				.unwrap_or_default()
				.into_iter()
				.map(ApiTitle::into_manga)
				.collect(),
			has_next_page: page.next.is_some(),
		})
	}

	fn base_query(page: i32) -> QueryParameters {
		let mut query = QueryParameters::new();
		query.push("page", Some(&page.max(1).to_string()));
		query.push("count", Some(&PAGE_LIMIT.to_string()));
		query
	}

	/// Maps the index of the `ordering` filter onto the API's values, in the
	/// same order the options appear in `filters.json`.
	fn ordering(index: i32) -> &'static str {
		match index {
			1 => "rating",
			2 => "views",
			3 => "chapter_date",
			4 => "id",
			5 => "votes",
			_ => "score",
		}
	}

	fn push_ordering(query: &mut QueryParameters, index: i32, ascending: bool) {
		let field = Self::ordering(index);
		let value = if ascending {
			String::from(field)
		} else {
			format!("-{field}")
		};
		query.push("ordering", Some(&value));
	}

	/// Translates Aidoku's filter values into the API's query parameters.
	///
	/// The wire format is repeated parameters with no brackets: `genres=3`
	/// twice, never `genres[]=3`, which the API accepts and then ignores. What
	/// repeating one means is not uniform, and the filter sheet is built to
	/// match: `genres` and `categories` intersect, `types` unions, and
	/// `status`, `translate_status` and `age_limit` keep only the last value
	/// given, so those three are single-choice selects.
	fn push_filters(query: &mut QueryParameters, filters: Vec<FilterValue>) {
		for filter in filters {
			match filter {
				FilterValue::Sort {
					index, ascending, ..
				} => Self::push_ordering(query, index, ascending),
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} => {
					for value in included {
						query.push(&id, Some(&value));
					}
					for value in excluded {
						query.push(&format!("exclude_{id}"), Some(&value));
					}
				}
				// The leading "Любой" option carries an empty id, which the
				// API would read as a real, matchless value.
				FilterValue::Select { id, value } if !value.is_empty() => {
					query.push(&id, Some(&value));
				}
				// Ranges are two parameters, suffixed. The double-underscore
				// spelling Django uses internally is ignored here.
				FilterValue::Range { id, from, to } => {
					if let Some(from) = from {
						query.push(&format!("{id}_gte"), Some(&from.to_string()));
					}
					if let Some(to) = to {
						query.push(&format!("{id}_lte"), Some(&to.to_string()));
					}
				}
				_ => {}
			}
		}
	}

	/// Picks the branch to read chapters from.
	///
	/// Nearly every title has exactly one. Where there are several they are
	/// rival translations of the same work rather than different parts of it,
	/// so the fullest one is the only sensible default.
	fn best_branch(title: &ApiTitle) -> Option<i64> {
		title
			.branches
			.as_ref()?
			.iter()
			.max_by_key(|branch| branch.count_chapters.unwrap_or(0))
			.map(|branch| branch.id)
	}

	/// Names the titles the site files as similar to this one.
	///
	/// This is the nearest thing the site has to volume navigation. A long run
	/// is one title per instalment under near-identical names, and the entry
	/// itself says nothing about the ones around it; the similar list puts the
	/// rest of the run at the top, ranked, because they share everything a
	/// relevance score looks at.
	///
	/// Aidoku's `Manga` has no field for related titles, so the description is
	/// the only place they can be shown at all.
	fn similar_titles(dir: &str) -> Vec<String> {
		let url = format!("{}/api/v2/titles/{}/similar/", base_url(), title_path(dir));
		let Ok(request) = Request::get(url) else {
			return Vec::new();
		};
		let Ok(page) = request.json_owned::<CursorPage<SimilarEntry>>() else {
			return Vec::new();
		};

		let mut names: Vec<String> = Vec::new();
		for entry in page.results.unwrap_or_default() {
			let Some(name) = entry
				.title
				.and_then(|title| non_empty(title.main_name).or(non_empty(title.secondary_name)))
			else {
				continue;
			};
			if !names.contains(&name) {
				names.push(name);
			}
			if names.len() >= SIMILAR_COUNT {
				break;
			}
		}
		names
	}

	fn fetch_title(dir: &str) -> Result<ApiTitle> {
		let url = format!("{}/api/v2/titles/{}/", base_url(), title_path(dir));
		Request::get(url)?.json_owned()
	}

	/// Fetches a title, recovering from the one way its key goes stale.
	///
	/// Titles are addressed by `dir`, and the site has no id-based lookup to
	/// fall back on — `/api/v2/titles/38753/` is a 404. When a title is
	/// licensed its `dir` is rewritten with a dated marker, so
	/// `omniscient-reader_` becomes `<29.04.2026>omniscient-reader_` and the
	/// old form stops resolving. Anything already in the library, or synced
	/// from another device, is then a dead entry.
	///
	/// Since the rewrite only ever prefixes the existing slug, searching for
	/// the stored title and taking the result whose `dir` ends with the stored
	/// one recovers it precisely, without matching unrelated titles.
	fn resolve_title(manga: &Manga) -> Result<ApiTitle> {
		if let Ok(title) = Self::fetch_title(&manga.key) {
			return Ok(title);
		}
		if manga.title.is_empty() {
			return Self::fetch_title(&manga.key);
		}

		let mut params = Self::base_query(1);
		params.push("query", Some(&manga.title));
		let url = format!("{}/api/v2/search/?{params}", base_url());
		let found: SearchPage = Request::get(url)?.json_owned()?;

		found
			.results
			.unwrap_or_default()
			.into_iter()
			.find(|candidate| candidate.dir.ends_with(&manga.key))
			.ok_or_else(|| error!("This title is no longer on the site"))
	}

	/// Walks every page of a branch's chapter list.
	///
	/// They arrive newest first already, which is the order Aidoku shows them
	/// in, so nothing is sorted afterwards.
	fn fetch_chapters(dir: &str, branch: i64) -> Result<Vec<Chapter>> {
		let mut chapters: Vec<Chapter> = Vec::new();
		let mut page = 1;

		loop {
			let mut query = QueryParameters::new();
			query.push("branch_id", Some(&branch.to_string()));
			query.push("count", Some(&CHAPTER_LIMIT.to_string()));
			query.push("page", Some(&page.to_string()));

			let url = format!("{}/api/v2/titles/chapters/?{query}", base_url());
			let result: CursorPage<ApiChapter> = Request::get(url)?.json_owned()?;
			let batch = result.results.unwrap_or_default();

			// A page that comes back empty would otherwise loop forever if the
			// cursor ever disagreed with the body.
			if batch.is_empty() {
				break;
			}
			for chapter in batch {
				chapters.push(chapter.into_chapter(dir));
			}
			if result.next.is_none() {
				break;
			}
			page += 1;
		}

		Ok(chapters)
	}
}

impl Source for Remanga {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		// Search and filtering are separate endpoints, and neither honours the
		// other's parameters: the catalog ignores `query` outright and the
		// search endpoint ignores every filter. The manifest hides the filter
		// sheet while a query is active so the two never appear to combine.
		if let Some(query) = non_empty(query) {
			let mut params = Self::base_query(page);
			params.push("query", Some(&query));

			let url = format!("{}/api/v2/search/?{params}", base_url());
			let result: SearchPage = Request::get(url)?.json_owned()?;

			let meta = result.meta;
			let current = meta.as_ref().and_then(|m| m.page).unwrap_or(page.max(1));
			let total = meta.as_ref().and_then(|m| m.total_pages).unwrap_or(current);

			return Ok(MangaPageResult {
				entries: result
					.results
					.unwrap_or_default()
					.into_iter()
					.map(ApiTitle::into_manga)
					.collect(),
				has_next_page: current < total,
			});
		}

		let mut params = Self::base_query(page);
		Self::push_filters(&mut params, filters);
		Self::fetch_catalog(params)
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

		// Chapters hang off a branch that only the details carry, so one
		// request serves both halves.
		let details = Self::resolve_title(&manga)?;

		// A recovered title answers under its current `dir`, so the stale one
		// is dropped here rather than being carried into the chapter urls.
		manga.key = details.dir.clone();

		if needs_chapters {
			let Some(branch) = Self::best_branch(&details) else {
				bail!("This title has no translations to read");
			};
			manga.chapters = Some(Self::fetch_chapters(&manga.key, branch)?);
		}

		if needs_details {
			manga.copy_from(details.into_manga());

			// Costs one extra request, so it is only worth it on the screen
			// that shows a description, and a failure here leaves the rest of
			// the details intact rather than losing them.
			let similar = Self::similar_titles(&manga.key);
			if !similar.is_empty() {
				let mut note = String::from("Похожее: ");
				for (index, name) in similar.iter().enumerate() {
					if index > 0 {
						note.push_str(" · ");
					}
					note.push_str(name);
				}

				manga.description = Some(match manga.description.take() {
					Some(text) if !text.trim().is_empty() => format!("{text}\n\n{note}"),
					_ => note,
				});
			}
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{}/api/v2/titles/chapters/{}/", base_url(), chapter.key);
		let detail: ChapterDetail = Request::get(url)?.json_owned()?;

		// Pages arrive grouped into slides, each holding several images.
		let pages: Vec<Page> = detail
			.pages
			.unwrap_or_default()
			.into_iter()
			.flatten()
			.filter_map(|image| {
				let link = non_empty(image.link)?;
				Some(Page {
					content: PageContent::url(link),
					thumbnail: None,
					has_description: false,
					description: None,
				})
			})
			.collect();

		if pages.is_empty() {
			if chapter.locked {
				bail!("This chapter has to be bought on the site first");
			}
			bail!("Chapter contains no pages");
		}

		Ok(pages)
	}
}

impl ListingProvider for Remanga {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let ordering = match listing.id.as_str() {
			"popular" => "-score",
			"latest" => "-chapter_date",
			"rating" => "-rating",
			"views" => "-views",
			"new" => "-id",
			_ => return Err(AidokuError::Unimplemented),
		};

		let mut params = Self::base_query(page);
		params.push("ordering", Some(ordering));
		Self::fetch_catalog(params)
	}
}

impl Home for Remanga {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components = Vec::new();

		// The most popular titles are the hero, and the rest of that same
		// request fills the row beneath it, so the two cost one fetch.
		let mut params = Self::base_query(1);
		params.push("ordering", Some("-score"));
		let popular: Vec<Manga> = Self::fetch_titles("search/catalog/", &params)?
			.into_iter()
			.map(ApiTitle::into_manga)
			.collect();

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
		for (title, id, ordering) in [
			("Обновления", "latest", "-chapter_date"),
			("Лучшее по оценке", "rating", "-rating"),
			("Самое читаемое", "views", "-views"),
			("Новинки", "new", "-id"),
		] {
			let mut params = Self::base_query(1);
			params.push("ordering", Some(ordering));

			let Ok(titles) = Self::fetch_titles("search/catalog/", &params) else {
				continue;
			};
			let entries: Vec<Manga> = titles.into_iter().map(ApiTitle::into_manga).collect();
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

impl DeepLinkHandler for Remanga {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		// Either domain can turn up in a shared link, whichever the reader
		// happens to be browsing with.
		let path = ["https://remanga.org", DEFAULT_URL, &base_url()]
			.into_iter()
			.find_map(|host| url.strip_prefix(host));
		let Some(path) = path else {
			return Ok(None);
		};

		// Title pages are `/manga/{dir}`, with the reader and the tabs hanging
		// off further segments that the key does not need.
		let mut segments = path.trim_matches('/').split('/');
		if segments.next() != Some("manga") {
			return Ok(None);
		}
		let Some(dir) = segments.next().filter(|s| !s.is_empty()) else {
			return Ok(None);
		};

		Ok(Some(DeepLinkResult::Manga {
			key: String::from(dir),
		}))
	}
}

register_source!(Remanga, ListingProvider, DeepLinkHandler, Home);
