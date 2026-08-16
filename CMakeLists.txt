cmake_minimum_required(VERSION 3.20)
project(hog_audio CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()

add_compile_options(-Wall -Wextra -Wpedantic)

# O ring buffer roda em thread de tempo real: um transbordo ali corrompe memória em silêncio.
# Os sanitizers tornam esse erro visível nos testes.
option(HOG_SANITIZE "Compilar com AddressSanitizer e UndefinedBehaviorSanitizer" OFF)
if(HOG_SANITIZE)
    add_compile_options(-fsanitize=address,undefined -fno-omit-frame-pointer -g)
    add_link_options(-fsanitize=address,undefined)
endif()

# Núcleo puro: nenhuma dependência de framework, para que os testes rodem sem hardware.
add_library(hog_core STATIC
    src/format_negotiation.cpp
)
target_include_directories(hog_core PUBLIC src)

enable_testing()

foreach(test_name format_negotiation ring_buffer)
    add_executable(test_${test_name} tests/test_${test_name}.cpp)
    target_link_libraries(test_${test_name} PRIVATE hog_core)
    target_include_directories(test_${test_name} PRIVATE tests)
    add_test(NAME ${test_name} COMMAND test_${test_name})
endforeach()
