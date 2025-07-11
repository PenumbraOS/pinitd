cargo ndk -t arm64-v8a -p 28 build --release
adb shell mkdir /data/local/tmp/bin
# Binary is included in APK, not in /bin
adb push target/aarch64-linux-android/release/pinitd-cli /data/local/tmp/bin/pinitd-cli
adb shell chmod +x /data/local/tmp/bin/pinitd-cli
cp target/aarch64-linux-android/release/pinitd android/app/src/main/jniLibs/arm64-v8a/libpinitd.so
cd android && ./gradlew installDebug
adb shell pm grant com.penumbraos.pinitd android.permission.WRITE_SECURE_SETTINGS
adb shell pm grant com.penumbraos.pinitd android.permission.READ_LOGS
adb shell pm disable-user --user 0 humane.experience.systemnavigation
adb shell cmd package set-home-activity com.penumbraos.pinitd/.LauncherActivity
adb shell pm enable --user 0 humane.experience.systemnavigation