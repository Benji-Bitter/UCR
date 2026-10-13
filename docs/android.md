# UCR on Android — the screen for screenless radios

The Scout keychain, the bare reference build, a modem dongle — none of them
have a display. **The phone is the display.** `apps/desktop` is Tauri 2, and
the same React UI + the same Rust core compile into a real Android app.

What the APK actually is:

- **A full UCR node in your pocket** — `ucr-core` runs *on the phone*:
  ed25519 identity, storage, mesh router, channels, DMs, groups, emergency,
  digital lock, morse. Not a thin client.
- **Network transports only (today)** — LAN UDP broadcast + TCP gateways,
  same as desktop. WiFi on, mesh on. USB-OTG serial to a tethered modem is
  the documented gap (see below).
- **Permissions**: `INTERNET`, `ACCESS_NETWORK_STATE`,
  `CHANGE_WIFI_MULTICAST_STATE` (LAN peer discovery needs the multicast
  lock on Android WiFi).

## Install

The repo builds `ucr.apk` (debug-signed, ~15 MB, `radio.ucr.app`,
minSdk 24 = Android 7+):

```bash
adb install ucr.apk        # plugged in over USB
# or copy it to the phone and tap it — allow "install unknown apps"
```

Debug-signed = sideloadable, not Play-signed. For a store/release build,
generate a real keystore and sign the unsigned release APK with
`apksigner`.

## Build from source

```bash
# one-time: Android SDK + NDK r26, rust targets
sdkmanager "platform-tools" "platforms;android-34" "build-tools;34.0.0" "ndk;26.1.10909125"
rustup target add aarch64-linux-android armv7-linux-androideabi

cd apps/desktop
pnpm build
ANDROID_HOME=~/android-sdk NDK_HOME=~/android-sdk/ndk/26.1.10909125 \
  cargo tauri android build --apk --target aarch64
# unsigned: gen/android/app/build/outputs/apk/universal/release/
# sign it:   apksigner sign --ks ~/.android/debug.keystore ucr.apk
```

`cargo tauri android dev` runs it on a connected device/emulator with
hot-reload.

## How a screenless radio talks to it

Today the honest path is **through the LAN**:

```
scout / bare modem ──usb──> Pi/PC running ucr-node ──wifi──> phone app
     (the modem)              (--class relay, mux bridges)      (the screen)
```

The screenless board plugs into *any* host — a Pi on your desk, the Don in
your attic — and the phone joins the same mesh over UDP/TCP. Your keychain
has no idea the screen exists; it doesn't need to.

The next step is direct **USB-OTG serial**: plug the Scout straight into the
phone's USB-C and the app owns it — no middle host. That needs an Android
serial backend (`usb-serial-for-android` via a Tauri plugin or JNI bridge);
`ucr-transport`'s `serial` feature is already target-gated so the port has a
clean slot to land in.

## Honest status

- APK builds and signs on `main` (arm64; add `--target arm` for 32-bit).
- UI is the desktop layout — usable on a phone, not yet phone-tuned. A
  compact/touch layout pass is queued work.
- Not yet run on-device — treat v0.1 as "compiles, installs, untested in
  the hand."
