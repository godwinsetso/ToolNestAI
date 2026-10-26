# JNA and the UniFFI bindings are called reflectively from native code.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class app.readit.core.** { *; }
-dontwarn java.awt.**
# sherpa-onnx JNI classes
-keep class com.k2fsa.sherpa.onnx.** { *; }
