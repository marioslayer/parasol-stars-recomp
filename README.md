# Parasol Stars (1992, Ocean) — Rust Static Recompilation (x64)

Native 64-bit Windows PC port of the Amiga 500 version of **Parasol Stars: The Story of Rainbow Islands II**, built via static Motorola 68000 → Rust recompilation with zero external crate dependencies.

## Features
- **True 68000 → x86_64 Static Recompilation:** `recomp/` lifts the 100 KB main 68000 executable directly from `game/Parasol Stars (1992)(Ocean)[cr].zip` into native Rust functions at build time.
- **Amiga OCS Runtime (`runtime/`):** 4-channel Blitter, scanline-accurate Copper, 5-bitplane + hardware sprites + Copper-multiplexed HUD, 4-channel Paula stereo audio (44.1 kHz), 1 MB Chip RAM + 512 KB Slow RAM cache, HLE disk loader.
- **Borderless Fullscreen:** Press `F11` or `Alt+Enter` (or pass `--fullscreen` / `-f`). Preserves 320:256 aspect ratio with black letterbox bars and hides the mouse cursor.
- **CRT Scanlines:** Press `F9` to cycle `CRT: Subtle` → `CRT: Classic` → `CRT: Off` (pass `--no-crt` to start with scanlines off).
- **Xbox Gamepad (XInput):** Plug-and-play support for up to 2 Xbox controllers (`xinput1_4.dll` / `xinput1_3.dll` / `xinput9_1_0.dll`).

## Build & Run
Place your own Amiga disk image `Parasol Stars (1992)(Ocean)[cr].zip` into the `game/` directory, then run:
```powershell
# Fast build (~20s, runs >130 FPS, locked to 50 Hz PAL in window):
cargo run -p parasol_stars

# Start directly in fullscreen:
cargo run -p parasol_stars -- --fullscreen

# Fully optimized release build:
cargo run --release -p parasol_stars
```

## Controls
| Action | Keyboard (P1) | Xbox Gamepad (P1 / P2) | Keyboard (P2) |
|---|---|---|---|
| Move Left / Right | `←` `→` or `A` `D` | D-Pad / Left Stick | `J` `L` |
| Jump / Up | `↑` or `W` or `X` | `A` / `Y` / D-Pad Up / Stick Up | `I` |
| Duck / Down | `↓` or `S` | D-Pad Down / Stick Down | `K` |
| Parasol / Fire / Start | `Z` / `Space` / `Ctrl` / `Enter` | `X` / `B` / `RB` / `RT` | `N` / `Right Shift` |
| Pause | `P` | `Start` | — |
| Escape / Menu | `Esc`, `F1`–`F5`, `1`–`5`, `M` | `Back` (Esc) | — |
| Toggle Fullscreen | `F11` or `Alt+Enter` | — | — |
| Cycle CRT Scanlines | `F9` | — | — |
