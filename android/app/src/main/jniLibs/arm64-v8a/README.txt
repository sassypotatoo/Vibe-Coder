VibeCoder intentionally ships no fake native payloads in source control.

Generated base-app native outputs belong under android/app/build/generated/jniLibs/arm64-v8a:
- libvibecoder_android_host.so
- libvibecoder_jcode_exec.so
- later Android-build binaries

Node is NOT a base-app payload. The independently proven Node 24.19.0 Android ARM64 artifact is
staged only into android/node_runtime/build/generated/jniLibs/arm64-v8a when producing the downloadable
node_runtime split. The base APK never packages libvibecoder_node_exec.so; first-run setup downloads it separately.

The base APK remains buildable without Node and reports Node as setup-required until the feature is
installed.
