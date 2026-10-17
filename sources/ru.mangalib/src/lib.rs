#![no_std]
//! Aidoku source for mangalib.org.
//!
//! Three things about this site decide how the source is written.
//!
//! The first is that the front end and the API are separate hosts:
//! `mangalib.org` serves the pages, `api.cdnlibs.org` serves the data, and
//! the API answers nothing without a `Referer` naming the front end. The
//! details endpoint wants `Site-Id` on top of that — without it a perfectly
//! valid slug comes back as a 404 with a "Not Found" toast. Both headers are
//! therefore on every request here.
//!
//! The second is that the same API serves several sibling libraries (manga,
//! ranobe, two adult ones), told apart by a site id. Everything here passes
//! `site_id[]=1`, and the filter vocabulary is cut down to the entries whose
//! `site_ids` contain 1, so the sheet never offers a tag that cannot match.
//!
//! The third is that several filter parameters are accepted and then
//! silently ignored rather than rejected: `exclude_genres`, `types_exclude`,
//! `status_exclude` and every spelling of an age-restriction filter return
//! the unfiltered catalog. Only the parameters checked against the returned
//! rows are shipped, which is why there is no age filter in `filters.json`
//! and why exclusion is offered for genres and tags alone.

use aidoku::{
	AidokuError, Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterItem, FilterValue,
	Home, HomeComponent, HomeComponentValue, HomeLayout, ImageRequestProvider, Link, LinkValue,
	Listing, ListingKind, ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent,
	PageContext, Result, Source, UpdateStrategy, Viewer,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{net::Request, std::parse_date},
	prelude::*,
};
use serde::{Deserialize, de::DeserializeOwned};

/// The front end. Only ever used for links shown to the reader and for the
/// `Referer` the API and the image hosts insist on.
const BASE_URL: &str = "https://mangalib.org";

/// Sent as `Referer` on every request. The trailing slash is what the site's
/// own pages send.
const REFERER: &str = "https://mangalib.org/";

const API_URL: &str = "https://api.cdnlibs.org/api";

/// MangaLib itself. The same API serves ranobe as 3 and the adult libraries
/// as 2 and 4, which is what the header and the repeated query parameter
/// below keep this source out of.
const SITE_ID: &str = "1";

/// Page images, as named by `/constants?fields[]=imageServers`.
///
/// The "main" server (`img2.imglib.info`) answers every request with AVIF
/// whatever the `Accept` header asks for. This is the site's own "compress"
/// and "download" server, which serves the identical image — same pixel
/// dimensions, byte for byte the same source — as JPEG, and serves it
/// without needing a `Referer` at all. A format every reader can decode is
/// worth more here than the third off the transfer.
const IMAGE_SERVER: &str = "https://img3.cdnlibs.org";

/// Fixed by the API: `meta.per_page` is 60 whatever is asked for.
const PAGE_SIZE: usize = 60;

/// How many of the top titles get the large treatment on the home page.
const FEATURED_COUNT: usize = 5;

/// How many related and similar titles to name in a description.
const RELATED_COUNT: usize = 8;

/// `2020-05-29T16:28:56.000000Z` — the fractional part and the zone marker
/// are trimmed before parsing, so only the stable prefix is described here.
const DATE_FORMAT: &str = "yyyy-MM-dd'T'HH:mm:ss";

/// A readable slice of the genre list, shown as chips on the home page.
const HOME_GENRES: &[(&str, &str)] = &[
	("34", "Боевик"),
	("69", "Фэнтези"),
	("56", "Романтика"),
	("43", "Драма"),
	("47", "Комедия"),
	("54", "Приключения"),
	("58", "Сверхъестественное"),
	("79", "Исекай"),
	("67", "Ужасы"),
	("40", "Детектив"),
	("70", "Школа"),
	("51", "Научная фантастика"),
];

// MARK: - API models

#[derive(Deserialize)]
struct Cover {
	thumbnail: Option<String>,
	default: Option<String>,
}

