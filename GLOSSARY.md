# bilidown

Desktop downloader for Bilibili content (video, audio, danmaku, subtitles,
covers, metadata). Single context: one app, one user-data directory.

## Language

### Content and parsing

**Parse**:
Turn a URL (or id list) into a list of downloadable Episodes without
downloading media. The app's front door.
_Avoid_: analyze, resolve

**Parser Type**:
Which of the 15 URL families a Parse targets: av/BV uploads (incl. multi-P
and collections), interactive, bangumi/movie/documentary/TV/variety,
cheese, mall lesson, space, favorites folder, collected list, weekly
popular, dynamic, watch-later, history, audio (au/am), b23.tv, batch.
_Avoid_: link type, url type

**Episode**:
One downloadable unit inside a Parse result (a page, a bangumi episode, a
course lesson, an interactive node path). Carries title, duration, badge,
attributes.
_Avoid_: item, part (reserved: multi-P page), video

**Interactive Video**:
A video whose choices form a stein graph; a downloadable path is a
selected chain of nodes, not the whole graph.

**Season**:
A bangumi/movie/documentary/TV/variety series container switchable within
one Parse; distinct from multi-P collections.

**Collection**:
A user-authored grouping of the uploader's own videos (合集); distinct from
favorites.

**Favorites Folder**:
A user's curated folder of others' videos (收藏夹); distinct from
collection.

**Collected List**:
A publicly shared favorites folder owned by another user.

**Parse History**:
The last 100 parsed URLs, capped.
_Avoid_: recent list

**Batch Parse**:
A Parse whose input is a multi-line list of av/BV ids.

**Batch Select**:
Selecting Episodes by episode-number ranges from a Parse result.

**Auto Parse**:
Walking all pagination pages of a list-type Parse with a progress worker.

### Login and session

**Anonymous Session**:
Auto-provisioned device identity (buvid3/buvid4/buvid_fp, uuid, b_lsid,
b_nut, bili_ticket) plus ExClimbWuzhi activation; enables downloading
without login.

**Wbi Keys**:
img_key/sub_key pair fetched from the nav endpoint; the signing material
for `enc_wbi`-signed requests.

**GeeTest**:
The third-party captcha gating SMS login; rendered in an embedded webview.

### Downloading

**Download Task**:
A unit of download work: Episode + selected media options + naming
options, frozen at creation. Lives in the active table until completion,
then the completed table.
_Avoid_: job

**Task Info**:
The full serialized snapshot of a Download Task (Basic/File/Episode/
Download/Options/Naming sections) persisted as JSON in the task database.
_Avoid_: task data

**Chunk**:
One ranged slice of a media file. Parallel downloads are chunk-parallel;
resume replays per-chunk offset tables.
_Avoid_: segment (reserved: danmaku segment), thread

**Hash Id**:
Content hash over a Task's content, used for duplicate detection across
both task tables.
_Avoid_: checksum

**Duplicate Resolution**:
The ask/skip/continue choice offered when a new task's hash_id matches an
existing one.

**Preview**:
Pre-download display of the actual qualities, codecs, bitrates and sizes a
logged-in session can fetch for an Episode.

**Quality Priority**:
The user's ordered preference of video (16→127) and audio (30216→30251)
ids; download picks the best available within it.

**Codec Priority**:
Ordered preference over AVC(7)/HEVC(12)/AV1(13) with fallback warning.

**CDN Probing**:
Concurrent HEAD/Range checks of replacement mirror hosts
(upos-sz-mirror*), CN vs OV area lists, tracking host health per provider.

**Speed Limit**:
Token-bucket global bandwidth cap.
_Avoid_: throttle

**FLV Legacy**:
The old multi-part FLV delivery path with concat merge; predates DASH.

### Extras

**Danmaku**:
Scrolling comments; fetched as protobuf segments, exported xml/ass/json.

**Danmaku Style**:
Font/border/display-area/opacity/speed/density/resolution options for ASS
rendering.

**Subtitle Style**:
ASS appearance options for subtitle export (srt/lrc/txt/ass/json).

**Cover**:
Episode thumbnail/poster; cached in the thumbnail database as webp blobs
and exportable jpg/png/avif/webp.

**NFO**:
Kodi/Jellyfin/Emby metadata sidecar (or JSON alternative).

**Chapter Info**:
Container chapter marks embedded via ffmetadata.

### Naming

**Naming Rule**:
A template with variables, `<>` optional segments and date/number formats;
compiled by the rule engine.
_Avoid_: filename template

**Convention Type**:
One of the 12 built-in Naming Rules keyed by Parser Type.
_Avoid_: preset

**Numbering Mode**:
How episode numbers are assigned: continuous, specified, or
use-parse-list.

**Rule Builder**:
The visual chip editor for composing Naming Rules.

### App state

**User Data Directory**:
The per-platform app-data location holding config.json, the three
databases (task/thumbnail/history) and logs; shared format with the legacy
Python build.

**Single Instance**:
Mutual exclusion of running instances, including against the legacy Python
build (named mutex + lock file + socket wake).

**Verhub**:
The third-party update service the app polls for new versions.

**MCP Server**:
The optional local loopback JSON-RPC endpoint exposing parse/download/
task tools, plus its stdio bridge for Claude Desktop.

**Teaching Tips**:
First-run tutorial layer.
_Avoid_: onboarding

**Terms Gate**:
The terms-of-use acceptance screen shown before first use.

### UI

**界面**:
Simplified-Chinese rendering of "interface" in all UI strings; the
traditional column keeps 介面. Applied catalog-wide (1012-entry base
plus hand-added block).

**注销 / 登出**:
zh_CN prefers 注销 for logout (注销登录 = log out of the account);
zh_TW prefers 登出. Do not mix per column. 注销登录失败 is the logout
failure message.

**Theme Mode**:
Light / Dark / Auto (follow system). Auto re-resolves against the OS
appearance at startup and on every appearance change while selected;
manual Light/Dark stay pinned.

**Nav Rail**:
The app-drawn left sidebar: fixed 88px column, one icon-over-label
entry per screen, login entry pinned at the bottom (uname when logged
in). Replaces the kit `Sidebar`, which cannot host vertical entries.
