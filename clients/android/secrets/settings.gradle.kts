// SPDX-License-Identifier: MPL-2.0
// CORE-015: Android secret store (Keystore + AES-GCM). Build and test with
// `just android secrets-test-device` (clients/android/justfile).

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

rootProject.name = "pa-secrets-android"
