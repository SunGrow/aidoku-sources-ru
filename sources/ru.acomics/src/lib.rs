#![no_std]
//! Aidoku source for acomics.ru.
//!
//! AComics hosts webcomics published one strip at a time, so each strip is
//! exposed as a single-page chapter rather than trying to group them.

use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Listing,
	ListingProvider, Manga, MangaPageResult, Page, PageContent, Result, Source, UpdateStrategy,
	Viewer,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{html::Document, net::Request},
	prelude::*,
};

const BASE_URL: &str = "https://acomics.ru";

/// The catalog returns ten cards per request and is paged with `skip`.
const PAGE_SIZE: i32 = 10;

/// Age ratings 1-6, all included unless the reader narrows them. Without this
/// the catalog hides the higher-rated comics.
const ALL_RATINGS: [&str; 6] = ["1", "2", "3", "4", "5", "6"];

fn absolute_url(path: &str) -> String {
	if path.starts_with("http") {
		path.to_string()
	} else {
		format!("{BASE_URL}{path}")
	}
}

/// Turns `/~LwHG` into `LwHG`, which is used as the manga key.
fn slug_from_href(href: &str) -> Option<String> {
	let slug = href
		.trim_start_matches(BASE_URL)
		.trim_start_matches('/')
		.trim_start_matches('~');
	let slug = slug.split('/').next()?;
	if slug.is_empty() {
		None
	} else {
		Some(slug.to_string())
	}
}

/// Parses the comic cards shared by the catalog and the search results.
fn parse_cards(html: &Document) -> Vec<Manga> {
	let Some(cards) = html.select("section.serial-card") else {
		return Vec::new();
	};

	cards
		.into_iter()
		.filter_map(|card| {
			let href = card.select_first("a.cover").and_then(|a| a.attr("href"))?;
			let key = slug_from_href(&href)?;
			let title = card.select_first("h2.title a").and_then(|a| a.text())?;

			// Covers are lazy-loaded: `src` holds a placeholder spinner and the
			// real path sits in `data-real-src`.
			let cover = card.select_first("a.cover img").and_then(|img| {
				img.attr("data-real-src")
					.or_else(|| img.attr("src"))
					.filter(|src| !src.ends_with(".svg"))
					.map(|src| absolute_url(&src))
			});

			Some(Manga {
				key: key.clone(),
				title,
				cover,
				description: card.select_first("p.about").and_then(|p| p.text()),
				url: Some(format!("{BASE_URL}/~{key}")),
				viewer: Viewer::Webtoon,
				update_strategy: UpdateStrategy::Always,
				..Default::default()
			})
		})
		.collect()
}

/// Reads a value out of the `<p><b>Label:</b> value</p>` rows on an about page.
fn labelled_value(html: &Document, label: &str) -> Option<String> {
	let paragraphs = html.select("p")?;
	for paragraph in paragraphs {
		let Some(text) = paragraph.text() else {
			continue;
		};
		if let Some(rest) = text.split_once(label).map(|(_, rest)| rest) {
			let value = rest.trim().to_string();
			if !value.is_empty() {
				return Some(value);
			}
		}
	}
	None
}

struct AComics;

impl AComics {
	fn fetch_cards(&self, url: String) -> Result<MangaPageResult> {
		let html = Request::get(url)?.html()?;
		let entries = parse_cards(&html);

		Ok(MangaPageResult {
			has_next_page: entries.len() as i32 >= PAGE_SIZE,
			entries,
		})
	}

	/// Maps the `sort` filter index onto the values the catalog form submits.
	fn sort_value(index: i32) -> &'static str {
		match index {
			1 => "serial_name",
			2 => "issue_count",
			3 => "subscr_count",
			_ => "last_update",
		}
	}

	fn catalog_url(page: i32, sort: &str, filters: &[FilterValue]) -> String {
		let mut params = QueryParameters::new();
		params.push("skip", Some(&((page.max(1) - 1) * PAGE_SIZE).to_string()));
		params.push("sort", Some(sort));

		let mut ratings: Vec<String> = Vec::new();
		for filter in filters {
			match filter {
				// The form submits these as `categories[]` / `ratings[]`; the
				// comma-separated form is accepted but silently ignored.
				FilterValue::MultiSelect { id, included, .. } if id == "categories" => {
					for value in included {
						params.push("categories[]", Some(value));
					}
				}
				FilterValue::MultiSelect { id, included, .. } if id == "ratings" => {
					ratings.extend(included.iter().cloned());
				}
				FilterValue::Select { id, value } if id == "updatable" => {
					params.push("updatable", Some(value));
				}
				_ => {}
			}
		}

		if ratings.is_empty() {
			for rating in ALL_RATINGS {
				params.push("ratings[]", Some(rating));
			}
		} else {
			for rating in &ratings {
				params.push("ratings[]", Some(rating));
			}
		}

		format!("{BASE_URL}/comics?{params}")
	}
}

