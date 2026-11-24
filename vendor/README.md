# Vendored patches

| Crate | Why | Remove when |
|---|---|---|
| `xattr-0.2.3` | `gpui` 0.2.2 pulls it in (via `gpui_http_client` → `zed-async-tar`). Newer `libc` removed `ENOATTR` on Linux, so it no longer compiles. Patched to use `ENODATA` (same value). See `src/sys/mod.rs`. | `gpui` stops depending on `xattr` 0.2 |
