# Keep the UniFFI-generated bindings: they talk to native code through JNA
# reflection-style lookups that R8 cannot see.
-keep class uniffi.pdfcompressor.** { *; }
# JNA resolves its own native entry points by reflection (official consumer
# rules) — without these the release build minifies the bridge away.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-dontwarn com.sun.jna.**
