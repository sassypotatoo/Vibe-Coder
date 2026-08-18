# Node runtime split

This module is empty in source control. A separately proven Android ARM64 Node 24.19.0 payload is staged into `build/generated/jniLibs/arm64-v8a/libvibecoder_node_exec.so` only when producing the downloadable development runtime package.

The resulting signed same-package split APK is published as a fixed GitHub Release asset. VibeCoder downloads it during first-run setup and installs it with Android PackageInstaller. Node is not part of the base Alpha APK and normal app builds do not compile Node.
