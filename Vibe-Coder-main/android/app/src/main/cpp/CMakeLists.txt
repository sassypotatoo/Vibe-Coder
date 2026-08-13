cmake_minimum_required(VERSION 3.22.1)
project(vibecoder_shell_jni C)

add_library(vibecoder_shell_jni SHARED native_bridge.c)

target_compile_options(vibecoder_shell_jni PRIVATE
    -Wall
    -Wextra
    -Werror
    -fvisibility=hidden
)

# NDK r28+ already defaults to flexible/16 KiB-compatible pages. Keep explicit alignment here so
# this tiny bridge remains auditable if the toolchain default changes later.
target_link_options(vibecoder_shell_jni PRIVATE
    "-Wl,-z,max-page-size=16384"
    "-Wl,-z,common-page-size=16384"
)

target_link_libraries(vibecoder_shell_jni PRIVATE dl log)
