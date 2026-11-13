// SPDX-License-Identifier: MPL-2.0
// Compiles the generated Kotlin bindings (target/bindings/kotlin) against JNA
// and calls the host cdylib (target/debug) from a JUnit test on the JVM.

plugins {
    kotlin("jvm") version "2.2.20"
}

repositories {
    mavenCentral()
}

val workspaceRoot = rootDir.resolve("../../../..").canonicalFile

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0")
    testImplementation(kotlin("test"))
}

kotlin {
    jvmToolchain(21)
    sourceSets["main"].kotlin.srcDir(workspaceRoot.resolve("target/bindings/kotlin"))
}

tasks.test {
    useJUnitPlatform()
    systemProperty("jna.library.path", workspaceRoot.resolve("target/debug").path)
    testLogging { events("passed", "failed") }
}
