# Keep the UniFFI-generated bindings: they talk to native code through JNA
# reflection-style lookups that R8 cannot see.
-keep class uniffi.pdfcompressor.** { *; }
-dontwarn com.sun.jna.**
