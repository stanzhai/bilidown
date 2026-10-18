# 33: Login identity — `uname` in status + icon rail with login footer

**What to build:** The main nav becomes a collapsed icon rail (5 items)
with the login entry moved to the sidebar `.footer(...)` as an icon
button (CircleUser when logged in, LogIn otherwise), tooltip = the
account `uname` or "Log In", always clickable, opening the login dialog
when logged out. Login status shows the account's `uname` (fetched from
`x/web-interface/nav` → `data.uname`), not the literal "Logged in".

Also covers ticket 32 item 3 verification only: the checkbox-alignment
fix (`collect_row_meta` walks children only) already exists on main —
run its regression test and record the result in the Answer.

**Blocked by:** (none)

**Status:** ready-for-agent

- [ ] `AuthState` carries a persisted `uname`, refreshed at startup when
      `is_login` and after any login success
- [ ] Sidebar is a collapsed icon rail (labels available via tooltip);
      5 nav items unchanged
- [ ] Footer login button: icon reflects login state, tooltip shows
      `uname` / "Log In", opens login dialog when logged out, never disabled
- [ ] Ticket-32 item 3 (`collect_row_meta_matches_display_rows` or
      equivalent) test located and passing; result recorded here

## Comments
