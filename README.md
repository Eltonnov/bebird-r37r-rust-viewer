# Bebird R37R Rust Viewer

**Unofficial, community-made Windows viewer** for the Bebird R37R (EarSight Plus) Wi-Fi camera. This project was developed through iterative, AI-assisted programming ("vibe coding") and validated against a real R37R camera. It is **not affiliated with or endorsed by Bebird**.

**Status:** v0.4.1 — user-verified on Windows 11 (2026-11-17); source-release, **no prebuilt executable**. The GUI shows live images, FPS, battery percentage, a controllable LED slider, and a photo button. Actual performance varies with Wi-Fi conditions and scene motion.

> Security: the device exposes an **unencrypted Wi-Fi access point**. Use only with your own camera, on a trusted local connection. This app uses only local UDP, does not require accounts, and does not modify Windows network configuration.

## What works (tested on R37R)

| Feature | Status |
| --- | --- |
| Windows 11 native Rust GUI | Tested |
| UDP MJPEG frames, JPEG reassembly, live display | Tested |
| Battery query and percentage | Tested |
| LED brightness via slider / keyboard | Tested |
| Save snapshot JPEG to `Aufnahmen/` | Tested |
| Stream retry after no complete frame | Implemented, occasional stalls remain |
| Video recording / firmware updates | Not implemented |
| Other Bebird models | **Not validated** |

## Requirements

- Bebird R37R powered on, connected to its own Wi-Fi on a **separate Wi-Fi adapter**; normal wired Internet can remain connected
- Windows 11 and Rust stable MSVC toolchain; C++ Build Tools may be required for the linker
- Camera defaults to `192.168.5.1`; Windows Wi-Fi adapter often receives `192.168.5.100`
- Camera's Android app should not simultaneously access the camera

### Build

```powershell
cargo test
if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
.\target\release\bebird-r37r-viewer.exe
```

For a different Wi-Fi IPv4 address, supply it as the sole argument:

```powershell
.\target\release\bebird-r37r-viewer.exe 192.168.5.100
```

### Controls

- Left-click / drag LED slider, or `+ 10%` / `- 10%` buttons
- Arrow up/down: adjust LED 10% (initial unknown value falls back to 50% for relative adjustments)
- Click `FOTO SPEICHERN` or press `S`: save JPEG snapshot
- `Esc` or close window: stop streaming and exit

## Troubleshooting

- **Waiting for video:** confirm separate Wi-Fi adapter is connected to the R37R access point; do not mistake the wired Ethernet IP for the camera interface.
- **Frames freeze:** moving quickly or abrupt brightness changes can lose UDP packets; the receiver discards incomplete frames and resends the start request after ~1.5 s without a complete frame.
- **Battery missing / LED unknown:** these queries may time out, but streaming can still work. LED value is unchanged unless you act.
- **Build failure:** run `rustc --version`, `cargo --version`; use stable `x86_64-pc-windows-msvc` and ensure Microsoft linker exists.
- **Other models:** do not assume identical IP, ports or protocols.

## Architecture and protocol

See [`docs/PROTOCOL.md`](docs/PROTOCOL.md) and [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md). Protocol notes are from interoperability research and real packet tests, not a claim of vendor specification.

## Vibe-coding note

This prototype was iteratively developed with AI assistance and corrected using **real compiler output and physical-camera tests**. AI output is not automatically trustworthy; contributions should include tests and concrete logs. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Privacy and license

The repo **does not include any vendor APK, decompiled Java, device-specific serials, private captures or sample photos**. The Rust implementation is independently written. MIT license applies to the code in this repository, not to trademarks or third-party proprietary software. Please report any accidental disclosure before sharing a fork.
