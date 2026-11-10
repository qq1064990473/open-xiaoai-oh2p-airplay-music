# Build verification: v1.4.0-airplay-music-ha.6-kuwo-radio

- Built: 2026-11-10
- Target: `armv7-unknown-linux-gnueabihf.2.25`
- Rust: 1.96.0
- Zig: 0.16.0 via `cargo-zigbuild`
- Binary: `client-airplay-music-ha-kuwo-radio-oh2p-armv7-glibc2.25`
- ELF: 32-bit little-endian ARM, EABI5 hard-float (`e_flags=0x05000400`)
- Interpreter: `/lib/ld-linux-armhf.so.3`
- Highest required glibc symbol version: `GLIBC_2.25`
- SHA-256: `82b9bd979eadfbbcc08ff2b87ac5672e2689a2c33fee31afae56986fb02e461b`
- Tests: `cargo test` — 20 passed, 0 failed
- JSON example: parsed successfully with PowerShell `ConvertFrom-Json`

This is the client executable only; it is not a full firmware image. Keep the
speaker's current executable and configuration as rollback copies during
deployment.
