# Russian sources for Aidoku

Aidoku sources for Russian manga, manhwa and comic sites, plus the source list
that installs them.

## Installing

Add this URL in Aidoku under **Settings → Source Lists**, then install sources
from the Browse tab:

```
https://sungrow.github.io/aidoku-sources-ru/index.min.json
```

All six sources need Aidoku 0.9.0 or newer.

## Sources

| Source | Site | Login | Notes |
|---|---|---|---|
| WaManga | [wamanga.ru](https://wamanga.ru) | username + password | JSON API. Search, filters (type, status, genre, year), listings, home page, deep links. |
| AComics | [acomics.ru](https://acomics.ru) | web sign-in | Webcomics. Each strip is a single-page chapter. Search, genre/rating filters, listings, home page, deep links. |
| Com-X | [com-x.life](https://com-x.life) | web sign-in | Comics, manga, manhwa and manhua. Search, sorting, filters (section, genre, type, status, year, author, artist), listings, home page, deep links. |
| Remanga | [remanga.org](https://remanga.org) | — | JSON API. Search, sorting, filters (type, genre, category, status, translation status, age rating, year, rating, chapter count), listings, home page, similar titles, deep links. Served over the site's mirror. |
| ReadManga | [readmanga.me](https://readmanga.me) | — | JSON catalogue, HTML titles. Search combined with filters (category, genre, features, age rating, production and translation status, year), listings, home page, deep links to a title or a chapter. Mirrored at `a.zazaza.me`. |
| MangaLib | [mangalib.org](https://mangalib.org) | — | JSON API. Search combined with filters (type, status, translation status, format, genre, tag, year, rating, chapter count), listings, home page, related and similar titles, rival translations as separate chapters, deep links. |
| SelfManga | [1.selfmanga.live](https://1.selfmanga.live) | — | Self-published Russian manga, 3,942 titles. Same engine and the same feature set as ReadManga, against a library of its own. |
| SeiManga | [1.seimanga.me](https://1.seimanga.me) | — | Seinen and josei, 6,281 titles. Same engine and the same feature set as ReadManga, against a library of its own. |

### Browsing

Each source opens on a home page in Aidoku's own layout: a large carousel
of the newest or most popular titles, then rows that each open the full
listing behind them. Com-X adds a row of genre chips, which drop straight
into the filtered catalog.

Com-X carries far more comics than manga, so its listings cover both:
**Комиксы** unions every western publisher section, with **Marvel** and
**DC** split out, alongside **Манга**, **Манхва** and **Маньхуа**.

Remanga has genre chips too, and its catalog is the most finely filtered
of the five: genres and categories can each be both required and excluded,
and the year, rating and chapter count are ranges.

MangaLib has genre chips as well, and is the one source where a search and
the filter sheet combine: typing a query narrows the filtered set rather
than replacing it, so the sheet stays on screen while searching. It is also
the only one that lists rival translations side by side — a chapter
translated by three teams appears three times, each labelled with its team,
and opening one reads that team's pages.

ReadManga has genre chips too, and the widest filter sheet here: its
categories, genres, features and age ratings are one shared vocabulary, so
any mixture of them can be required or excluded together, alongside the
production status, the translation status and a year range. Its chapter
links carry the translator, so a long run shows which team did which part.

SelfManga and SeiManga browse exactly like it, each over its own library.
Nothing is shared between the three but the engine: every filter id, genre
list and year bound in those two sources was read from that site's own
filter sheet, because the same format is numbered differently on each.
SelfManga publishes no age ratings at all, so its sheet has no such control
and its titles report an unknown rating rather than a guessed one.

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
- **MangaLib** has two per-title lists rather than a recommendations
  listing, and both are appended to the description. `/relations` is the
  one that answers what a long run raises — it names the sequel, the
  prequel, the colour version and the spin-offs, each with its relation
  spelled out — and `/similar` is the looser "readers also read" list.
- **ReadManga** has no recommendations listing, but its title pages carry a
  related block, which is appended to the description. It names the
  prequel, the spin-off and the colour edition with the relation spelled
  out, so it doubles as the volume navigation a long run otherwise lacks.
  Entries pointing at the network's anime, drama and novel sites are left
  out, since a manga reader cannot open them.

### Accounts

All six sources work fully signed out — logging in is optional and only affects
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
- **MangaLib** has no sign-in here either. Everything this source touches —
  catalogue, search, filters, details, chapters and pages — answers in full
  to an unauthenticated request, and what an account adds (bookmarks,
  reading history, personal recommendations) is not used. How the site
  authenticates was not established: `/auth/me` redirects rather than
  challenging, and the front end is a shell that reveals nothing, so there
  was no mechanism to implement against and no account to test one with.
- **ReadManga, SelfManga and SeiManga** have no sign-in here. Accounts on
  this network live on a host of their own, `grouple.co`, shared across its
  sites, and what they add — bookmarks, reading history, comments — is not
  used by these sources. Everything they do touch answers to an
  unauthenticated request carrying nothing but a browser user agent.

### Sites not implemented yet

Every result below was observed from a connection egressing outside Russia.

This section once said DDoS-Guard rejected MangaLib and the Grouple sites
at the edge, with no way past it. That was wrong, and wrong in a way worth
recording: a bare `curl` was treated as the whole story. Loading the same
sites in a browser and reading what their own pages requested showed both
answering normally, and both are sources above. What is left here is one
dead domain and three libraries that are simply not built yet.

| Site | State | What happens |
|---|---|---|
| SelfLib | reachable, out of scope | A Grouple site, but a prose library rather than a comics one. See below. |
| desu.me | gone | Not blocked — the domain publishes no address at all. See below. |

**SelfLib.** `1.selflib.me` runs the same engine as the three Grouple
sources above and answers the same `/api/catalog/search`, which is why it
was listed here as a sibling waiting to be built. Opening a chapter showed
why it cannot be: it is a самиздат **prose** library. A chapter is sixty-odd
paragraphs of text in a `chapter-content` block with no page images anywhere
in it, and what the catalogue calls categories are audience labels — «для
подростков», «научная проза» — rather than comic formats. Aidoku reads
pages, so there is nothing here for a source to return. Its 1,465 titles are
not comics and are not missing from the list by oversight.

`mintmanga` and `usagi.one` answer on no prefix that was tried and are not
in the network's own site switcher, which lists only ReadManga, SelfManga
and SelfLib.

**desu.me.** This one is not blocked and not moved. The domain is delegated
to Cloudflare's nameservers but publishes no `A`, `AAAA` or `CNAME` record at
all, so there is nothing to connect to. The similarly named domains are not
it either: `desu.win` and `desu.city` are parked and bounce through an
advertising redirect, and `desu.su` is openly for sale. Nothing to implement
against.

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
mean the same thing everywhere and not drift between versions. All six
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

  What the grant expiring looks like, confirmed on a device: a comic's
  details spin without ever arriving, its chapter list comes up empty, and
  the home page's covers crawl. The last of those is a consequence rather
  than a separate fault — the source caps itself at two parallel requests,
  so a stalled page load starves the image queue behind it. None of it is
  a parsing fault, and chasing it as one wasted a version: without the
  grant every request, whatever its user agent and whether or not it
  follows redirects, lands on the `/_c` gate with a 404 and no payload,
  while the same request carrying the grant returns the page whole. The
  gate itself is a proof-of-work — `SHA-256(token + ":" + nonce)` until
  the hash starts with `00`, posted to `/_v` — and it also reads signals
  meant to spot automation, which is reason enough to leave it to the web
  view that runs the site's own script rather than reimplementing it here.

  The grant is refreshed by opening the source's web view sign-in once,
  which clears the gate in a visible browser and stores the cookies; the
  plain-HTTP path works again immediately afterwards. Since a background
  web view cannot be relied on to clear it, the error says that in as many
  words rather than leaving an empty chapter list to be puzzled over.

  One endpoint escapes the challenge — `/engine/ajax/controller.php` with
  `mod=reader&method=reader/chapters&news_id=<id>` returns chapter JSON over
  plain HTTP — but it only yields chapter ids and titles, with no catalog,
  detail or page images, so it is not enough to build on.

  The year and the publisher are put at the front of the tag list: each
  volume of a run is its own title under a near-identical name, and those
  two are what distinguish a 1963 run from a 2018 one when both are called
  «Amazing Spider-Man».

  Those rows sit in a `<ul class="page__list">`. The source looked for
  them under `div.page__list`, which matches nothing and read as a page
  with no rows rather than as an error, so every title went without an
  author, an artist or a status until this was corrected. All of the rows
  are now read in a single pass rather than one document query per label.

  Version 7 also claimed to list the rest of a run from a «Связанное»
  panel. There is no such panel: `page__related` appears nowhere in the
  HTML the site serves, checked on six pages, four of them comics. The
  selector was written from an assumption rather than from the markup, so
  it never matched and the feature never did anything. It is gone, and
  Aidoku still has no field for related titles.

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

- **MangaLib** (`mangalib.org`; `mangalib.me` answers 403 with DDoS-Guard's
  geoblock page) serves its pages from one host and its data from another,
  `api.cdnlibs.org`. That API answers 403 to a request carrying no
  `Referer`, and the details endpoint answers 404 with a silent "Not Found"
  toast to one carrying no `Site-Id: 1`, so both headers are on every
  request. Covers on `cover.cdnlibs.org` need the `Referer` too, which is
  the Com-X situation and the same fix.

  The same API serves four sibling libraries — manga is site 1, ranobe 3,
  and two adult ones are 2 and 4 — told apart by `site_id[]`. Every entry
  in the filter vocabulary carries the `site_ids` it belongs to, so the
  sheet is built from the entries containing 1 and never offers a tag that
  cannot match. The vocabulary is inconsistent about how it labels itself:
  types, statuses and translation statuses use `label`, while genres, tags
  and formats use `name`.

  Several filter parameters are accepted and then silently ignored rather
  than rejected, which makes them worse than an error: `exclude_genres`,
  `genres_exclusion`, `types_exclude` and `status_exclude` all return the
  unfiltered catalogue, as does every spelling of an age filter that was
  tried (`caution`, `age_restriction`, `ageRestriction`, `caution_id`,
  `restriction`, `age_restriction_id`, `cautions`, each as both an array
  and a scalar). Only parameters checked against the rows they returned are
  shipped, which is why exclusion is offered for genres and tags alone and
  why there is no age filter at all. Repeating a value does not mean one
  thing either: `genres` and `tags` intersect, while `types`, `status`,
  `scanlate_status` and `format` union.

  Search is the same endpoint as the catalogue with a `q` added, so it
  combines with the filters and the sort instead of replacing them —
  `hidesFiltersWhileSearching` is off for that reason.

  `summary` is not a string. It is a ProseMirror document, a tree of nodes
  under `{"type": "doc", "content": [...]}`, which has to be walked and
  flattened; older entries are still plain strings, so both are accepted.

  Chapters arrive in one response, oldest first, and each row carries one
  entry per translation. Those are rival translations of the same chapter
  rather than different parts of it, so each becomes its own chapter with
  its team named. Pages are addressed by volume and number rather than by
  chapter id, with `branch_id` picking the translation — the chapter row's
  own `id` is not interchangeable with it, and `bid` is ignored.

  Page urls come back as host-relative fragments to hang off an image
  server. `/constants?fields[]=imageServers` names several: the "main" one,
  `img2.imglib.info`, answers with AVIF whatever `Accept` asks for, while
  the "compress" and "download" server, `img3.cdnlibs.org`, serves the
  identical image — same pixel dimensions, same source — as JPEG and
  without needing a `Referer`. The JPEG host is used, because a format
  every reader can decode is worth more here than the third off the
  transfer.

- **ReadManga** (`readmanga.me`, mirrored at `a.zazaza.me`: the two report
  31,198 and 31,200 titles and their first catalogue page agreed on 17 of
  18) wants one thing and nothing else: a
  browser `User-Agent`. Without one every url on it, API and page alike,
  answers `402` with a 24-byte `NOT FOUND IN THE ADDRESS` body, which reads
  like a hard block and is only a filter on non-browser clients. No cookies,
  no `Referer`, no sign-in.

  Its catalogue is JSON — `/api/catalog/search`, the endpoint the site's own
  filter sheet calls — while the titles and chapters around it are
  server-rendered HTML with no detail endpoint to ask instead. The catalogue
  pages with `offset`, answers 50 rows, and reports a real `total`.

  Guessing its filter parameters was a waste of fifteen requests: it accepts
  and silently ignores every name that is not its own, `categories`,
  `genres`, `tags`, `el_3001=in` and the rest alike. The real ones are in the
  code it ships. Categories, genres, features and age ratings are one shared
  id space addressed by `includeElementIds` / `excludeElementIds`, repeating
  which intersects; the statuses take `includeProductionStatuses` and
  `includeTranslationStatuses`; the site's canned filters take
  `includeSearchFilter`; and the year range is one `years=min,max` parameter,
  which is why an open end of the range filter is completed with the site's
  own bound rather than left out. The vocabulary for all of them ships in the
  catalogue page as `__FILTERS.<key>` assignments, which is what generates
  `filters.json`. An unknown `sortType` is read as `RATING` rather than
  rejected.

  The page images are the reason this source did not exist until now. The
  reader is handed its pages as `readerInit(chapterInfo, [[host, dir,
  "path?t=…&u=…&h=…", width, height, ''], …])`, and fetching one of those
  urls *with* its signed query returns `300` and a 77-byte body holding a
  decoy url pointing at an unrelated photograph on Wikimedia. The earlier
  conclusion here — that the signature was the obstacle and the source could
  not get past it — had it backwards. The signature is the trap. The site's
  own reader never sends it: it strips the query before assigning `img.src`,
  which is visible in the DOM of a rendered chapter, where every `img.src`
  is the bare path. The same url without the query answers `200` with the
  real page and needs no headers at all. Checked across three titles and
  five chapters, the images returned match the dimensions the payload
  declares.

  Chapters come from the title page's table. A row's `data-num` is the
  chapter number times ten, so the number is read from the link instead,
  where it is written plainly and keeps halves like `99.5` intact. The
  link's `title` attribute is the translating team, which becomes the
  chapter's scanlator; its text repeats the volume and chapter before any
  name, so the leading digits are trimmed off. A licensed title keeps its
  chapter list and loses its pages, which is the one way a chapter that
  opened yesterday comes back empty.

  One more thing the reader wants: `?mtr=1` on the chapter url. It is the
  site's own age gate being answered, and without it a chapter of any of
  the 1,232 titles flagged `MATURE` renders the warning instead of the
  reader, so the payload this parses is simply absent. A cookie does not
  substitute for it. The title page is unaffected, so only the reader asks.

- **SelfManga and SeiManga** (`1.selfmanga.live`, `1.seimanga.me`) are the
  same engine as ReadManga and behave the same way, down to the `402` on a
  missing user agent and the signed page urls that work only once the
  signature is stripped. Three things are not shared.

  Element ids are per-site. The same category, genre or status is numbered
  differently on each, so nothing was carried over: every id in those two
  sources, and both year bounds, was read from that site's own `__FILTERS`
  block. SelfManga ships no `limitation` vocabulary at all, so it has no
  age-rating filter and its titles report `Unknown` rather than a guess.

  SeiManga's image host checks the `Referer`. `sei.one-way.work` answers a
  request without one with `403` and an HTML body — covers included, which
  would have left the grid empty — while `rm.one-way.work` and
  `sm.one-way.work` serve the same urls to anyone. The header costs nothing
  on the other two, so all three sources send it.

  Takedowns are more common on the siblings than on ReadManga, and the
  payload shows them plainly: every entry of such a chapter carries an
  empty host and the path `/static/deleted1.png`. Both top-rated titles —
  `no_way` and `tokiiskie_mstiteli` — are in that state, which is a fair
  warning against sampling one title and calling the source broken. Out of
  eight randomly drawn chapters per site, seven and eight respectively had
  real pages. The empty host is resolved against the base url so the reader
  gets the placeholder the site itself serves rather than a url nothing can
  fetch.
