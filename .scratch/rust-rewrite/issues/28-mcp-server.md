# 28: MCP server + stdio bridge

**What to build:** Local loopback HTTP JSON-RPC at `/mcp` (default port
23330) with Bearer token auth exposing parse/download/task tools, plus
the stdio bridge mode for Claude Desktop.

**Blocked by:** 09, 16.

**Status:** resolved

- [x] axum server on loopback with Bearer auth; wrong token rejected
- [x] parse/download/task tools drive the real parser and task manager (wiremock-backed tests)
- [x] stdio bridge speaks the same protocol over stdin/stdout
- [x] Port + token configurable via config store


## Answer

Implemented in `crates/mcp` at commit `b719903d` (merge `77a02696`, app
adaptation `2b61ecb9`): axum loopback endpoint (Bearer + mirrored-header
validation + origin pinning), parse/download/task tools over the real
`parsers` + `tasks::TaskManager` stack, `mcp-stdio` bridge binary; token
auto-generated + persisted under the `MCP` config group. Deviations from
the Python source: stdio auto-launch of the GUI omitted (no GUI running
headless), `get_login_status` reports no username/session expiry (auth
cookie state only), bangumi category is fixed `"ANIME"` (season type not
exposed), and create_download creates tasks directly (no GUI signal bus,
so no 60s wait / "submitted" path).

## Comments
