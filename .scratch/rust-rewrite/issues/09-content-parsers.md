# 09: Episode list parsers (av/BV, collections, bangumi family, cheese, mall)

**What to build:** Parsers producing checkable Episode lists from:
av/BV uploads incl. multi-P and collections; interactive videos (stein
graph traversal with node selection); bangumi/movie/documentary/TV/
variety with season switching; cheese courses; mall lessons. Each
returns Episode lists with title/duration/badge/attributes against
wiremock-pinned API responses.

**Blocked by:** 08, 06.

**Status:** resolved

- [x] av/BV multi-P expansion into Episodes (test_episode_list_parsers, test_list_parser vectors ported)
- [x] Collection (合集) grouping matches Python behavior
- [x] Interactive: stein graph fetch + selected-path Episodes
- [x] Season list + switching for the bangumi family
- [x] Cheese lessons and mall lessons parse to Episode lists
- [x] All parser tests run against wiremock fixtures mirroring real API shapes

## Answer

Ported the episode/parser layers of `src/util/parse` to `crates/parsers`:

- `episode/` — pure tree builders: `list_base` + `list_parsers` (space,
  watch-later, favlist, history, popular, collection), `video` (single /
  分P / 合集 with 章节 flattening, 分P sub-layers, `attribute` bit badges,
  `current_page_number` cid pin, CST-naive `timestamp_from_string`),
  `bangumi` (正片 section prepend, 预告 number skip, bvid/cid-less section
  drop, season number/rating/premiered side data), `media` (cheese
  `label`/status badges, `release_date` regex premiere; mall lesson
  blank-chapter flattening, watch-right badges, section/lesson ids).
- `media_parser.rs` — the HTTP side: `parse_video_url` dispatch
  (redirect / `is_stein_gate` / episode; av→BV via `batch::aid_to_bvid`,
  `?p=` page override), `parse_interactive` (graph_version probe with the
  exact player/v2 params, `edgeinfo_v2` BFS over `(cid, edge_id)` states
  with visited-set dedup and edge-id option merging), `parse_bangumi_url`
  (ep/ss/md via `pgc/review/user`, `current_ep_id` injection),
  `parse_cheese_url` (pugv season v2), `parse_lesson_url` (login guard,
  strict-digit id extraction, POST detail, `data` object check, id merge,
  durl `build_lesson_media_info`).
- `tree.rs` — check-state model additions: `set_checked_state` /
  `refresh_check_state` / `set_child_checked_state` (value-tree parent
  aggregation), `get_all_checked_children`, `EpisodeData::parsing_with_clear`
  + `active_parsers`/`reset`/`episode_ids`.

List-family HTTP (space/favlist/collection links) landed via T10's
`list.rs` at merge `f32397ac`; ticket 09's `parser_list.rs` was dropped as
superseded.

Tests: 104 parser tests green across 10 binaries
(`episode_tree`, `episode_list`, `episode_media`, `media_parser` are
ticket 09's: 14 + 10 + 11 + 13), all against wiremock fixtures mirroring
real API shapes. Full workspace: build + clippy `-D warnings` +
270 tests green.

Commits (branch `ticket-09`): `26f89711` episode-list parsers + HTTP
ListParser, `f32397ac` merge of integration `4c5ae49a`, `b8084b97`
post-merge reexport fix, `760d3fff` slice-3 media episode parsers,
`4f7a1b4a` slice-4 media HTTP parsers.

## Comments
