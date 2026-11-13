// SPDX-License-Identifier: MPL-2.0
// CORE-004: Android library `pa-core-android` (AAR): the Rust core (pa-ffi)
// as jniLibs for arm64-v8a, armeabi-v7a and x86_64 plus the UniFFI Kotlin
// bindings. Build with `just core aar` (tools/bindings/build-aar.sh).

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "pa-core-android"
