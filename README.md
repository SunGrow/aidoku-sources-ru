# Russian sources for Aidoku

Aidoku sources for Russian manga, manhwa and comic sites, plus the source list
that installs them.

## Installing

Add this URL in Aidoku under **Settings → Source Lists**, then install sources
from the Browse tab:

```
https://sungrow.github.io/aidoku-sources-ru/index.min.json
```

All four sources need Aidoku 0.9.0 or newer.

## Sources

| Source | Site | Login | Notes |
|---|---|---|---|
| WaManga | [wamanga.ru](https://wamanga.ru) | username + password | JSON API. Search, filters (type, status, genre, year), listings, home page, deep links. |
| AComics | [acomics.ru](https://acomics.ru) | web sign-in | Webcomics. Each strip is a single-page chapter. Search, genre/rating filters, listings, home page, deep links. |
| Com-X | [com-x.life](https://com-x.life) | web sign-in | Comics, manga, manhwa and manhua. Search, sorting, filters (section, genre, type, status, year, author, artist), listings, home page, deep links. |
| Remanga | [remanga.org](https://remanga.org) | — | JSON API. Search, sorting, filters (type, genre, category, status, translation status, age rating, year, rating, chapter count), listings, home page, similar titles, deep links. Served over the site's mirror. |

### Browsing

Each source opens on a home page in Aidoku's own layout: a large carousel
of the newest or most popular titles, then rows that each open the full
listing behind them. Com-X adds a row of genre chips, which drop straight
into the filtered catalog.

Com-X carries far more comics than manga, so its listings cover both:
**Комиксы** unions every western publisher section, with **Marvel** and
**DC** split out, alongside **Манга**, **Манхва** and **Маньхуа**.

Remanga has genre chips too, and its catalog is the most finely filtered
of the four: genres and categories can each be both required and excluded,
and the year, rating and chapter count are ranges.

Recommendations differ by site:

- **WaManga** has a recommendations API. Signed in, **Рекомендации** is
  built from your own reading (`/recommendations/for-you`); signed out it
  falls back to the site's public trending list, since the personalised
  endpoints answer 403 without a token. Both return the same shape, so
  the listing works either way.
- **AComics** has an editorially curated **Рекомендуемые** page, which the
  site links beside its catalog. It is a page of its own rather than a
  catalog sort, so it takes no filters, but it pages like the catalog.
- **Com-X** has none. There is no recommendations, similar-titles or
  related block anywhere on it — detail pages carry no such section, and
  the only occurrence of «рекомендую» on one is the share button's text.
  Nothing was added there rather than passing something else off as a
  recommendation.
- **Remanga** has no public recommendations listing — the one it does
  have, `/api/v2/users/my-recommendations/`, is personal and needs a
  token. What it has instead is per-title: a ranked **Похожее** list,
  which is appended to the description. For a long run that list leads
  with the rest of the run, so it doubles as the volume navigation the
  entries themselves lack.

### Accounts

All four sources work fully signed out — logging in is optional and only affects
what your account sees on the site itself (bookmarks, subscriptions, personal
recommendations). Sign in from the source's settings page in Aidoku.

- **WaManga** uses the site's JWT API (`POST /api/v1/auth/login`). Aidoku
  prompts for the username and password, and the returned access token is
  attached as a bearer token to later API requests.
- **AComics** uses Aidoku's web sign-in: it opens the site's own login page so
  the password is entered on the site rather than handled by the source, and
  the session cookie is reused for requests. Logging out clears those cookies.

  As on Com-X, the cookies the web view returns are stored and attached to
  later requests rather than left to the app. Anonymous browsing here sets
  no cookies at all, so that is the only jar the source keeps. The age gate
  has a cookie of its own that this header can displace, which is harmless:
  the reader already re-posts the gate whenever a strip arrives without its
  image, so the cost is one extra request on a rated comic.
- **Com-X** also uses web sign-in. The site has no standalone login page,
  so the setting opens the homepage — use the **Войти** button in the header.
  Being signed in is detected from the page itself rather than by guessing a
  cookie name.

  Com-X keeps the cookies the web view hands back and merges them with the
  challenge grant on every request, the login's winning where both carry a
  cookie of the same name. The two are stored apart so that re-solving the
  challenge cannot sign the reader back out. Earlier versions dropped the
  login cookies entirely and sent only the grant, which made a sign-in that
  plainly succeeded in the web view look as though it had never happened.
- **Remanga** has no sign-in here. The site authenticates with a bearer
  token rather than a cookie — `/api/v2/users/current/` answers
  `WWW-Authenticate: Bearer realm="api"` — so the web-view flow the other
  two use does not apply, and the token lives where a cookie jar cannot
  reach it. The username-and-password flow it would need instead goes
  through `/api/v2/users/login/`, which has a two-factor step beside it
  (`login/2fa/issue`, `login/2fa/verify`). None of that can be exercised
  without an account, so rather than ship a login that silently fails it
  was left out. Everything the source does works signed out.

### Sites not implemented yet

These were attempted and are blocked by anti-bot measures. Every result below
was observed from a connection egressing outside Russia, so some are
plausibly geo- or IP-based and may behave differently from a Russian network — if a site
loads normally for you, it is worth retrying.

Adding a login does not unblock any of them. MangaLib and the Grouple
sites are fronted by DDoS-Guard, which rejects the request at the edge:
`api.cdnlibs.org` answers `403` from `Server: ddos-guard` even for its own
`/api/auth/login` endpoint, so there is no way to authenticate past it.

| Site | What happens |
|---|---|
| readmanga / selfmanga / seimanga | Catalog HTML loads, but DDoS-Guard serves decoy images to untrusted clients: image requests return 77 bytes containing an unrelated Wikipedia URL rather than the page. Needs decoy detection plus mirror fallback. |
| mangalib.me | HTTP 403 on both the site and the API. |
| desu.me | Host unreachable. |

Implementing a source against markup that cannot be observed means guessing
selectors, and that guess is usually wrong: WaManga and AComics both needed
endpoints and selectors that differ from the public reference parsers, which
had gone stale. Com-X was the opposite — its reference selectors still
matched — but that only became checkable once the site could be loaded.

## iCloud sync

Nothing in a source switches this on or off. Aidoku mirrors its own Core
Data store with `NSPersistentCloudKitContainer`, gated by a single app
setting, and the source API has no sync trait or manifest field at all.

What a source does decide is whether the synced rows still resolve on the
other device. The library, chapter and history records are keyed on the
source id together with the manga and chapter keys, so those keys have to
mean the same thing everywhere and not drift between versions. All four
sources here derive them from the site's own identifiers — WaManga from the
API's ids, AComics from the permanent `~slug`, Com-X from the numeric id
the URL carries, Remanga from the title's `dir` — with no device, session
or host state mixed in.

Com-X titles get renamed, which changes the slug in their URL, but the key
survives it: the site resolves a title by the numeric prefix and redirects
to the current address, so a stale key still opens the right entry.

Remanga is the one that needed work. Its keys are slugs with no id-based
lookup behind them — `/api/v2/titles/38753/` is a 404 — and a title that
gets licensed has its slug rewritten with a dated marker, so
`omniscient-reader_` becomes `<29.04.2026>omniscient-reader_` and the old
form answers 404. Every library entry saved before that rename, on any
device, would be dead. Because the rewrite only prefixes the existing
slug, the source recovers from it: a title that no longer resolves is
searched for by its stored name and matched on a slug ending with the
stored one, which picks the renamed title and not its doujinshi.

## Building

Requires [Rust](https://rustup.rs) and the Aidoku CLI:

```bash
rustup target add wasm32-unknown-unknown
cargo install --git https://github.com/Aidoku/aidoku-rs aidoku-cli
```

Build every source and assemble the list into `public/`:

```bash
bash tools/build-all.sh
```

To work on a single source, run the CLI from its directory:

```bash
cd sources/ru.wamanga
aidoku package          # produces package.aix
aidoku verify package.aix
```

`aidoku serve sources/*/package.aix` hosts the list on your local network, which
is the quickest way to install a development build on a device.

Each source is a standalone crate with its own `target/` directory. This is
deliberate: `aidoku package` copies the first `.wasm` it finds in the build
directory, so sources sharing one target directory would pick up each other's
binaries. Don't set `CARGO_TARGET_DIR` when building.

Icons are generated originals (`python tools/make_icons.py`) rather than site
logos.

## Layout

```
sources/<source.id>/
├── Cargo.toml
├── res/
│   ├── source.json     metadata, listings, capabilities
│   ├── filters.json    search filters shown in the UI
│   └── icon.png        128x128, opaque
└── src/lib.rs          the source implementation
```

## Notes on site APIs

Site behaviour drifts, and the public reference parsers lag behind it. Where a
site's API was re-written, the implementation here follows what the live
endpoints actually return:

- **WaManga** exposes `/api/v1/recommendations` with `trending`, `new`,
  `for-you` and `similar/{mangaId}` beneath it, plus a `/manga/homepage`
  that returns six ready-made sections in one response. `for-you` and the
  bare `/recommendations` require a bearer token and answer 403 without
  one; `trending` and `new` are public. They all return the same bare
  array of titles the catalog does and take the same `limit` and `offset`,
  so they page without any special handling.
- **WaManga** serves an OpenAPI spec at `/api/docs-json`. The current API is
  `/api/v1/manga` (paginated with `offset`/`limit`, searched with `query`,
  array filters passed as repeated parameters). Image paths come back as
  `/app/uploads/...` and resolve against the site root with the `/app` prefix
  intact.
- **Com-X** (`com-x.life` — note the hyphen; `comi-x.life` and `comx.life`
  do not resolve) is DataLife Engine behind a Vue front end. The pages are
  server rendered, so parsing only ever looks at served markup: chapter and
  page lists come from the `window.__DATA__` object each page embeds, and
  chapter images are served from `img.com-x.life/comix/`. Reaching that
  markup is the hard part — see the challenge note below.

  Catalog filters are path segments, `/ComicList/g=17/page/2/`. A key takes
  several values as a comma separated list and the site returns the titles
  carrying all of them. The other separators are traps that fail silently:
  `g=17/g=76` keeps only the last value and `g=17;76` only the first, both
  without an error. The year filter is an exact year rather than a range —
  its comma form keeps only the first value too.

  Sorting is a form post rather than part of the URL, and the response to
  that post is itself the sorted page — it does not redirect. So a catalog
  request is a `POST` carrying `dlenewssortby` and `dledirection`, which
  the **Сортировка** filter sets. The two `set_*` fields name the slot the
  site stores the choice in; it ignores which slot is named when deciding
  what to return, but the form it ships always sends them.

  Stating the sort on every request is not optional. The site remembers the
  last one it was given, so a plain `GET` comes back in whatever order was
  asked for previously rather than in a known one.

  The bare `/ComicList/` is not the full catalog: it is a trimmed landing
  page of ten entries whose own pagination points elsewhere, so
  `/ComicList/page/2/` serves the first page again and scrolling never
  advances. The whole catalog is requested by naming every section instead,
  `p.cat=50,11,12,...`, which pages properly at twenty per request. Unlike
  the genre filter, `p.cat` unions its values rather than intersecting
  them, which is also what makes the combined Комиксы listing possible.

  Every page is behind a JavaScript challenge. A plain HTTP client asking
  for the catalog, a title or the reader is answered with a 302 to a
  `/_c?t=...` interstitial, which sets a cookie and serves a script that
  has to run before the real page appears. Retrying does not help: each
  attempt is issued a fresh challenge. Neither does replaying the cookie
  from the redirect, nor changing the user agent. The challenge is not
  IP-based — a browser on the same address passes it while curl does not.

  So the source loads pages in Aidoku's web view, which can run the script,
  and keeps the cookies that come out of it. Later requests go out as plain
  HTTP with those cookies attached and only fall back to the web view when
  they stop being accepted. `get_cookies` is a 0.9.0 API, which is what
  forces the 0.9.0 minimum.

  The interstitial is the page that finishes loading first, so the document
  has to be polled until the real one replaces it. That polling is done with
  `sleep` and the synchronous `eval` rather than by handing one promise to
  `eval_async`: on device `eval_async` returned no result at all, which the
  CLI's own version table hints at when it notes the call crashed before
  0.9. If the document cannot be read out of the web view, the source still
  retries the page over plain HTTP with the cookies the challenge issued.

  The page is loaded with `load` and never `load_blocking`. Waiting for the
  load to finish can never return, because the interstitial navigates away
  as soon as its script runs and the wait is left pending. That hung the
  listing indefinitely on an iPad while the same build worked on a phone,
  so nothing in the fetch path is allowed to block without a bound: the
  poll is the only wait, and it always ends in a page or an error.

  One endpoint escapes the challenge — `/engine/ajax/controller.php` with
  `mod=reader&method=reader/chapters&news_id=<id>` returns chapter JSON over
  plain HTTP — but it only yields chapter ids and titles, with no catalog,
  detail or page images, so it is not enough to build on.

  Each volume of a run is its own title, with near-identical names and
  nothing in the entry to say which volume follows which. The site keeps
  that in a «Связанное» panel on comics pages — manga pages have no such
  section — beside a «С этим читают» panel, which is a recommendation
  strip rather than part of the run, so only the open panel is read.
  Aidoku's `Manga` has no field for related titles, so they are appended
  to the description. That is the only place they can be shown at all.

  For the same reason the year and the publisher are put at the front of
  the tag list: they are what distinguishes a 1963 run from a 2018 one
  when both are called «Amazing Spider-Man».

  Those rows sit in a `<ul class="page__list">`. The source looked for
  them under `div.page__list`, which matches nothing and read as a page
  with no rows rather than as an error, so every title went without an
  author, an artist or a status until this was corrected.

  The challenge also re-triggers after bursts of requests, which is why the
  source caps parallel requests at two.

  Cover and page images need a `Referer`. Aidoku requests images with no
  headers at all by default, and the image hosts answer a request without
  one with an intermittent 403 — a 1.5KB HTML body in place of the image.
  That shows up as a grid where some covers are missing and others are
  not, differing from one load to the next, rather than as a clean
  failure. A `Referer` alone was enough; a user agent on its own was not,
  and no cookies are needed. The source supplies it through
  `ImageRequestProvider`. Being intermittent, the 403 is hard to pin to a
  single trigger, so treat the header as the mitigation it is.

  Cover resolution is capped by the site. Listings carry only a 130x196
  thumbnail under `/uploads/mini/mid/`, and no larger variant of that path
  exists — `mini/big`, `mini/small` and the rest all 404. The detail page
  has a 268x400 poster under `/uploads/posts/`, which is also what
  `og:image` points at, so that is the largest cover the site holds. The
  two paths share no common form, so the poster cannot be derived from the
  thumbnail: entries show the thumbnail until details are loaded, which is
  what swaps in the larger one.

- **Remanga** has two generations of its API answering side by side, and only
  the newer one is correct. The source uses `/api/v2/...` throughout, which is
  what the site's own pages call. The older `/api/search/catalog/` still
  responds, but it drops every row the moment a status is named — `status=2`
  returns zero items even though titles plainly carry that status, and every
  other value does the same — and it unions repeated genres where the site
  intersects them. Neither failure reports itself, so an implementation built
  against it looks like it works until someone opens the filter sheet.

  The main domain is geo-blocked: `remanga.org` answers 403 outside the CIS,
  while the punycode form of `реманга.орг` serves the full API to a bare
  request with no headers, cookies or token. That mirror is therefore the
  default, with the main domain offered beside it through the app's base-url
  picker, so a connection that prefers the other one needs no new build.

  Filters are repeated parameters with no brackets — `genres=3` twice, never
  `genres[]=3`, which is accepted and then ignored. What repeating one means
  is not uniform, and the filter sheet is built to match what each actually
  does:

  | Parameter | Repeated |
  |---|---|
  | `genres`, `categories` | intersect, so the sheet offers them as multi-select |
  | `types` | union |
  | `exclude_genres`, `exclude_categories`, `exclude_types` | all are excluded |
  | `status`, `translate_status`, `age_limit` | only the last value counts, so these are single-choice selects |

  Ranges are a `_gte`/`_lte` suffix on `issue_year`, `rating` and
  `count_chapters`. The double-underscore spelling Django uses internally is
  ignored, as is a bare `issue_year=2020`.

  Search and filtering are separate endpoints that ignore each other's
  parameters: `/api/v2/search/?query=` takes no filters, and the catalog
  ignores `query` outright rather than erroring. The manifest hides the filter
  sheet while a query is active so the two never appear to combine. They page
  differently too — the catalog returns a `next` cursor, search a
  `total_pages` count.

  The two page-size caps differ, and only one of them complains. Catalog and
  search reject `count` above 30 with a validation error; the chapter endpoint
  silently clamps to 50, so asking for 1000 quietly yields 50.

  Chapters hang off a *branch* — one translation team's run — rather than the
  title, so a branch has to be chosen before they can be listed at all. Nearly
  every title has exactly one; where there are several they are rival
  translations of the same work, so the fullest is taken. They arrive newest
  first, which is the order Aidoku wants, and are walked to the end through
  the cursor.

  Chapter pages are a list of *slides*, each holding several images, rather
  than a flat list, so `pages` is flattened before it means anything. Page
  images are served absolute from a separate host and, unlike Com-X, need no
  `Referer`: covers and pages both load from a request carrying no headers.

  Creator roles are unlabelled integers with no vocabulary endpoint to resolve
  them, so they were read off titles whose credits are known independently —
  on *The Breaker* the writer is type 1 and the artist type 4. Sampling the
  rest showed type 2 to be art studios and type 3 to be publishers and
  platforms rather than people, so only 1, 2 and 4 become authors and artists.

  Catalog rows carry none of the fields a content rating comes from, so a
  listing yields `Unknown` rather than a guessed `Safe`; the real value
  arrives with the details.

