// glibc >= 2.38 redirects strtol/strtoul/sscanf to __isoc23_* when the code is
// compiled with _GNU_SOURCE (which the cc crate and CMake define on Linux).
// Those __isoc23_* symbols require GLIBC_2.38, but 'shop' runs Debian 12
// (glibc 2.36), so a binary referencing them would not start there.
//
// These shims provide the same entry points with pre-C23 semantics, which are
// identical for the decimal/hex parsing used by libusb and paho.mqtt.c. They
// rely on the old, universally available symbols (strtol@GLIBC_2.4 etc.).
//
// Deliberately NOT including <stdlib.h>/<stdio.h>: their feature-test logic
// would redirect the calls below back onto these very shims (recursion).
// <stdarg.h> is a compiler header and is safe.
//
// Compile per cross target (e.g. arm-linux-gnueabihf-gcc -std=gnu11 -fPIC -c
// isoc23_shim.c) and pass to rustc via
// CARGO_TARGET_<TRIPLE>_RUSTFLAGS="-C link-arg=<this>.o".

#include <stdarg.h>

extern long strtol(const char *restrict nptr, char **restrict endptr, int base);
extern unsigned long strtoul(const char *restrict nptr, char **restrict endptr, int base);
extern int vsscanf(const char *restrict s, const char *restrict format, va_list arg);

long __isoc23_strtol(const char *restrict nptr, char **restrict endptr, int base) {
    return strtol(nptr, endptr, base);
}

unsigned long __isoc23_strtoul(const char *restrict nptr, char **restrict endptr, int base) {
    return strtoul(nptr, endptr, base);
}

int __isoc23_sscanf(const char *restrict s, const char *restrict format, ...) {
    va_list ap;
    va_start(ap, format);
    int result = vsscanf(s, format, ap);
    va_end(ap);
    return result;
}