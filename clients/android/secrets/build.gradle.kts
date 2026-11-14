// SPDX-License-Identifier: MPL-2.0
// CORE-015: Android implementation of the core's PlatformSecretStore.
//
//   src/main/kotlin     KeystoreSecretStore: Android Keystore AES-256 key, AES-GCM
//                       over app-private SharedPreferences. No dependencies; the
//                       Flutter plugin pa_platform_secrets compiles this directory.
//   src/uniffi/kotlin   CorePlatformSecretStore: the UniFFI callback adapter for
//                       Kotlin users of pa-ffi (needs the CORE-004 bindings).
//
// The core comes from the CORE-004 build (tools/bindings/build-aar.sh), as in
// pa-core-android: generated Kotlin bindings and libpa_ffi.so from `paGenDir`
// (default <workspace>/target/android).
// AGP 9 compiles Kotlin itself (built-in Kotlin): no Kotlin plugin.

plugins {
    id("com.android.library") version "9.5.1"
}

val workspaceRoot = rootDir.resolve("../../..").canonicalFile
val genDir: File = providers.gradleProperty("paGenDir").orNull?.let { file(it) }
    ?: workspaceRoot.resolve("target/android")

android {
    namespace = "dev.projectalpha.secrets"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles(workspaceRoot.resolve("tools/bindings/pa-core-android/consumer-rules.pro"))
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDir("src/uniffi/kotlin")
            kotlin.srcDir(genDir.resolve("kotlin"))
            jniLibs.srcDir(genDir.resolve("jniLibs"))
        }
    }

    // Every test runs in its own process: ProcessRestartTest checks that
    // secrets written by one process are read by the next one.
    testOptions {
        execution = "ANDROIDX_TEST_ORCHESTRATOR"
    }
}

dependencies {
    // UniFFI's Kotlin runtime. JNA is dual-licensed Apache-2.0 / LGPL-2.1;
    // we use it under Apache-2.0.
    implementation("net.java.dev.jna:jna:5.19.1@aar")

    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestUtil("androidx.test:orchestrator:1.6.1")
}
