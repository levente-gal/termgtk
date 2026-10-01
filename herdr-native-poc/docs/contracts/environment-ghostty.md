# environment-ghostty.md

Captured 2026-09-28 on the developer machine during Agent B (Ghostty FFI) discovery. Research only; no Ghostty tree was left under the repo.

## Zig

| Check | Result |
| --- | --- |
| `zig version` (system `PATH`) | **missing** — `zsh: command not found: zig` |
| `rpm -q zig` | `package zig is not installed` |
| Fedora packages available (`dnf repoquery`) | `zig-0.15.2-3.fc44.x86_64`, **`zig-0.16.0-1.fc44.x86_64`** |
| Zig used for successful `/tmp` build | Official tarball `zig-x86_64-linux-0.16.0` → reports **`0.16.0`** |
| Upstream requirement at pinned commit | `build.zig.zon` → `minimum_zig_version = "0.16.0"` |

Install recommendation for Cargo `build.rs`: `sudo dnf install zig` (0.16.0 on Fedora 44) or pin the same official tarball.

## Ghostty pin

| Field | Value |
| --- | --- |
| Provenance | https://github.com/ghostty-org/ghostty |
| Commit | **`b40acce58dcf77df52231c3798ea58e924647c89`** |
| Tag at discovery | `tip` |
| License | MIT (Mitchell Hashimoto, Ghostty contributors) |
| Newest stable release inspected | **`v1.3.1`** / `332b2aefc6e72d363aa93ab6ecfc86eeeeb5ed28` — `minimum_zig_version = "0.15.2"`; **libghostty-vt C API lacks terminal object / render-state** (parsers only). Unsuitable for `TerminalEngine`. |

## GTK / Adwaita (context only; not required for libghostty-vt)

| Package | `rpm -q` |
| --- | --- |
| `gtk4` | `gtk4-4.22.1-2.fc44.x86_64` |
| `libadwaita` | `libadwaita-1.9.0-1.fc44.x86_64` |
| `gtk4-devel` | not installed |
| `libadwaita-devel` | not installed |

`libghostty-vt` does not link GTK; these packages matter for the native shell (Agent C), not for the VT library build.

## Local libghostty-vt build attempt

### Blocker (first attempt)

System Zig absent. Official Zig 0.16.0 tarball downloaded to `/tmp`. First `zig build` failed under the agent sandbox with:

```text
error: unable to open global cache directory '/home/lgal/.cache/zig': AccessDenied
```

Retry with writable caches:

```bash
export ZIG=/tmp/zig-x86_64-linux-0.16.0/zig
export ZIG_GLOBAL_CACHE_DIR=/tmp/zig-global-cache
export ZIG_LOCAL_CACHE_DIR=/tmp/zig-local-cache
```

### Success

Checkout: `/tmp/ghostty-tip` @ `b40acce58dcf77df52231c3798ea58e924647c89`  
Command:

```bash
$ZIG build -Demit-lib-vt -Doptimize=ReleaseFast --prefix /tmp/ghostty-vt-prefix
```

Exit status: **0** (~130 s including Zig dependency fetch).

### Produced paths

| Artifact | Path |
| --- | --- |
| Headers | `/tmp/ghostty-vt-prefix/include/ghostty/vt.h` (+ `include/ghostty/vt/*.h`) |
| Static lib | `/tmp/ghostty-vt-prefix/lib/libghostty-vt.a` |
| Shared lib | `/tmp/ghostty-vt-prefix/lib/libghostty-vt.so` → `libghostty-vt.so.0.1.0` |
| pkg-config | `/tmp/ghostty-vt-prefix/share/pkgconfig/libghostty-vt.pc` |
| pkg-config (static) | `/tmp/ghostty-vt-prefix/share/pkgconfig/libghostty-vt-static.pc` |

### Link flags (from generated `.pc`)

Shared:

```text
-I/tmp/ghostty-vt-prefix/include
-L/tmp/ghostty-vt-prefix/lib -lghostty-vt
```

Static (`libghostty-vt-static.pc`):

```text
-I/tmp/ghostty-vt-prefix/include
/tmp/ghostty-vt-prefix/lib/libghostty-vt.a
```

Exported symbols verified with `nm -D` on the shared library include at least: `ghostty_terminal_new`, `ghostty_terminal_vt_write`, `ghostty_terminal_resize`, `ghostty_terminal_free`, `ghostty_render_state_*`, `ghostty_cell_get`, `ghostty_key_encoder_*`.

### Not left in the repo

`/tmp/ghostty-tip`, `/tmp/ghostty-vt-prefix`, and `/tmp/zig-x86_64-linux-0.16.0` are ephemeral inspection trees only. Vendoring is a later task (`dev/03-ghostty-engine.md`).

## Required Fedora packages (Ghostty VT build path)

| Package | Status on machine | Need |
| --- | --- | --- |
| `zig` 0.16.0 | not installed (available in repos) | **required** to build libghostty-vt |
| C toolchain (`gcc`/`clang`, binutils) | assumed present via normal Fedora/Rust tooling | link step |
| `gtk4` / `libadwaita` | installed (runtime) | not for VT lib |
| `gtk4-devel` / `libadwaita-devel` | not installed | not for VT lib |

## Summary for gate

- Pin Ghostty **`b40acce58dcf77df52231c3798ea58e924647c89`** with Zig **0.16.0**.
- System `zig` must be installed (or bootstrapped) before Cargo can rebuild libghostty-vt.
- C API at that pin is **sufficient** for create/feed/resize/iterate/destroy; **no C shim required**.
- Newest stable tag `v1.3.1` is **not** viable for the planned facade.