impl Cover {
	/// `default` and `md` are the same file; the thumbnail is a smaller
	/// render of it and is only worth taking when the full one is missing.
	fn best(&self) -> Option<&String> {
		self.default.as_ref().or(self.thumbnail.as_ref())
	}
}

/// Types, statuses and age restrictions label themselves with `label`; the
/// genre, tag and format vocabularies use `name` for the same thing.
#[derive(Deserialize)]
struct LabeledId {
	id: i32,
}

#[derive(Deserialize)]
struct Named {
	name: String,
}

/// ProseMirror's document format, which is what `summary` is: a tree of
/// nodes rather than a string. Older entries are still plain strings, hence
/// the untagged pair below.
#[derive(Deserialize)]
struct ProseNode {
	#[serde(rename = "type")]
	kind: Option<String>,
	text: Option<String>,
	content: Option<Vec<ProseNode>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Summary {
	Text(String),
	Doc(ProseNode),
}

impl ProseNode {
	fn write_into(&self, out: &mut String) {
		if let Some(text) = &self.text {
			out.push_str(text);
		}
		if self.kind.as_deref() == Some("hardBreak") {
			out.push('\n');
		}
		for child in self.content.iter().flatten() {
			child.write_into(out);
		}
		// Block-level nodes are what carry the paragraph breaks; inline
		// nodes run together.
		if matches!(
			self.kind.as_deref(),
			Some("paragraph") | Some("heading") | Some("blockquote") | Some("listItem")
		) {
			out.push_str("\n\n");
		}
	}
}

impl Summary {
	fn into_text(self) -> Option<String> {
		let text = match self {
			Self::Text(text) => text,
			Self::Doc(node) => {
				let mut out = String::new();
				node.write_into(&mut out);
				out
			}
		};
		let trimmed = text.trim();
		(!trimmed.is_empty()).then(|| String::from(trimmed))
	}
}

#[derive(Deserialize)]
struct ApiTitle {
	slug_url: String,
	name: Option<String>,
	rus_name: Option<String>,
	eng_name: Option<String>,
	cover: Option<Cover>,
	#[serde(rename = "type")]
	kind: Option<LabeledId>,
	status: Option<LabeledId>,
	#[serde(rename = "ageRestriction")]
	age_restriction: Option<LabeledId>,
	genres: Option<Vec<Named>>,
	tags: Option<Vec<Named>>,
	authors: Option<Vec<Named>>,
	artists: Option<Vec<Named>>,
	summary: Option<Summary>,
}

/// Both the relations and the similar endpoints wrap the title the same way,
/// and differ only in the label they put beside it.
#[derive(Deserialize)]
struct RelatedEntry {
	media: Option<ApiTitle>,
	#[serde(rename = "related_type")]
	related_type: Option<RelatedLabel>,
}

#[derive(Deserialize)]
struct RelatedLabel {
	label: Option<String>,
}

#[derive(Deserialize)]
struct Meta {
	has_next_page: Option<bool>,
}

#[derive(Deserialize)]
struct Envelope<T> {
	data: Option<T>,
}

#[derive(Deserialize)]
struct ListEnvelope<T> {
	data: Option<Vec<T>>,
	meta: Option<Meta>,
}

/// One chapter, as the chapter list gives it. A chapter is identified by its
/// volume and number rather than by its id, because that is what the page
/// endpoint takes.
#[derive(Deserialize)]
struct ApiChapter {
	volume: Option<String>,
	number: Option<String>,
	name: Option<String>,
	branches: Option<Vec<ChapterBranch>>,
}

/// One team's translation of that chapter.
///
/// `branch_id` is the one to ask the page endpoint for; `id` is the chapter
/// row's own id and is not interchangeable with it. A title with a single
/// translation leaves `branch_id` null, and the page endpoint then needs no
/// branch at all.
#[derive(Deserialize)]
struct ChapterBranch {
	branch_id: Option<i64>,
	created_at: Option<String>,
	teams: Option<Vec<Named>>,
}

#[derive(Deserialize)]
struct ApiPage {
	url: Option<String>,
}

#[derive(Deserialize)]
struct ChapterDetail {
	pages: Option<Vec<ApiPage>>,
}

// MARK: - Helpers

fn non_empty(value: Option<String>) -> Option<String> {
	value.filter(|s| !s.trim().is_empty())
}

fn parse_timestamp(value: Option<String>) -> Option<i64> {
	let value = non_empty(value)?;
	parse_date(value.get(..19)?, DATE_FORMAT)
}

/// Maps the site's status ids, as listed by `/constants?fields[]=status`.
fn parse_status(status: Option<&LabeledId>) -> MangaStatus {
	match status.map(|s| s.id) {
		Some(1) => MangaStatus::Ongoing,
		Some(2) => MangaStatus::Completed,
		Some(4) => MangaStatus::Hiatus,
		Some(5) => MangaStatus::Cancelled,
		// 3 is «Анонс», a title announced but not yet released.
		_ => MangaStatus::Unknown,
	}
}

/// Maps the site's type ids, as listed by `/constants?fields[]=types`.
fn parse_viewer(kind: Option<&LabeledId>) -> Viewer {
	match kind.map(|k| k.id) {
		Some(1) => Viewer::RightToLeft,
		// Manhwa and manhua are long-strip.
		Some(5) | Some(6) => Viewer::Webtoon,
		// OEL manga and comics read the western way.
		Some(4) | Some(9) => Viewer::LeftToRight,
		// 8 «Руманга» is drawn both ways, so it is left to the reader.
		_ => Viewer::Unknown,
	}
}

/// Maps `/constants?fields[]=ageRestriction`: 1 is 6+, 2 is 12+, 3 is 16+,
/// and 4 and 5 are the two 18+ grades.
fn parse_content_rating(age: Option<&LabeledId>) -> ContentRating {
	match age.map(|a| a.id) {
		Some(1) | Some(2) => ContentRating::Safe,
		Some(3) => ContentRating::Suggestive,
		Some(4) | Some(5) => ContentRating::NSFW,
		_ => ContentRating::Unknown,
	}
}

fn names_of(people: &Option<Vec<Named>>) -> Option<Vec<String>> {
	let names: Vec<String> = people.as_ref()?.iter().map(|p| p.name.clone()).collect();
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
	/// The Russian name is the one the site shows; the romanisation and the
	/// English title are the fallbacks, in that order.
	fn display_title(&self) -> String {
		non_empty(self.rus_name.clone())
			.or_else(|| non_empty(self.name.clone()))
			.or_else(|| non_empty(self.eng_name.clone()))
			.unwrap_or_default()
	}

	fn all_tags(&self) -> Option<Vec<String>> {
		let mut tags: Vec<String> = Vec::new();
		for group in [self.genres.as_ref(), self.tags.as_ref()]
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
		let title = self.display_title();
		let tags = self.all_tags();

		Manga {
			key: self.slug_url.clone(),
			title,
			cover: self.cover.as_ref().and_then(Cover::best).cloned(),
			artists: names_of(&self.artists),
			authors: names_of(&self.authors),
			description: self.summary.and_then(Summary::into_text),
			url: Some(format!("{BASE_URL}/ru/manga/{}", self.slug_url)),
			tags,
			status: parse_status(self.status.as_ref()),
			content_rating: parse_content_rating(self.age_restriction.as_ref()),
			viewer: parse_viewer(self.kind.as_ref()),
			update_strategy: UpdateStrategy::Always,
			next_update_time: None,
			chapters: None,
		}
	}
}

// MARK: - Source

struct MangaLib;

impl MangaLib {
	/// Every call goes through here, because the API answers nothing without
	/// both headers: no `Referer` is a 403 from the edge, and no `Site-Id`
	/// turns the details endpoint into a 404.
	fn get<T: DeserializeOwned>(path: &str) -> Result<T> {
		Request::get(format!("{API_URL}{path}"))?
			.header("Referer", REFERER)
			.header("Site-Id", SITE_ID)
			.json_owned()
	}

