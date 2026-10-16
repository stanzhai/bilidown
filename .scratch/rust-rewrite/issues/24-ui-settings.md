# 24: Settings screens

**What to build:** All settings groups from the Python app: appearance
(theme/accent/scaling/language), network (proxy, UA, area), download
(paths, parallelism, retry, speed limit, delete-after), ffmpeg
(bundled/system/custom), naming rules management, danmaku/subtitle
styles, about screen (version/license), update settings (channel,
skip-version, prerelease). Reads/writes via the config store.

**Blocked by:** 02, 19, 20.

**Status:** in-progress

- [ ] Every Python settings group present with the same controls
- [ ] Edits persist through the config store atomically and apply live where applicable
- [ ] Naming rule management CRUD with the 12 conventions
- [ ] About screen shows app id/version line info

## Comments
