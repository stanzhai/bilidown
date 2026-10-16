# Bili23 Downloader Rust Rewrite

Status: ready-for-agent

Rewrite of the entire application in Rust with a gpui-kit UI. All features
unchanged; screen layout, information architecture and interaction flows
unchanged; visual style switches to gpui-kit's native look. The Rust build
is a drop-in replacement reading/writing the same user-data directory and
formats as the Python version.

Grounding: this spec was produced from a full codebase inventory
(src/, test/, launcher/, scripts/, assets/, .github/) plus a gpui-kit
(0.7.x) and upstream GPUI investigation.

## Problem Statement

The application is a ~25-30k line Python/PySide6 desktop app. It requires
bundling a Python runtime, distribution depends on a Windows-specific
C++ launcher (PyStand fork), and the runtime needs to be dragged onto every
user machine. The user wants the same product, same features, same UI
design, delivered as a native cross-platform (Windows / macOS / Linux)
application.

## Solution

A Rust rewrite in this same repository under `crates/`, UI built with
gpui-kit (Longbridge's GPUI desktop framework):

- **Functional parity by checklist**: every feature in the Implementation
  Decisions list below stays. The parity checklist is derived from it.
- **Layout/interaction parity, native visuals**: the existing screens,
  navigation structure, and control placement/behavior are preserved one to
  one; the widgets themselves render in gpui-kit's own style (shadcn-like),
  not a pixel clone of Fluent. Mica/acrylic materials are dropped (GPUI has
  no such capability).
- **User data drop-in compatible**: first Rust run reads the existing
  `Bili23 Downloader/` user-data directory (config.json + task.db +
  thumbnail.db + history.db) in place, same schemas and formats, including
  running config migrations on `config_version`. The two versions can be
  used interchangeably during the transition.
- **Platform matrix**: Windows 10 1809+ / macOS 12+ / Linux x64 (X11 or
  Wayland). Win7/old-Win10 support is dropped. CI must go green on all
  three platforms before a release; three-platform CI green is a release
  gate.
- **Product identity unchanged**: same name, same app id, continues the
  v2.21.0 version line, same Verhub update channel key
  (`scottsloan-bili23-downloader`). Update checks call the Verhub HTTP API
  directly (Rust client written against the actual API surface).
- **Clean cutover**: the Python source, launcher and PyStand stay
  releaseable until the parity checklist is fully green; after that they
  are deleted in one pass.

## User Stories

Actors: **用户** (end user of the downloader), **匿名用户** (no login),
**登录用户** (logged-in), **开发者/贡献者**.

1. As a 用户, I want to paste any Bilibili content URL (av/BV/au/am/b23.tv
   short link/season page/space page/favorites/watch-later/history/weekly
   popular/dynamic/cheese/mall lesson), so that I can see its episodes
   listed without remembering URL formats.
2. As a 用户, I want the parser to auto-detect the URL type, so that I
   never have to pick a mode manually.
3. As a 用户, I want multi-P videos, collections, and seasons to expand
   into a checkable tree, so that I can select exactly the parts I want.
4. As a 用户, I want a batch parse dialog accepting a multi-line list of
   av/BV ids, so that I can enqueue many videos at once.
5. As a 用户, I want server-side keyword search within space/favorites/
   watch-later/history lists, so that I can find items in long lists
   without client-side paging through everything.
6. As a 用户, I want the parsed list pager plus in-page search with match
   jumping, so that I can navigate long parse results.
7. As a 用户, I want batch select by episode-number ranges, so that I can
   pick e.g. episodes 3-7 quickly.
8. As a 用户, I want interactive videos traversed by their stein graph
   with node selection, so that I can download a chosen path through an
   interactive video.
9. As a 用户, I want to preview actual stream qualities, codecs, bitrates
   and sizes before downloading, so that I know what I will get.
10. As a 用户, I want to set video quality priority (16→127 incl. 8K/
    Dolby Vision/HDR/4K), audio priority (30216→30251), and codec priority
    (AVC/HEVC/AV1), so that downloads pick the best available within my
    preference.
11. As a 用户, I want a warning when my preferred codec falls back to
    another, so that I am never surprised by what landed on disk.
12. As a 用户, I want video+audio merged DASH downloads (and legacy FLV
    multi-part concat), so that I get a single playable file.
13. As a 用户, I want m4a→mp3 conversion (libmp3lame), so that audio
    downloads play everywhere.
14. As a 用户, I want mp4 or mkv container choice with cover attached as
    attached_pic and optional delete-after, so that my library looks
    right.
15. As a 用户, I want danmaku downloaded as xml/ass/json, so that I can
    use it in players or archive it.
16. As a 用户, I want danmaku ASS style options (font/border/display area/
    opacity/speed/density/resolution), so that rendered subtitles match my
    taste.
17. As a 用户, I want subtitles in srt/lrc/txt/ass/json with language
    selection and ASS style config, so that I can watch foreign-language
    videos.
18. As a 用户, I want chapter info embedded via ffmetadata, so that
    players show chapters.
19. As a 用户, I want NFO metadata (Kodi/Jellyfin/Emby) or JSON sidecars,
    so that media servers index my downloads.
20. As a 用户, I want covers saved as jpg/png/avif/webp at original
    quality, so that library posters are perfect.
21. As a 用户, I want downloads with 1-10 parallel chunks per task and
    1-10 parallel tasks, so that big files download fast.
22. As a 用户, I want paused/failed tasks resumable from per-chunk offset
    tables, so that nothing is redownloaded from zero.
23. As a 用户, I want chunk-level retries with exponential backoff plus
    task-level auto-retry, so that flaky networks do not fail a task.
24. As a 用户, I want a global speed limit, so that downloads do not
    saturate my link.
25. As a URL-fluent 用户, I want CDN host selection (CN/OV areas, per-
    provider preference) with concurrent host probing and health tracking,
    so that the fastest mirror is used.
26. As a 登录用户, I want QR-code scan login, so that I can log in from
    the mobile app.
27. As a 登录用户, I want SMS login with GeeTest captcha handled in-app,
    so that I can log in without another device.
28. As a 登录用户, I want manual cookie paste login, so that I can use
    cookies from my browser.
29. As a 登录用户, I want my login state and wbi keys persisted, so that I
    stay logged in across restarts.
30. As a 匿名用户, I want the app to auto-provision buvid3/buvid4/buvid_fp/
    uuid/b_lsid/b_nut/bili_ticket and activate ExClimbWuzhi, so that I can
    download without logging in.
31. As a 用户, I want the naming engine with variables, `<>` optional
    segments, date/number formats, 12 built-in conventions and a visual
    rule builder, so that files land where my library expects them.
32. As a 用户, I want numbering modes (continuous/specified/use-parse-list),
    so that series numbering fits my scheme.
33. As a 用户, I want duplicate downloads detected across active and
    completed tasks by content hash, so that I do not store the same video
    twice.
34. As a 用户, I want the duplicate-detection dialog to offer
    ask/skip/continue, so that I control what happens.
35. As a 用户, I want the download list with cover, title, progress, speed
    and status, start-all/pause-all/delete-all, sort and filter, so that I
    can manage the queue.
36. As a 用户, I want clipboard monitoring (debounced) plus a startup
    re-check, so that copying a link is enough to start parsing.
37. As a 用户, I want parse history (last 100 entries), so that I can
    revisit previous URLs.
38. As a 用户, I want a system tray with menu and window restore, so that
    the app stays out of the way.
39. As a 用户, I want native OS notifications for task completion, so that
    I know when downloads finish while I do other things.
40. As a 用户, I want single-instance behavior: launching twice focuses
    the existing window, so that tray + clipboard watcher are not
    duplicated.
41. As a 用户, I want light/dark/auto themes with accent color and
    display-scaling override, so that the app matches my desktop.
42. As a 用户, I want the UI in zh_CN/zh_TW/en, so that I can use my
    language.
43. As a 用户, I want a window-state memory, so that the app reopens
    where I left it.
44. As a 用户, I want proxy support (none/system/manual http) and a custom
    User-Agent, so that the app works in my network.
45. As a 用户, I want area auto-detection, so that CDN area choices make
    sense.
46. As a 用户, I want FFmpeg selection (bundled/system/custom), so that I
    control the muxer used.
47. As a 用户, I want to choose where completed files land and how
    downloads are handled (delete-after etc.), so that my disk stays
    organized.
48. As a 用户, I want the log viewer with filter, so that I can self-
    diagnose problems.
49. As a 用户, I want update checks against Verhub with skip-version and
    prerelease opt-in, so that I stay current without being nagged.
50. As a 用户, I want the teaching tips / tutorial layer, so that first
    run teaches me the app.
51. As a 用户, I want the terms-of-use gate and privacy dialog, so that
    usage rules are clear before first use.
52. As a 用户, I want the About screen, so that I can see version/license
    info.
53. As a 用户, I want the exit dialog (minimize-to-tray vs exit, with
    remember choice), so that closing behaves the way I expect.
54. As a 用户, I want my settings, tasks and history readable from the
    Python version's data directory without migration, so that upgrading
    to the Rust build loses nothing.
55. As a 用户, I want the same screens (parse interface, downloads pivot,
    settings groups, login dialog, download-options dialog and its
    sub-pages, all misc/setting dialogs), so that my habits transfer.
56. As a 用户, I want download options (media/additional/naming pages with
    bottom preview) per task, so that per-task overrides are easy.
57. As a 用户, I want a log viewer dialog, so that log inspection is
    in-app.
58. As an 开发者/贡献者, I want the domain logic tests (wbi signature, URL
    patterns, task naming snapshots, rule template/model, units/time/space
    parsing, runtime state) ported to Rust before their modules are
    implemented, so that parity is verifiable.
59. As an 开发者/贡献者, I want wiremock-based HTTP fixtures for Bilibili
    API/CDN responses, so that parser/signing/CDN-probing/chunked-download
    logic is tested without touching the live network.
60. As an 开发者/贡献者, I want golden-sample data-format tests pinning
    config.json and the three SQLite schemas against real old files, so
    that drop-in compatibility cannot silently rot.
61. As an 开发者/贡献者, I want three-platform CI green as a release gate,
    so that cross-platform stays true.
62. As an 开发者/贡献者, I want single-instance interoperability with the
    Python version, so that the two builds never run simultaneously
    against the same data dir.
63. As an 开发者/贡献者, I want MCP tools (parse/download/task) exposed
    over the local HTTP JSON-RPC endpoint and the stdio bridge, so that
    automation (e.g. Claude Desktop) can drive the app.
64. As a 用户, I want the Rust app to be a native, small, self-contained
    executable on each platform, so that it installs and starts fast.

## Implementation Decisions

- **Repository shape**: same repo, Rust workspace under `crates/`. The
  Python version stays releaseable until the parity checklist is fully
  green; afterwards Python sources, launcher (PyStand) and related build
  scripts are deleted in a single clean-cutover commit.
- **UI**: gpui-kit 0.7.x, exactly pinned (`gpui-pre` =0.3.9 at the time of
  spec; bump deliberately, never `^`). Layout/IA/interaction parity with
  the existing screens; native gpui-kit visuals (no pixel-level Fluent
  clone, no Mica). Dark/light/auto theme + accent color + display-scaling
  override + zh_CN/zh_TW/en i18n preserved; the two qss skins map to
  gpui-kit theme tokens.
- **Platform matrix**: Windows 10 1809+ (MSVC), macOS 12+, Linux x64
  (X11/Wayland). Win7 shims and the `windows:nodirectwrite` workaround are
  dropped with Win7 support. Tray via Tauri's `tray-icon` + `muda`
  (mature, cross-platform); not gpui-tray. Notifications via gpui system
  notification API with its platform caveats (macOS needs a bundled .app
  for dev-build delivery testing). File dialogs via GPUI
  `prompt_for_paths`/`prompt_for_new_path`. Linux distribution: AppImage.
  Windows keeps Inno Setup; macOS ships .app/.dmg.
- **GeeTest captcha**: embedded via `gpui-webview` (WebKitWebView on
  macOS, WebView2 on Windows, webkit2gtk on Linux); fallback path =
  loopback redirect flow in the system browser if a platform's webview
  integration fails (webview is the primary, browser is the escape hatch).
- **Async/network stack**: dedicated tokio runtime for non-UI work;
  `reqwest` (rustls) for HTTP with cookie store and proxy support; `axum`
  for the MCP HTTP server on loopback. UI receives updates over channels
  into `Entity::update` + notify. This decouples the network stack from
  gpui's weekly snapshot churn.
- **Async model for downloads**: same conceptual engine as the Python
  version — multi-chunk ranged parallel download (1-10 chunks × 1-10
  parallel tasks), per-chunk offset table persisted in task JSON for
  resume, chunk-level retry with exponential backoff, task-level
  auto-retry (1-20), token-bucket global speed limit, file preallocation,
  206-vs-full-response guard, pause/resume/cancel, legacy FLV multi-part
  concat.
- **User data compatibility**: same `Bili23 Downloader/` app-data location
  per platform, same config.json structure with `config_version`
  migrations, same three SQLite databases (download_task/completed_task
  tables; thumbnail blob cache with size cap; history capped at 100),
  same PRAGMA user_version rehash migration semantics. WAL mode retained.
  Atomic config save retained.
- **Single-instance interop**: replicate the Windows named mutex (same
  fixed GUID), the lock file with staleness detection, and the local
  socket wake protocol, so the Rust and Python builds are mutually
  exclusive against one data dir.
- **Bilibili API client**: direct HTTP implementation (no third-party
  Bilibili crate), porting the request signing stack: wbi img_key/sub_key
  fetch + `enc_wbi` signing, anonymous session auto-provisioning
  (buvid3/buvid4/buvid_fp via murmur3, uuid, b_lsid, b_nut, bili_ticket
  via HMAC-SHA256 GenWebTicket), ExClimbWuzhi activation. All endpoint
  paths preserved.
- **Parsers**: port all 15 parser types (av/BV incl. multi-P and
  collections, interactive stein graph, bangumi/movie/documentary/TV/
  variety with season switching, cheese, mall lesson, space, favorites
  folder, collected lists, weekly popular, dynamic, watch-later, history,
  audio au/am incl. menus, b23.tv resolution, batch).
- **Danmaku**: protobuf segments via a Rust prost-generated client,
  output xml/ass/json; native ASS renderer ported from the Python
  implementation with all style options.
- **Subtitles**: srt/lrc/txt/ass/json output, language selection, ASS
  style config, embedded-language metadata.
- **Naming engine**: port the rule template compiler (variables, `<>`
  optional segments, date/number formats), all 12 built-in conventions,
  the visual rule builder UI, and the three numbering modes. Duplicate
  detection: content hash across both task tables with ask/skip/continue
  resolution.
- **FFmpeg**: subprocess wrapper (bundled/`bundle/ffmpeg(.exe)` location,
  system PATH, or custom path), `-progress pipe:1 -nostats` progress
  parsing, no-output watchdog kill, CREATE_NO_WINDOW on Windows.
- **Update service**: Verhub HTTP API client written in Rust against the
  actual API surface; skip-version, prerelease opt-in, forced-update
  support.
- **MCP server**: local loopback HTTP JSON-RPC at `/mcp` (default port
  23330), Bearer token auth, tools parse/download/task, plus the stdio
  bridge mode for Claude Desktop.
- **Maturity acknowledgment**: gpui-kit pins weekly GPUI snapshots with
  breaking API changes; the version pin is deliberate and upgraded only
  in a dedicated bump task.

## Testing Decisions

- **What makes a good test here**: only external behavior is tested. For
  network clients, the observable behavior is "given these HTTP requests/
  responses, this domain result comes out". For persistence, the
  observable behavior is "this old real-world file reads, and this write
  round-trips into the same shape a Python build would produce". Internal
  refactorings must not break tests; API-shape changes must.
- **Three seams**:
  1. **HTTP mock seam (wiremock)**: canned Bilibili API and CDN responses
     drive parsers, signing, CDN host probing, and the chunked downloader
     (including retry, 206-vs-full, resume-from-offset-table). Highest
     seam; preferred for everything that talks HTTP.
  2. **Data-format golden samples**: real old config.json and the three
     SQLite DBs (or realistic fixtures) pin read compatibility, migration
     behavior, and write round-trip. Guards drop-in compatibility.
  3. **Domain pure-function tests**: port the existing pytest suite's
     domain tests (wbi signature vectors, URL pattern matrix, task naming
     snapshots, rule template/model, units/time/space parsing, runtime
     state) as Rust tests written before their modules (specs-first).
- **Prior art**: the existing pytest suite in `test/` (39 files) is the
  prior art and the source for seam 3; its vectors/snapshots are reused
  verbatim.
- **UI is not automated**: gpui-kit headless smoke (window opens, screens
  render) plus a manual test checklist per screen; no widget-level
  automated tests.

## Out of Scope

- Win7 / pre-1809 Windows support.
- Mica/acrylic material effects (GPUI cannot).
- Pixel-level Fluent visual clone; the look is gpui-kit native.
- Mobile / wasm targets.
- arm64 Windows/Linux builds (macOS arm64 is covered naturally).
- New features beyond parity (e.g. the TODO.md filter/sort params idea is
  not included; it is a future feature, not part of this rewrite).
- Automated UI tests.
- Simultaneously shipping both versions after cutover (Python stays only
  during the transition, then is deleted).

## Further Notes

- Original request: "将整个项目使用Rust语言重构，UI界面使用
  https://github.com/longbridge/gpui-kit，所有功能保持不变，界面设计保持和现有的一致，最终的应用要支持跨平台使用"
- Design-tree decisions were settled through a grilling session:
  UI fidelity = native visuals with layout/IA parity (B); data compat =
  drop-in same dir/formats (A); platforms = Win10+/macOS 12+/Linux x64,
  drop Win7 (A); GeeTest = webview primary + browser-loopback fallback
  (C); tray = tray-icon+muda; Linux package = AppImage; repo = same repo
  clean cutover; identity = keep name/ids/v2.21.0/Verhub key; tests =
  three seams with domain-tests-first; single-instance interop with the
  Python build; async = tokio+reqwest+axum.
- GPL-3.0 compatibility: gpui-kit is Apache-2.0 (compatible); tray-icon/
  muda MIT/Apache (compatible).
- gpui-kit pins an exact GPUI snapshot (`gpui-pre`) and updates weekly
  with breaking changes; upgrades are a deliberate task, never incidental.
- The de-facto domain glossary currently lives in code (`ParserType`,
  `ConventionType`, `DownloadType`, `DownloadStatus`, `Attribute` IntFlag,
  `TaskInfo` sections, `EpisodeData`); this spec has materialized it into
  GLOSSARY.md.