	fn catalog(query: QueryParameters) -> Result<MangaPageResult> {
		let page: ListEnvelope<ApiTitle> = Self::get(&format!("/manga?{query}"))?;
		let entries = page.data.unwrap_or_default();

		Ok(MangaPageResult {
			// `has_next_page` is a real boolean here rather than something to
			// be inferred from the row count, so it is taken as given; the
			// page size is only the fallback for a response without meta.
			has_next_page: page
				.meta
				.and_then(|meta| meta.has_next_page)
				.unwrap_or(entries.len() >= PAGE_SIZE),
			entries: entries.into_iter().map(ApiTitle::into_manga).collect(),
		})
	}

	fn base_query(page: i32) -> QueryParameters {
		let mut query = QueryParameters::new();
		query.push("site_id[]", Some(SITE_ID));
		query.push("page", Some(&page.max(1).to_string()));
		query
	}

	/// Maps the index of the sort filter onto the API's values, in the same
	/// order the options appear in `filters.json`.
	fn sort_field(index: i32) -> &'static str {
		match index {
			1 => "views",
			2 => "last_chapter_at",
			3 => "created_at",
			4 => "releaseDate",
			5 => "chap_count",
			6 => "rate_avg",
			7 => "name",
			_ => "rating_score",
		}
	}

