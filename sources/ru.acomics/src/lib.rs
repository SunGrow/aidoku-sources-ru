#![no_std]
//! Aidoku source for acomics.ru.
//!
//! AComics hosts webcomics published one strip at a time, so each strip is
//! exposed as a single-page chapter rather than trying to group them.

use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, HashMap, Home,
	HomeComponent, HomeComponentValue, HomeLayout, Link, LinkValue, Listing, ListingKind,
	ListingProvider, Manga, MangaPageResult, Page, PageContent, Result, Source, UpdateStrategy,
	Viewer, WebLoginHandler,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{
		defaults::{DefaultValue, defaults_get, defaults_set},
		html::Document,
		net::Request,
	},
	prelude::*,
};

const BASE_URL: &str = "https://acomics.ru";

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

/// Where the cookies the web view login produced are kept.
///
/// Anonymous browsing sets no cookies at all here, so this is the only
/// jar the source has to maintain. Attaching it by hand rather than
/// leaving it to the app is what makes a sign-in actually reach the
/// site: the cookies arrive as an argument to the login handler and are
/// otherwise thrown away.
const LOGIN_COOKIE_KEY: &str = "login.cookies";

fn stored_login() -> Option<String> {
	defaults_get::<String>(LOGIN_COOKIE_KEY).filter(|value| !value.is_empty())
}

/// Attaches the stored login, when there is one.
///
/// The age gate's own cookie is not tracked here and may be displaced by
/// this header. That is harmless: the reader already re-posts the gate
/// whenever a strip comes back without its image, so losing the cookie
/// costs one extra request on a rated comic rather than failing it.
fn with_cookies(request: Request) -> Request {
	match stored_login() {
		Some(cookies) => request.header("Cookie", cookies.as_str()),
		None => request,
	}
}

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

fn strip_image(html: &Document) -> Option<String> {
	html.select_first("section.reader-issue img.issue")
		.and_then(|img| img.attr("src"))
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
		let html = with_cookies(Request::get(url)?).html()?;
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

	/// The site's own recommended list, which it links as
	/// «Рекомендуемые» beside the catalog. It is a separate page rather
	/// than a catalog sort, and takes neither sort nor filters, but it
	/// pages with the same `skip` and serves the same cards.
	fn featured_url(page: i32) -> String {
		let skip = (page.max(1) - 1) * PAGE_SIZE;
		format!("{BASE_URL}/featured?skip={skip}")
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
		let html = with_cookies(Request::get(format!("{BASE_URL}/~{}/about", manga.key))?).html()?;

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

			// Ratings are NR, G, PG, PG-13, R and NC-17. These are matched
			// exactly: a substring test would read the "R" in "NR" (not rated)
			// as an adult rating.
			manga.content_rating = match labelled_value(&html, "Возрастной рейтинг:")
				.as_deref()
				.map(str::trim)
			{
				Some("NC-17") => ContentRating::NSFW,
				Some("R") | Some("PG-13") => ContentRating::Suggestive,
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

		let src = match strip_image(&with_cookies(Request::get(&url)?).html()?) {
			Some(src) => src,
			None => {
				// Comics rated R or NC-17 serve an age gate in place of the
				// strip. Confirming it posts the same form the gate displays,
				// which returns the strip directly and sets a cookie so later
				// requests skip the gate.
				let confirmed = with_cookies(Request::post(&url)?)
					.header("Content-Type", "application/x-www-form-urlencoded")
					.body("ageRestrict=17")
					.html()?;

				let Some(src) = strip_image(&confirmed) else {
					bail!("Could not find the strip image");
				};
				src
			}
		};

		Ok(Vec::from([Page {
			content: PageContent::url(absolute_url(&src)),
			..Default::default()
		}]))
	}
}

impl ListingProvider for AComics {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		if listing.id == "featured" {
			return self.fetch_cards(Self::featured_url(page));
		}

		let sort = match listing.id.as_str() {
			"popular" => "subscr_count",
			"longest" => "issue_count",
			_ => "last_update",
		};
		self.fetch_cards(Self::catalog_url(page, sort, &[]))
	}
}

impl Home for AComics {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut components = Vec::new();

		// Plain catalog requests, none of them gated, so the page is
		// built from the listings the source already offers. A row that
		// fails is left out rather than failing the whole page.
		for (title, id, url, big) in [
			(
				"Популярное",
				"popular",
				Self::catalog_url(1, "subscr_count", &[]),
				true,
			),
			("Рекомендуемые", "featured", Self::featured_url(1), false),
			(
				"Обновления",
				"latest",
				Self::catalog_url(1, "last_update", &[]),
				false,
			),
			(
				"Самые длинные",
				"longest",
				Self::catalog_url(1, "issue_count", &[]),
				false,
			),
		] {
			let Ok(result) = self.fetch_cards(url) else {
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

impl WebLoginHandler for AComics {
	fn handle_web_login(&self, _key: String, cookies: HashMap<String, String>) -> Result<bool> {
		// These are what the web view came back with, and keeping them is the
		// whole point: dropped, the source goes on asking for every page as a
		// guest no matter how the sign-in went.
		if !cookies.is_empty() {
			let mut header = String::new();
			for (name, value) in cookies.iter() {
				if !header.is_empty() {
					header.push_str("; ");
				}
				header.push_str(name);
				header.push('=');
				header.push_str(value);
			}
			defaults_set(LOGIN_COOKIE_KEY, DefaultValue::String(header));
		}

		// The session cookie's name isn't documented, so rather than guessing
		// at one, ask the site: the header carries the login form only while
		// signed out. This goes out with the cookies just stored, so it
		// reports what the source will get from here on rather than what the
		// web view alone can see.
		let html = with_cookies(Request::get(format!("{BASE_URL}/comics"))?).html()?;
		let logged_in = html
			.select_first("form[action=\"/action/guest/login\"]")
			.is_none();

		// Either the sign-in did not take or this call is a logout; the two
		// cannot be told apart here, and in both cases the stored cookies are
		// not worth sending again.
		if !logged_in {
			defaults_set(LOGIN_COOKIE_KEY, DefaultValue::String(String::new()));
		}

		Ok(logged_in)
	}
}

register_source!(AComics, ListingProvider, DeepLinkHandler, WebLoginHandler, Home);
