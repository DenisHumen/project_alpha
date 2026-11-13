// SPDX-License-Identifier: MPL-2.0
// PLAY-AND-002: Media3/ExoPlayer engine behind the core PlayerEngine contract
// (ADR-0017), talking to the core through the UniFFI bindings of pa-ffi.
//
// The core comes from the CORE-004 build (tools/bindings/build-aar.sh), as in
// pa-core-android: generated Kotlin bindings and libpa_ffi.so from `paGenDir`
// (default <workspace>/target/android). Instrumented-test media come from
// tools/make-fixtures.sh in `paFixturesDir` (default build/fixtures).
// AGP 9 compiles Kotlin itself (built-in Kotlin): no Kotlin plugin.

plugins {
    id("com.android.library") version "9.5.1"
}

val workspaceRoot = rootDir.resolve("../../..").canonicalFile
val genDir: File = providers.gradleProperty("paGenDir").orNull?.let { file(it) }
    ?: workspaceRoot.resolve("target/android")
val fixturesDir: File = providers.gradleProperty("paFixturesDir").orNull?.let { file(it) }
    ?: layout.buildDirectory.dir("fixtures").get().asFile

android {
    namespace = "dev.projectalpha.player"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // JNA and the UniFFI bindings are found by reflection (same rules as pa-core-android).
        consumerProguardFiles(workspaceRoot.resolve("tools/bindings/pa-core-android/consumer-rules.pro"))
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDir(genDir.resolve("kotlin"))
            jniLibs.srcDir(genDir.resolve("jniLibs"))
        }
        getByName("androidTest") {
            assets.srcDir(fixturesDir)
        }
    }
}

dependencies {
    val media3 = "1.11.1"
    api("androidx.media3:media3-exoplayer:$media3")
    // UniFFI's Kotlin runtime. JNA is dual-licensed Apache-2.0 / LGPL-2.1; we use it under Apache-2.0.
    api("net.java.dev.jna:jna:5.19.1@aar")

    testImplementation("junit:junit:4.13.2")

    androidTestImplementation("androidx.test:core:1.7.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
}
