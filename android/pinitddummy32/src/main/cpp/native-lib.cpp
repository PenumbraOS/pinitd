#include <jni.h> // Include JNI header
// No Java_ methods needed if you don't plan to call it

// Optional: a minimal dummy function
extern "C" JNIEXPORT jint JNICALL Java_com_com_penumbraos_pinitddummy32_dummyNativeFunction(
        JNIEnv* env,
        jobject /* this */) {
    // Does nothing
    return 0;
}

// Alternatively, you can just have the file with includes and no functions.
// The build system just needs a source file to compile into a .so