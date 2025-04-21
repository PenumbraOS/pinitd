cargo ndk -t arm64-v8a -p 28 build --release
adb push target/aarch64-linux-android/release/pinit /data/local/tmp/pinit