	fn push_sort(query: &mut QueryParameters, field: &str, ascending: bool) {
		query.push("sort_by", Some(field));
		query.push("sort_type", Some(if ascending { "asc" } else { "desc" }));
	}

	/// Translates Aidoku's filter values into query parameters.
	///
	/// Repeated values mean different things depending on the parameter, and
	/// the filter sheet is built to match what was measured: `genres` and
	/// `tags` intersect, while `types`, `status`, `scanlate_status` and
	/// `format` union. Exclusion exists for genres and tags only — the
	/// `_exclude` spelling works there and is ignored everywhere else.
	fn push_filters(query: &mut QueryParameters, filters: Vec<FilterValue>) {
		for filter in filters {
			match filter {
				FilterValue::Sort {
					index, ascending, ..
				} => Self::push_sort(query, Self::sort_field(index), ascending),
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} => {
					for value in included {
						query.push(&format!("{id}[]"), Some(&value));
					}
					for value in excluded {
						query.push(&format!("{id}_exclude[]"), Some(&value));
					}
				}
				FilterValue::Range { id, from, to } => {
					if let Some(from) = from {
						query.push(&format!("{id}_min"), Some(&from.to_string()));
					}
					if let Some(to) = to {
						query.push(&format!("{id}_max"), Some(&to.to_string()));
					}
				}
				_ => {}
			}
		}
	}

	fn fetch_titles(query: &QueryParameters) -> Result<Vec<ApiTitle>> {
		let page: ListEnvelope<ApiTitle> = Self::get(&format!("/manga?{query}"))?;
		Ok(page.data.unwrap_or_default())
	}

	fn fetch_title(slug: &str) -> Result<ApiTitle> {
		let fields = [
			"summary", "genres", "tags", "authors", "artists", "teams", "status_id",
		]
		.map(|field| format!("fields[]={field}"))
		.join("&");

		let title: Envelope<ApiTitle> = Self::get(&format!("/manga/{slug}?{fields}"))?;
		title
			.data
			.ok_or_else(|| error!("This title is no longer on the site"))
	}

	/// Names the titles the site files alongside this one.
	///
	/// `/relations` is the one that answers the question a long run raises —
	/// which part comes next — and `/similar` is the looser recommendation
	/// list. Aidoku's `Manga` has no field for either, so the description is
	/// the only place they can be shown.
	fn related_titles(slug: &str, path: &str) -> Vec<String> {
		let Ok(page) = Self::get::<ListEnvelope<RelatedEntry>>(&format!("/manga/{slug}/{path}"))
		else {
			return Vec::new();
		};

		let mut names: Vec<String> = Vec::new();
		for entry in page.data.unwrap_or_default() {
			let Some(media) = entry.media else { continue };
			let title = media.display_title();
			if title.is_empty() {
				continue;
			}
			let name = match entry.related_type.and_then(|r| non_empty(r.label)) {
				Some(label) => format!("{title} ({label})"),
				None => title,
			};
			if !names.contains(&name) {
				names.push(name);
			}
			if names.len() >= RELATED_COUNT {
				break;
			}
		}
		names
	}

	fn append_note(description: &mut Option<String>, heading: &str, names: &[String]) {
		if names.is_empty() {
			return;
		}
		let mut note = String::from(heading);
		for (index, name) in names.iter().enumerate() {
			if index > 0 {
				note.push_str(" · ");
			}
			note.push_str(name);
		}

		*description = Some(match description.take() {
			Some(text) if !text.trim().is_empty() => format!("{text}\n\n{note}"),
			_ => note,
		});
	}

	/// Builds the chapter list.
	///
	/// The endpoint returns every chapter in one response, oldest first, and
	/// each row carries one entry per translation of it. Those are rival
	/// translations rather than different parts of the work, so each becomes
	/// its own chapter with the team named as the scanlator, which is what
	/// lets a reader pick between them.
	fn fetch_chapters(slug: &str) -> Result<Vec<Chapter>> {
		let list: ListEnvelope<ApiChapter> = Self::get(&format!("/manga/{slug}/chapters"))?;
		let mut chapters: Vec<Chapter> = Vec::new();

		for item in list.data.unwrap_or_default().into_iter().rev() {
			let volume = item.volume.clone().unwrap_or_default();
			let number = item.number.clone().unwrap_or_default();
			let title = non_empty(item.name.clone());

			let branches = item.branches.unwrap_or_default();
			// A title with no branch row at all still reads: the page
			// endpoint then serves the only translation there is.
			if branches.is_empty() {
				chapters.push(Self::build_chapter(
					slug, &volume, &number, &title, None, None, None,
				));
				continue;
			}

			for branch in branches {
				let scanlators = branch.teams.as_ref().and_then(|teams| {
					let names: Vec<String> = teams.iter().map(|t| t.name.clone()).collect();
					(!names.is_empty()).then_some(names)
				});
				chapters.push(Self::build_chapter(
					slug,
					&volume,
					&number,
					&title,
					branch.branch_id,
					parse_timestamp(branch.created_at),
					scanlators,
				));
			}
		}

		Ok(chapters)
	}

	/// The key is what `get_page_list` takes apart again: volume and number
	/// are what the page endpoint asks for, and the branch picks the
	/// translation when there is more than one.
	fn build_chapter(
		slug: &str,
		volume: &str,
		number: &str,
		title: &Option<String>,
		branch: Option<i64>,
		date_uploaded: Option<i64>,
		scanlators: Option<Vec<String>>,
	) -> Chapter {
		let key = match branch {
			Some(branch) => format!("{volume}/{number}/{branch}"),
			None => format!("{volume}/{number}"),
		};

		Chapter {
			key,
			title: title.clone(),
			// Kept as a string by the API so that halves like "53.5"
			// survive; anything unparseable is simply left unnumbered.
			chapter_number: number.trim().parse::<f32>().ok(),
			volume_number: volume.trim().parse::<f32>().ok(),
			date_uploaded,
			scanlators,
			url: Some(format!("{BASE_URL}/ru/manga/{slug}/read/v{volume}/c{number}")),
			language: Some(String::from("ru")),
			thumbnail: None,
			locked: false,
		}
	}
}