impl Source for AComics {
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
			// Search returns a single unpaged result set.
			if page > 1 {
				return Ok(MangaPageResult {
					entries: Vec::new(),
					has_next_page: false,
				});
			}
			let mut params = QueryParameters::new();
			params.push("keyword", Some(&query));
			let mut result = self.fetch_cards(format!("{BASE_URL}/search?{params}"))?;
			result.has_next_page = false;
			return Ok(result);
		}

		let sort = filters
			.iter()
			.find_map(|filter| match filter {
				FilterValue::Sort { index, .. } => Some(Self::sort_value(*index)),
				_ => None,
			})
			.unwrap_or("last_update");

		self.fetch_cards(Self::catalog_url(page, sort, &filters))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let html = Request::get(format!("{BASE_URL}/~{}/about", manga.key))?.html()?;

		if needs_details {
			manga.url = Some(format!("{BASE_URL}/~{}", manga.key));
			manga.viewer = Viewer::Webtoon;

			if let Some(description) = html
				.select_first("section.serial-about-text")
				.and_then(|section| section.text())
			{
				manga.description = Some(description);
			}

			if let Some(author) = labelled_value(&html, "Автор оригинала:") {
				manga.authors = Some(Vec::from([author]));
			}

			if let Some(tags) = html.select("p.serial-about-badges a.badge.category") {
				let tags: Vec<String> = tags.into_iter().filter_map(|tag| tag.text()).collect();
				if !tags.is_empty() {
					manga.tags = Some(tags);
				}
			}

			// The catalog exposes an age rating per comic; "R" and above are the
			// ones worth marking.
			manga.content_rating = match labelled_value(&html, "Возрастной рейтинг:") {
				Some(rating) if rating.contains("NC-17") => ContentRating::NSFW,
				Some(rating) if rating.contains("R") => ContentRating::Suggestive,
				_ => ContentRating::Safe,
			};
		}

		if needs_chapters {
			let count = labelled_value(&html, "Количество выпусков:")
				.and_then(|value| {
					value
						.chars()
						.take_while(|c| c.is_ascii_digit())
						.collect::<String>()
						.parse::<i32>()
						.ok()
				})
				.unwrap_or(0);

			// Strips are numbered from 1; Aidoku lists newest first.
			let chapters = (1..=count)
				.rev()
				.map(|number| Chapter {
					key: number.to_string(),
					title: Some(format!("Выпуск {number}")),
					chapter_number: Some(number as f32),
					url: Some(format!("{BASE_URL}/~{}/{number}", manga.key)),
					language: Some("ru".into()),
					..Default::default()
				})
				.collect();
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{BASE_URL}/~{}/{}", manga.key, chapter.key);
		let html = Request::get(url)?.html()?;

		let Some(src) = html
			.select_first("section.reader-issue img.issue")
			.and_then(|img| img.attr("src"))
		else {
			bail!("Could not find the strip image");
		};

		Ok(Vec::from([Page {
			content: PageContent::url(absolute_url(&src)),
			..Default::default()
		}]))
	}
}

impl ListingProvider for AComics {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let sort = match listing.id.as_str() {
			"popular" => "subscr_count",
			"longest" => "issue_count",
			_ => "last_update",
		};
		self.fetch_cards(Self::catalog_url(page, sort, &[]))
	}
}

impl DeepLinkHandler for AComics {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		let Some(key) = slug_from_href(path) else {
			return Ok(None);
		};

		// A trailing number is a strip, i.e. a chapter.
		let strip = path
			.trim_start_matches('/')
			.trim_start_matches('~')
			.split('/')
			.nth(1)
			.filter(|segment| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()));

		Ok(Some(match strip {
			Some(strip) => DeepLinkResult::Chapter {
				manga_key: key,
				key: strip.to_string(),
			},
			None => DeepLinkResult::Manga { key },
		}))
	}
}

register_source!(AComics, ListingProvider, DeepLinkHandler);
