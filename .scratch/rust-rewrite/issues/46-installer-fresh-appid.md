# 46: Installer identity decoupled from legacy Bili23 Downloader

**What to fix:** The Rust release installer still shared the legacy
Python "Bili23 Downloader" Inno AppId, so it upgraded in place into
whatever directory the old install lived in. The Rust version installs
to its own `Program Files\bilidown` with no legacy coupling.

**Status:** resolved

## Answer
Fresh AppId `3C3AEF77-CE35-40B1-8767-74C8D42586A0` in `setup.iss` and
`packager.py` (`INNO_APP_GUID`), both asserted in
`check_packaging.py` (`inno_script_keeps_app_id_and_exes`,
`packager_guid_matches_inno_script`). `MyAppName=bilidown` keeps
`DefaultDirName={autopf}\bilidown`. The `InitializeSetup` legacy-cleanup
probe now targets the old install's own registry key
(`B096F0C1-…_is1`), still offering to delete the old folder so the two
never coexist. The single-instance AppMutex GUID stays with the old value
(`single_instance.rs` ↔ `AppMutex`/`OpenMutexW` interop unchanged).
Data-dir migration in `crates/config/src/lib.rs` (renames
`Bili23 Downloader/` → `bilidown/` in the user data dir) is separate and
retained.