impl Source for MangaLib {
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
		// One endpoint does both, and a query narrows the filtered set
		// rather than replacing it, so the filter sheet stays usable while
		// searching — `hidesFiltersWhileSearching` is off in the manifest
		// for that reason.
		if let Some(query) = non_empty(query) {
			params.push("q", Some(&query));
		}
		Self::push_filters(&mut params, filters);
		Self::catalog(params)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if needs_details {
			let details = Self::fetch_title(&manga.key)?;
			manga.copy_from(details.into_manga());

			// Two extra requests, so they are only worth it on the screen
			// that shows a description; a failure in either leaves the rest
			// of the details intact rather than losing them.
			let relations = Self::related_titles(&manga.key, "relations");
			Self::append_note(&mut manga.description, "Связанное: ", &relations);
			let similar = Self::related_titles(&manga.key, "similar");
			Self::append_note(&mut manga.description, "Похожее: ", &similar);
		}

		if needs_chapters {
			manga.chapters = Some(Self::fetch_chapters(&manga.key)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let mut parts = chapter.key.split('/');
		let (Some(volume), Some(number)) = (parts.next(), parts.next()) else {
			bail!("Chapter cannot be opened");
		};

		let mut query = QueryParameters::new();
		query.push("number", Some(number));
		query.push("volume", Some(volume));
		if let Some(branch) = parts.next().filter(|value| !value.is_empty()) {
			query.push("branch_id", Some(branch));
		}

		let detail: Envelope<ChapterDetail> =
			Self::get(&format!("/manga/{}/chapter?{query}", manga.key))?;

		// Page urls arrive as host-relative fragments, `//manga/<slug>/…`,
		// to be hung off whichever image server is in use.
		let pages: Vec<Page> = detail
			.data
			.and_then(|detail| detail.pages)
			.unwrap_or_default()
			.into_iter()
			.filter_map(|page| {
				let url = non_empty(page.url)?;
				Some(Page {
					content: PageContent::url(format!(
						"{IMAGE_SERVER}/{}",
						url.trim_start_matches('/')
					)),
					thumbnail: None,
					has_description: false,
					description: None,
				})
			})
			.collect();

		if pages.is_empty() {
			bail!("Chapter contains no pages");
		}

		Ok(pages)
	}
}

impl ListingProvider for MangaLib {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let field = match listing.id.as_str() {
			"popular" => "rating_score",
			"latest" => "last_chapter_at",
			"views" => "views",
			"rating" => "rate_avg",
			"new" => "created_at",
			_ => return Err(AidokuError::Unimplemented),
		};

