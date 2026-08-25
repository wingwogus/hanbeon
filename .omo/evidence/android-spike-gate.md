# Android Spike Gate Evidence

## INIT_RESULT

PASS. Ran from `apps/desktop` with `PATH=$HOME/.cargo/bin:$PATH` and `ANDROID_HOME=$HOME/Library/Android/sdk`:

```text
cargo tauri android init
Info Using installed NDK: /Users/wingwogus/Library/Android/sdk/ndk/26.3.11579264
Info Installing Android Rust targets...
Generating Android Studio project...
victory: Project generated successfully!
```

The generated Android project is at `apps/desktop/src-tauri/gen/android/`.

## BUILD_RESULT

NOT YET PASSING; command exited with code `1`:

```text
cargo tauri android build --debug --target aarch64
```

The Android build did not reach Gradle, Rust compilation, or APK packaging. Tauri invoked the configured `beforeBuildCommand` (`bun run build`), which ran `next build` and failed during TypeScript processing:

```text
Cannot read properties of undefined (reading 'getCurrentDirectory')
Next.js build worker exited with code: 1 and signal: null
error: script "build" exited with code 1
Error beforeBuildCommand `bun run build` failed with exit code 1
```

## FAIL_REASON

No APK was produced because the desktop front-end `beforeBuildCommand` failed before the Android-specific build pipeline began. This is not a structural Android/Tauri integration failure: Android initialization completed and generated the native Gradle project using the installed NDK.

## BRANCH_DECISION

**T - Tauri-plugin path.** The generated Tauri Android project is present and the observed failure is confined to the existing Next.js production build. After that application build issue is fixed, the Tauri Android build and install path remains plausible. There is no evidence requiring the pure-Gradle fallback (G).

## NEXT_STEPS

1. Diagnose and fix the Next.js TypeScript build-worker error (`getCurrentDirectory`) outside this Android scaffold task.
2. Re-run `PATH=$HOME/.cargo/bin:$PATH ANDROID_HOME=$HOME/Library/Android/sdk cargo tauri android build --debug --target aarch64` from `apps/desktop`.
3. On success, record the resulting APK path and install it on an aarch64 Android emulator or device to validate the T branch end to end.
