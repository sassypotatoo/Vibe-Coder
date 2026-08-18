VibeCoder intentionally ships no fake native payloads in source control.

Generated base-app native outputs belong under android/app/build/generated/jniLibs/arm64-v8a:
- libvibecoder_android_host.so
- libvibecoder_jcode_exec.so
- later Android-build binaries

Node is NOT a base-app payload. The independently proven Node 24.19.0 Android ARM64 artifact is
staged only into android/node_runtime/build/generated/jniLibs/arm64-v8a for the Play on-demand
node_runtime feature module. Do not copy libvibecoder_node_exec.so into the base app JNI tree.

The base APK remains buildable without Node and reports Node as setup-required until the feature is
installed.

Sideload diagnostic exception: a separately proven Node payload may be staged only under
android/app/build/generated/jniLibs at build time for the explicit sideload Alpha lane.
Production Play AAB generation removes any base Node payload and keeps node_runtime on demand.
