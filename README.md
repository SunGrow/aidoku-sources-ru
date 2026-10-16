# Russian sources for Aidoku

Aidoku sources for Russian manga, manhwa and comic sites, plus the source list
that installs them.

## Installing

Add this URL in Aidoku under **Settings → Source Lists**, then install sources
from the Browse tab:

```
https://sungrow.github.io/aidoku-sources-ru/index.min.json
```

Aidoku 0.7.1 or newer is required.

## Sources

| Source | Site | Notes |
|---|---|---|
| WaManga | [wamanga.ru](https://wamanga.ru) | JSON API. Search, filters (type, status, genre, year), listings, deep links. |
| AComics | [acomics.ru](https://acomics.ru) | Webcomics. Each strip is a single-page chapter. Search, genre/rating filters, listings, deep links. |

### Sites not implemented yet

These were attempted and are blocked by anti-bot measures. Every result below
was observed from a connection egressing in Singapore, so some are plausibly
geo- or IP-based and may behave differently from a Russian network — if a site
loads normally for you, it is worth retrying.

| Site | What happens |
|---|---|
| comx.life | Anti-adblock gate (`router.parklogic.com`) returns a JS interstitial to every HTTP client regardless of user agent. In a real browser it redirects to affiliate landing pages instead of content, so the page markup could not be observed. Note `comi-x.life` does not resolve; the live domain is `comx.life`. |
| readmanga / selfmanga / seimanga | Catalog HTML loads, but DDoS-Guard serves decoy images to untrusted clients: image requests return 77 bytes containing an unrelated Wikipedia URL rather than the page. Needs decoy detection plus mirror fallback. |
| remanga.org, mangalib.me | HTTP 403 on both the site and the API. |
| desu.me | Host unreachable. |

Implementing a source against markup that cannot be observed means guessing
selectors, and the two sources that are implemented both needed selectors that
differ from the public reference parsers — so guessing would likely ship
something broken.

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
