# 37: First-run window = 2/3 of main display, centered, min 950×600

**What to build:** When no saved window state exists
(`saved_window_bounds` → None), the fallback is currently the kit
default, not the spec'd default. Compute 2/3 of the main display's
bounds, floor at the 950×600 minimum (`services::window_state`
DEFAULT), and center on that display. Saved bounds
(`remember_window_state`) still win when present and valid.

**Blocked by:** (none)

**Status:** ready-for-agent

- [ ] No saved state → window opens at 2/3 main display, centered
- [ ] Size never below 950×600
- [ ] Saved bounds unchanged in behavior
- [ ] Unit test for the fallback computation (fake screens, incl.
      sub-minimum 2/3 result flooring)

## Comments
