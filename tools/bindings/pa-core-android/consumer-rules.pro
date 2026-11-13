# SPDX-License-Identifier: MPL-2.0
# R8/ProGuard rules shipped inside the AAR for apps that minify.
# The UniFFI bindings call the Rust library through JNA, which finds
# structures, callbacks and native methods by reflection: keep both sides.
-dontwarn java.awt.**
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keep class dev.projectalpha.core.** { *; }
