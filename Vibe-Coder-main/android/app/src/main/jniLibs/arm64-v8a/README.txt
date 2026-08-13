Part 28 intentionally ships no fake native payloads in source control.

The provisioning/build scripts place verified outputs here:
- libvibecoder_android_host.so
- libvibecoder_jcode_exec.so
- libvibecoder_node_exec.so
- later Android-build binaries

The APK shell remains buildable without these files and reports them as NOT READY at runtime.
