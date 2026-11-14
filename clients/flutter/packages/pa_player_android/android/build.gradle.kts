// SPDX-License-Identifier: MPL-2.0
// pa_player_android (PLAY-AND-002): Flutter side of the Android player.
//
// Kotlin sources:
//   src/main/kotlin                            PlayerActivity, OSD channels, plugin
//   clients/android/player/src/main/kotlin     Media3Engine (PlayerEngine contract)
//   <paGenDir>/kotlin + jniLibs                UniFFI bindings and libpa_ffi.so of the core
//                                              (tools/bindings/build-aar.sh, CORE-004)
// paGenDir defaults to <workspace>/target/android, as in pa-core-android. An app
// using this plugin must not also add the pa-core-android AAR (same classes).

group = "dev.projectalpha.player.flutter"
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
val genDir: File = (findProperty("paGenDir") as String?)?.let { file(it) } ?: workspaceRoot.resolve("target/android")

android {
    namespace = "dev.projectalpha.player.flutter"
    compileSdk = 36

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDir("src/main/kotlin")
            kotlin.srcDir(workspaceRoot.resolve("clients/android/player/src/main/kotlin"))
            kotlin.srcDir(genDir.resolve("kotlin"))
            jniLibs.srcDir(genDir.resolve("jniLibs"))
        }
    }

    defaultConfig {
        minSdk = 24
        consumerProguardFiles(workspaceRoot.resolve("tools/bindings/pa-core-android/consumer-rules.pro"))
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    val media3 = "1.11.1"
    implementation("androidx.media3:media3-exoplayer:$media3")
    // PlayerView: the native OSD fallback (variant A of ADR-0008).
    implementation("androidx.media3:media3-ui:$media3")
    implementation("androidx.fragment:fragment-ktx:1.8.9")
    implementation("androidx.annotation:annotation:1.9.1")
    // UniFFI's Kotlin runtime. JNA is dual-licensed Apache-2.0 / LGPL-2.1; we use it under Apache-2.0.
    implementation("net.java.dev.jna:jna:5.19.1@aar")
}
