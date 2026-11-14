// SPDX-License-Identifier: MPL-2.0
// pa_platform_secrets (CORE-015): Flutter channel of the Android secret store.
//
// Kotlin sources:
//   src/main/kotlin                              PaPlatformSecretsPlugin (channel)
//   clients/android/secrets/src/main/kotlin      KeystoreSecretStore (tested there
//                                                on an emulator: just android secrets-test-device)
// No UniFFI bindings here: the Flutter app reaches the core through
// flutter_rust_bridge, and pa_player_android already ships pa-ffi.

group = "dev.projectalpha.secrets.flutter"
version = "0.1.0"

buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.3.1")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.20")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

plugins {
    id("com.android.library")
}

val workspaceRoot: File = projectDir.resolve("../../../../..").canonicalFile

android {
    namespace = "dev.projectalpha.secrets.flutter"
    compileSdk = 36

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDir("src/main/kotlin")
            kotlin.srcDir(workspaceRoot.resolve("clients/android/secrets/src/main/kotlin"))
        }
    }

    defaultConfig {
        minSdk = 24
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}
