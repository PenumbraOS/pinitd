cmake_minimum_required(VERSION 3.18.1) # Use an appropriate NDK version

project("my_minimal_native_lib") # Project name

add_library(native-lib SHARED native-lib.cpp)
