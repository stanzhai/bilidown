# 37: First-run window = 2/3 of main display, centered, min 950×600

**What to build:** When no saved window state exists
(`saved_window_bounds` → None), the fallback is currently the kit
default, not the spec'd default. Compute 2/3 of the main display's
bounds, floor at the 950×600 minimum (`services::window_state`
DEFAULT), and center on that display. Saved bounds
(`remember_window_state`) still win when present and valid.

**Blocked by:** (none)

**Status:** resolved

- [x] No saved state → window opens at 2/3 main display, centered
- [x] Size never below 950×600
- [x] Saved bounds unchanged in behavior
- [x] Unit test for the fallback computation (fake screens, incl.
      sub-minimum 2/3 result flooring)
## Answer

`services::window_state::default_state(screen)` — pure computation:
2/3 of the given screen's width/height (integer floor), `.max()` at
`DEFAULT_WIDTH`/`DEFAULT_HEIGHT` (950×600), then centered on that screen
(`x + (width - w)/2`). `main.rs` renamed `saved_window_bounds` →
`startup_bounds`: display-to-`ScreenRect` conversion factored into a
closure used for both the saved-state screen list and
`cx.primary_display()`; saved state (via `saved_state`) still wins via
`.or_else(...)`, primary display bounds feed `default_state` when it
returns None.

Tests in `crates/services/tests/window_state.rs`:
`default_is_two_thirds_of_screen_centered` (2560×1440 → 1706×960 at
(427, 240)), `default_floors_sub_minimum_two_thirds_at_950x600`
(1200×750 → 950×600 at (125, 75)),
`default_centers_on_an_offset_display` (1920×1080 at (1000, 200) →
1280×720 at (1320, 380)). Existing saved-state tests unchanged.

## Comments
