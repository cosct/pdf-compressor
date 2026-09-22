// Root build: plugin versions only (applied per-module). The Rust cdylib
// and UniFFI bindings live outside Gradle — see scripts/build-android-libs.sh
// (wired into app/preBuild) and scripts/gen-android-bindings.sh.
plugins {
    id("com.android.application") version "8.10.0" apply false
    id("org.jetbrains.kotlin.android") version "2.2.0" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.0" apply false
}
