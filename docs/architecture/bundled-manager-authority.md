# Bundled manager CLI authority

Status: implementation prerequisite; desktop backend/launch remains unchanged.
Internal ad-hoc packaging is not public signing/notarization or release acceptance.

The GUI compiles a descriptor only when the packaging build explicitly sets
`DEVFORGE_PACKAGE_MANAGER=1`. The fixed generated manifest contains schema, exact
Cargo target, fixed adjacent filename (`devforge` or `devforge.exe`) and SHA-256.
The build parser rejects other targets, paths/names, non-lowercase-hex hashes,
extra fields and inputs larger than 1,024 bytes; its file read is capped at 1,025
bytes before parsing. Ordinary builds emit `None` and ignore stale manifests.
The build gate is not a runtime environment override.

`manager_artifact::bundled_launch()` resolves only next to `current_exe`, returning
an executable candidate, target and the compiled `[u8; 32]` expected digest. It
never searches PATH/cwd, reads runtime configuration, hashes a file to manufacture
its own expected authority, or launches anything. The native SDK must compare the
candidate's bytes with the compiled expected digest before launch. A missing or
changed candidate is an error, not permission to select a different executable.

## macOS signing order

Hashing the raw externalBin copy is insufficient: Tauri signs external binaries
after copying them, changing their bytes. Internal packaging now:

1. Copies the built CLI into the owned sidecar directory.
2. Ad-hoc signs that copy with a fixed manager identifier and verifies it.
3. Hashes those final CLI bytes and builds the GUI descriptor.
4. Removes only this CLI from macOS `externalBin`; copies it through fixed
   `macOS.files["MacOS/devforge"]`. The helper remains an externalBin.
5. Lets Tauri sign its explicit helper, GUI and outer application targets.
6. Verifies the final `.app/Contents/MacOS/devforge` hash against the pre-build
   manifest and verifies both the CLI and app signatures. Mismatch aborts packaging.

This relies on the matching Tauri CLI's custom-file copy and explicit signing
semantics, not on the assumption that repeated signatures preserve a file hash.
The pinned primary source has custom files copied before signing but does not add
them to sign_paths; Keychain::sign invokes codesign without `--deep`:

- [Tauri CLI 2.12.4 app bundling](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/macos/app.rs)
- [Matching codesign invocation](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-macos-sign/src/keychain.rs)

A Tauri/signing-pipeline update requires revisiting these semantics and rerunning
actual final-bundle hash verification. `--app-only` permits this scoped internal
macOS check without generating a DMG or changing OS trust/keychain permissions.

## Windows and verification boundary

Windows keeps the existing externalBin mapping. The internal unsigned NSIS build
uses that source directly; packaging rechecks the source after the bundler returns.
The report labels this `nsis-external-binary-source`, not installed-CLI acceptance.
An actual Windows install fixture must still verify the installed fixed adjacent
CLI against the compiled digest. New Authenticode/signing behavior also needs a
final-sign-before-hash pipeline; this change does not claim that future gate.

Focused checks cover generation, target/filename/hash/bounds, explicit gate,
ordinary-build None, adjacent resolution and final-payload mutation refusal.
Actual macOS app hash/signature verification is separate from parser tests and
from launch/bootstrap, GUI behavior, production credentials and Windows runtime.


## Concurrent-build authority and provenance

Packaging captures the exact generated authority bytes before the GUI build. After
the bundler returns it requires the on-disk authority to be byte-identical before
checking the final CLI. A concurrent prepare that changes whitespace or content
therefore invalidates the build rather than claiming the Python snapshot was the
hash read by Rust. This detects a changed final manifest; it is not a general
protection against adversarial concurrent build-input replacement and restoration.
Packaging should run from stable owned inputs.

The report captures Git HEAD and the dirty flag before building any artifacts,
and explicitly labels the working tree unfrozen. A concurrent commit at the end
cannot become the reported build-input commit. This remains provenance context,
not an immutable source attestation for concurrent dirty builds. The existing
actual macOS app hash/signature evidence predates this report/guard-only change;
it is reused without claiming that the new guard ran during that earlier build.
