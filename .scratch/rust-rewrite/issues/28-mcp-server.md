# 28: MCP server + stdio bridge

**What to build:** Local loopback HTTP JSON-RPC at `/mcp` (default port
23330) with Bearer token auth exposing parse/download/task tools, plus
the stdio bridge mode for Claude Desktop.

**Blocked by:** 09, 16.

**Status:** ready-for-agent

- [ ] axum server on loopback with Bearer auth; wrong token rejected
- [ ] parse/download/task tools drive the real parser and task manager (wiremock-backed tests)
- [ ] stdio bridge speaks the same protocol over stdin/stdout
- [ ] Port + token configurable via config store

## Comments
