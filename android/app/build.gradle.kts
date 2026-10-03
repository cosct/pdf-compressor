import org.gradle.api.tasks.Input
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.TaskAction
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property

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
        // Native libraries are generated separately per variant; never package
        // the old shared src/main/jniLibs directory by accident.
        getByName("main").jniLibs.setSrcDirs(emptyList<String>())
    }

    packaging {
        // JNA needs the .so unpacked from the APK to dlopen it.
        jniLibs { useLegacyPackaging = true }
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
    testImplementation("org.mockito:mockito-core:5.18.0")
    val composeBom = platform("androidx.compose:compose-bom:2025.06.00")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.1")
    // Flow state survives recreation (rotation, theme/locale switches).
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.1")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    // Settings persistence (preset choice, theme, language).
    implementation("androidx.datastore:datastore-preferences:1.1.7")
    // Background queue (Phase 4): process-death-proof work chain.
    implementation("androidx.work:work-runtime-ktx:2.10.3")
    // Writes queue outputs into the user-picked document tree.
    implementation("androidx.documentfile:documentfile:1.1.0")
    // Per-app language switching (AppCompatDelegate.setApplicationLocales)
    // back-ported below API 33; theme is DayNight-aware through appcompat.
    implementation("androidx.appcompat:appcompat:1.7.1")
    // UniFFI's generated Kotlin talks to the cdylib through JNA.
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    debugImplementation("androidx.compose.ui:ui-tooling")
}

// Each variant gets its own task and output directory. Missing toolchains
// or ABIs fail the build, including when invoked via generic assemble/build.
abstract class RustJniTask : DefaultTask() {
    @get:Input abstract val releaseBuild: Property<Boolean>
    @get:OutputDirectory abstract val outputDirectory: DirectoryProperty

    @TaskAction fun buildLibraries() {
        val repo = project.rootProject.projectDir.parentFile
        project.exec {
            workingDir(repo)
            commandLine(buildList {
                add(repo.resolve("scripts/build-android-libs.sh").absolutePath)
                if (releaseBuild.get()) add("--release")
                add("--out-dir")
                add(outputDirectory.get().asFile.absolutePath)
            })
        }
        listOf("arm64-v8a", "x86_64").forEach { abi ->
            check(outputDirectory.file("$abi/libpdf_core_ffi.so").get().asFile.length() > 0) {
                "Missing native engine for $abi"
            }
        }
    }
}

androidComponents.onVariants { variant ->
    val rust = tasks.register<RustJniTask>("buildRust${variant.name.replaceFirstChar { it.uppercase() }}") {
        releaseBuild.set(variant.buildType == "release")
        outputDirectory.set(layout.buildDirectory.dir("generated/rust/${variant.name}"))
        // Always let cargo validate its own dependency graph/toolchain.
        outputs.upToDateWhen { false }
    }
    variant.sources.jniLibs?.addGeneratedSourceDirectory(rust, RustJniTask::outputDirectory)
}
