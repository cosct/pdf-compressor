plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.cosct.pdfcompressor"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.cosct.pdfcompressor"
        // versionName tracks the repo version (package.json ← sync-version.mjs);
        // versionCode bumps per release at publish time.
        versionName = "0.11.0"
        versionCode = 1
        minSdk = 26
        targetSdk = 36
        vectorDrawables { useSupportLibrary = true }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }

    sourceSets {
        // UniFFI-generated Kotlin (committed; freshness gated by
        // scripts/gen-android-bindings.sh --check). Package root: uniffi.pdfcompressor.
        getByName("main").java.srcDir("src/main/uniffi")
    }

    packaging {
        // JNA needs the .so unpacked from the APK to dlopen it.
        jniLibs { useLegacyPackaging = true }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.06.00")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.1")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    // UniFFI's generated Kotlin talks to the cdylib through JNA.
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    debugImplementation("androidx.compose.ui:ui-tooling")
}

// --- Rust integration -------------------------------------------------------
// Cross-compile the engine cdylib before merging resources. Skips with a
// warning when no NDK is configured AND libs are already present, so pure
// Kotlin iteration does not require the Rust toolchain.
val rustRelease = gradle.startParameter.taskNames.any { it.contains("Release") }
val ndkConfigured = providers.environmentVariable("ANDROID_NDK_HOME").isPresent ||
    providers.environmentVariable("ANDROID_NDK_LATEST_HOME").isPresent ||
    providers.environmentVariable("ANDROID_SDK_ROOT").isPresent ||
    providers.environmentVariable("ANDROID_HOME").isPresent
val jniLibsPresent = layout.projectDirectory
    .dir("src/main/jniLibs")
    .asFile.listFiles()?.isNotEmpty() == true

tasks.register("buildRustLibs") {
    group = "rust"
    description = "Cross-compile pdf-core-ffi into src/main/jniLibs (cargo-ndk)."
    onlyIf { ndkConfigured || !jniLibsPresent }
    doLast {
        if (!ndkConfigured) {
            logger.warn("ANDROID_NDK_HOME not set; reusing existing jniLibs (stale risk).")
            return@doLast
        }
        val script = rootProject.projectDir.parentFile.resolve("scripts/build-android-libs.sh")
        exec {
            workingDir(rootProject.projectDir.parentFile)
            commandLine(mutableListOf<String>().apply {
                add(script.absolutePath)
                if (rustRelease) add("--release")
            })
        }
    }
}

tasks.named("preBuild") { dependsOn("buildRustLibs") }
