#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "omni_protocol.h"

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    omni_mm_header h;
    if (size < sizeof(h)) {
        (void)omni_mm_validate(NULL, size);
        return 0;
    }
    memcpy(&h, data, sizeof(h));
    (void)omni_mm_validate(&h, size);
    return 0;
}