		let mut params = Self::base_query(page);
		Self::push_sort(&mut params, field, false);
		Self::catalog(params)
	}
}

impl Home for MangaLib {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components = Vec::new();

		// The highest-ranked titles are the hero, and the rest of that same
		// response fills the row beneath it, so the two cost one request.
		let mut params = Self::base_query(1);
		Self::push_sort(&mut params, "rating_score", false);
		let popular: Vec<Manga> = Self::fetch_titles(&params)?
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
		for (title, id, field) in [
			("Обновления", "latest", "last_chapter_at"),
			("Самое читаемое", "views", "views"),
			("Лучшее по оценке", "rating", "rate_avg"),
			("Новинки", "new", "created_at"),
		] {
			let mut params = Self::base_query(1);
			Self::push_sort(&mut params, field, false);

			let Ok(titles) = Self::fetch_titles(&params) else {
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

impl ImageRequestProvider for MangaLib {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		// Covers are the reason this exists: `cover.cdnlibs.org` answers a
		// request carrying no `Referer` with a 403 and an HTML body, which
		// is what would leave the grid empty. Page images come from a host
		// that does not need it, but the one header is harmless there.
		Ok(Request::get(url)?.header("Referer", REFERER))
	}
}

impl DeepLinkHandler for MangaLib {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};

		// Shared links take several shapes — `/ru/manga/{slug}`, `/manga/
		// {slug}`, the bare `/{slug}` the site used before, and any of those
		// with a reader path hanging off the end. The slug is the one
		// segment carrying the `{id}--{name}` form, so it is found rather
		// than counted to.
		let slug = path
			.trim_matches('/')
			.split('/')
			.find(|segment| segment.contains("--"));

		Ok(slug.map(|slug| DeepLinkResult::Manga {
			key: String::from(slug),
		}))
	}
}

register_source!(
	MangaLib,
	ListingProvider,
	DeepLinkHandler,
	Home,
	ImageRequestProvider
);
