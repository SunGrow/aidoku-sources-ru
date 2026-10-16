# Russian sources for Aidoku

Aidoku sources for Russian manga, manhwa and comic sites, plus the source list
that installs them.

## Installing

Add this URL in Aidoku under **Settings → Source Lists**, then install sources
from the Browse tab:

```
https://sungrow.github.io/aidoku-sources-ru/index.min.json
```

Aidoku 0.7.1 or newer is required, except Com-X, which needs 0.9.0 — it
relies on the web view API to get past the site's JavaScript challenge.

## Sources

| Source | Site | Login | Notes |
|---|---|---|---|
| WaManga | [wamanga.ru](https://wamanga.ru) | username + password | JSON API. Search, filters (type, status, genre, year), listings, deep links. |
| AComics | [acomics.ru](https://acomics.ru) | web sign-in | Webcomics. Each strip is a single-page chapter. Search, genre/rating filters, listings, deep links. |
| Com-X | [com-x.life](https://com-x.life) | web sign-in | Comics, manga, manhwa and manhua. Search, filters (section, genre, type, status, year, author, artist), listings, deep links. Needs Aidoku 0.9.0. |

### Accounts

All three sources work fully signed out — logging in is optional and only affects
what your account sees on the site itself (bookmarks, subscriptions, personal
recommendations). Sign in from the source's settings page in Aidoku.

- **WaManga** uses the site's JWT API (`POST /api/v1/auth/login`). Aidoku
  prompts for the username and password, and the returned access token is
  attached as a bearer token to later API requests.
- **AComics** uses Aidoku's web sign-in: it opens the site's own login page so
  the password is entered on the site rather than handled by the source, and
  the session cookie is reused for requests. Logging out clears those cookies.
- **Com-X** also uses web sign-in. The site has no standalone login page,
  so the setting opens the homepage — use the **Войти** button in the header.
  Being signed in is detected from the page itself rather than by guessing a
  cookie name.

### Sites not implemented yet

These were attempted and are blocked by anti-bot measures. Every result below
was observed from a connection egressing outside Russia, so some are
plausibly geo- or IP-based and may behave differently from a Russian network — if a site
loads normally for you, it is worth retrying.

Adding a login does not unblock any of them. Remanga, MangaLib and the Grouple
sites are all fronted by DDoS-Guard, which rejects the request at the edge:
`api.cdnlibs.org` answers `403` from `Server: ddos-guard` even for its own
`/api/auth/login` endpoint, so there is no way to authenticate past it.

| Site | What happens |
|---|---|
| readmanga / selfmanga / seimanga | Catalog HTML loads, but DDoS-Guard serves decoy images to untrusted clients: image requests return 77 bytes containing an unrelated Wikipedia URL rather than the page. Needs decoy detection plus mirror fallback. |
| remanga.org, mangalib.me | HTTP 403 on both the site and the API. |
| desu.me | Host unreachable. |

Implementing a source against markup that cannot be observed means guessing
selectors, and that guess is usually wrong: WaManga and AComics both needed
endpoints and selectors that differ from the public reference parsers, which
had gone stale. Com-X was the opposite — its reference selectors still
matched — but that only became checkable once the site could be loaded.

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

  Sorting is not exposed. The site changes it with a form post that stores
  the choice in the session instead of putting it in the URL, and that post
  redirects away from the catalog, so there is nothing a stateless source
  can use. The listings select sections instead.

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
  they stop being accepted. This is what forces the 0.9.0 minimum, since
  `eval_async` and `get_cookies` are 0.9.0 APIs.

  One endpoint escapes the challenge — `/engine/ajax/controller.php` with
  `mod=reader&method=reader/chapters&news_id=<id>` returns chapter JSON over
  plain HTTP — but it only yields chapter ids and titles, with no catalog,
  detail or page images, so it is not enough to build on.

  The challenge also re-triggers after bursts of requests, which is why the
  source caps parallel requests at two.
