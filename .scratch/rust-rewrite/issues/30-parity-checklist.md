# 30: End-to-end parity checklist + manual test checklist

**What to build:** A parity checklist derived from the spec's feature
list, each item verified against the Rust app (automated where a seam
exists, manual with recorded results otherwise), plus a per-screen manual
test checklist. This is the gate the cutover ticket consumes.

**Blocked by:** 22, 23, 24, 25, 26, 27, 28, 29.

**Status:** ready-for-agent

- [ ] Every user story 1-64 mapped to a checklist row with pass/fail + evidence
- [ ] Automated seams (wiremock/golden/unit) enumerated per feature with their test names
- [ ] Manual screen checklist per screen with recorded results
- [ ] Three-platform CI green recorded as evidence

## Comments
