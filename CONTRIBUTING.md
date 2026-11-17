# Contributing

Thank you for improving this community interoperability project!

1. Open an issue describing your Bebird model, OS, Wi-Fi setup, expected vs. actual behavior.
2. Submit focused pull requests with `cargo test` passing and clear reproduction steps.
3. For protocol changes, provide **sanitized** sample packet headers only; no serials, MAC addresses, user photos, APKs, decompiled vendor source, credentials, or tokens.
4. Do not add firmware commands, OTA, device resets or cloud connectivity without explicit technical and safety review.
5. Disclose when code or text was AI-assisted; review and test it yourself.

Developers: `cargo fmt --check`, `cargo test`, `cargo build --release` on Windows. CI includes a Windows test/build check.
