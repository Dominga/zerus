cmake_minimum_required(VERSION 3.20)
project(demo CXX)

add_library(demo src/math.cpp)
target_include_directories(demo PUBLIC src)

add_executable(demo_test tests/test_math.cpp)
target_link_libraries(demo_test PRIVATE demo)

enable_testing()
add_test(NAME math.add COMMAND demo_test)
