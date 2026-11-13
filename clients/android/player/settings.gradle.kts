// SPDX-License-Identifier: MPL-2.0
// PLAY-AND-002: Android library with the Media3 engine of the PlayerEngine
// contract. Build and test with `just android …` (clients/android/justfile).

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

rootProject.name = "pa-player-android"
