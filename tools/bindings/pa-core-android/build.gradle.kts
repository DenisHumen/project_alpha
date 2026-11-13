// SPDX-License-Identifier: MPL-2.0
// CORE-004: AAR with the Rust core (pa-ffi) and its UniFFI Kotlin bindings.
//
// Native libraries and Kotlin sources are generated, never committed
// (ADR-0001). tools/bindings/build-aar.sh writes them to `paGenDir`
// (default <workspace>/target/android) and then runs this build:
//   <paGenDir>/jniLibs/<abi>/libpa_ffi.so    cargo-ndk
//   <paGenDir>/kotlin/dev/projectalpha/core  uniffi-bindgen --language kotlin
// AGP 9 compiles Kotlin itself (built-in Kotlin): no Kotlin plugin.

plugins {
    id("com.android.library") version "9.5.1"
}

val workspaceRoot = rootDir.resolve("../../..").canonicalFile
val genDir: File = providers.gradleProperty("paGenDir").orNull?.let { file(it) }
    ?: workspaceRoot.resolve("target/android")

android {
    namespace = "dev.projectalpha.core"
    compileSdk = 36
    // Same NDK as cargo-ndk in build-aar.sh; AGP uses its llvm-strip on the .so.
    ndkVersion = "29.0.14206865"

    defaultConfig {
        minSdk = 24
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
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
    }
}

dependencies {
    // UniFFI's Kotlin runtime. JNA is dual-licensed Apache-2.0 / LGPL-2.1;
    // we use it under Apache-2.0.
    api("net.java.dev.jna:jna:5.19.1@aar")

    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
}
