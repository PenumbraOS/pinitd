cargo ndk -t arm64-v8a -p 28 build --release
adb push target/aarch64-linux-android/release/pinitd /data/local/tmp/pinitd
cp target/aarch64-linux-android/release/pinitd android/app/src/main/jniLibs/arm64-v8a/libpinitd.so