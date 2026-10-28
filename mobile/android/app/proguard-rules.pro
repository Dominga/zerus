# Optional Firebase entry point is called reflectively only in private Firebase builds.
-keep class app.zerus.mobile.OptionalFirebase { public static *; }

# The task replica's UniFFI bindings call the Rust library through JNA, which finds
# its classes and the bindings' structures by reflection.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class app.zerus.mobile.replica.** { *; }
-dontwarn java.awt.**

# Tink 1.24.0's optional X-Wing implementation uses API 37 / ART extension 22
# classes absent from compileSdk 36. WorkspaceCrypto fixes the KEM to X25519,
# so it never selects that guarded X-Wing path.
-dontwarn android.crypto.hpke.Hpke
-dontwarn android.crypto.hpke.Message
