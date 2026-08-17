#pragma once

#include <cstdio>
#include <string>

namespace check {

inline int failures = 0;
inline int checks = 0;

template <typename T>
std::string str(const T& v) {
    return std::to_string(v);
}

inline void report(bool ok, const char* expr, const char* file, int line, const std::string& extra) {
    ++checks;
    if (ok) return;
    ++failures;
    std::fprintf(stderr, "FAIL %s:%d\n  %s\n", file, line, expr);
    if (!extra.empty()) std::fprintf(stderr, "  %s\n", extra.c_str());
}

inline int summary() {
    std::fprintf(stderr, "%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}

}  // namespace check

#define CHECK(expr) ::check::report((expr), #expr, __FILE__, __LINE__, "")

// Avalia cada lado uma única vez: os argumentos podem ter efeito colateral (escrever no
// ring buffer, por exemplo), e uma macro que os avalia duas vezes falseia o teste.
#define CHECK_EQ(a, b)                                                                        \
    do {                                                                                      \
        const auto checkGot = (a);                                                            \
        const auto checkWant = (b);                                                           \
        ::check::report(checkGot == checkWant, #a " == " #b, __FILE__, __LINE__,               \
                        "got: " + ::check::str(checkGot) + " want: " + ::check::str(checkWant)); \
    } while (0)
