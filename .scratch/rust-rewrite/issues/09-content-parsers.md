# 09: Episode list parsers (av/BV, collections, bangumi family, cheese, mall)

**What to build:** Parsers producing checkable Episode lists from:
av/BV uploads incl. multi-P and collections; interactive videos (stein
graph traversal with node selection); bangumi/movie/documentary/TV/
variety with season switching; cheese courses; mall lessons. Each
returns Episode lists with title/duration/badge/attributes against
wiremock-pinned API responses.

**Blocked by:** 08, 06.

**Status:** ready-for-agent

- [ ] av/BV multi-P expansion into Episodes (test_episode_list_parsers, test_list_parser vectors ported)
- [ ] Collection (合集) grouping matches Python behavior
- [ ] Interactive: stein graph fetch + selected-path Episodes
- [ ] Season list + switching for the bangumi family
- [ ] Cheese lessons and mall lessons parse to Episode lists
- [ ] All parser tests run against wiremock fixtures mirroring real API shapes

## Comments
