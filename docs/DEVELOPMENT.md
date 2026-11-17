# Development log and reproducibility

## Proven steps

1. Windows USB-Wi-Fi adapter connected to the Bebird R37R AP (`192.168.5.100` on PC, `192.168.5.1` device).
2. Android app package identified via ADB (`com.molink.john.hummingbird`). Original APK was examined **locally only**, not included here.
3. Independent protocol reimplementation: battery probe `66 3A` received a 4-byte reply; connected UDP video socket on `58080` received JPEG chunks.
4. PowerShell image reassembly produced valid still photos.
5. Rust v0.1 GUI worked but had stalls; v0.2 recovery improved live display and snapshots.
6. v0.3 confirmed physically working LED brightness control.
7. v0.4 initial build failed `E0503`; v0.4.1 fixed the simultaneous immutable/mutable use of `led`.
8. **User proof for v0.4.1:** Windows screenshot showing live camera, battery, LED controls, ~10.8 FPS and zero decoder errors at capture; console log passed 1,000 complete frames and issued successful LED set commands. No independent long-duration stress test yet.

## Validation boundaries

- Rust unit tests check assembler edge cases, not physical camera hardware.
- Manual Windows 11 hardware test is confirmed by user; no benchmark across other systems or camera models.
- UI-only release-label adjustment in publication copy; user must build this exact package to validate its displayed label.

## Planned work

- Improve first-frame latency and recoveries; track missing packet rates and frame timing.
- Optional camera rotation and mirroring, explicit fullscreen mode, video recording.
- Separate protocol crate from GUI; mockable transport and integration tests.
- Parameterize camera address and interface; compatibility matrix for other models.
- Release reproducible Windows binary with integrity checks after clean CI build and testing.
