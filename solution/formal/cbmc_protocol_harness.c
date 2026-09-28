#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include "omni_protocol.h"

extern uint32_t nondet_uint32_t(void);
extern uint16_t nondet_uint16_t(void);
extern size_t nondet_size_t(void);

int main(void) {
    omni_mm_header h;
    h.magic = nondet_uint32_t();
    h.version = nondet_uint16_t();
    h.command = nondet_uint16_t();
    h.payload_size = nondet_uint32_t();
    h.flags = nondet_uint32_t();
    size_t buffer_size = nondet_size_t();
    __CPROVER_assume(buffer_size <= sizeof(h) + OMNI_MM_MAX_PAYLOAD + 64u);
    omni_status s = omni_mm_validate(&h, buffer_size);
    if (s == OMNI_OK) {
        assert(buffer_size >= sizeof(h));
        assert(h.magic == OMNI_MM_MAGIC);
        assert(h.version == OMNI_MM_VERSION);
        assert(h.payload_size <= OMNI_MM_MAX_PAYLOAD);
        assert((size_t)h.payload_size <= buffer_size - sizeof(h));
        assert(h.command >= 1 && h.command <= 16);
    }
    return 0;
}
