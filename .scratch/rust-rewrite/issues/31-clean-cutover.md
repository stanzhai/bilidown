# 31: Clean cutover — delete Python build

**What to build:** The single clean-cutover commit: delete the Python
sources, launcher (PyStand), and related build scripts once the parity
checklist is fully green. Rust tree becomes the product.

**Blocked by:** 30.

**Status:** ready-for-agent

- [ ] src/ Python app, launcher/, PyStand, Python build scripts and CI jobs deleted
- [ ] README/docs updated to the Rust build only
- [ ] Repo still builds and all Rust tests green after deletion

## Comments
