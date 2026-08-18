# Node runtime feature

This module is intentionally empty in source control. A separately proven Android ARM64 Node 24.19.0 payload is staged into `build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so` only when producing the Play App Bundle. The module is delivered on demand by Google Play and is not part of the base APK.

For phone-side testing outside Google Play, the Play-bundle workflow may additionally produce a **sideload Alpha APK** after the production AAB has already been built and verified. That diagnostic APK copies the same evidence-bound Node payload into the base APK only for sideload testing. The production AAB remains on-demand and its verifier rejects any Node payload in the base module.
