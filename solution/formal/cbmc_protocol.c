#include <assert.h>
#include <stddef.h>
#include "omni_protocol.h"

static void prove_mm_contract(void) {
    omni_mm_header h;
    size_t size;
    omni_status result = omni_mm_validate(&h, size);

    if (result == OMNI_OK) {
        assert(size >= sizeof(h));
        assert(h.magic == OMNI_MM_MAGIC);
        assert(h.version == OMNI_MM_VERSION);
        assert(h.payload_size <= OMNI_MM_MAX_PAYLOAD);
        assert((size_t)h.payload_size <= size - sizeof(h));
        assert(h.command >= 1 && h.command <= 16);
    }
}

static void prove_password_redaction(void) {
    omni_a11y_node node;
    omni_status result = omni_a11y_validate(&node);

    if ((node.state & OMNI_A11Y_PASSWORD) != 0 && node.value_length != 0) {
        assert(result != OMNI_OK);
    }
    if (result == OMNI_OK) {
        assert(node.name_length > 0);
        assert((node.state & OMNI_A11Y_PASSWORD) == 0 || node.value_length == 0);
    }
}


static void prove_non_vacuity_and_boundaries(void) {
    omni_mm_header good = {
        .magic = OMNI_MM_MAGIC,
        .version = OMNI_MM_VERSION,
        .command = 1,
        .payload_size = 0,
        .flags = 0,
    };
    assert(omni_mm_validate(&good, sizeof(good)) == OMNI_OK);

    good.command = 16;
    good.payload_size = OMNI_MM_MAX_PAYLOAD;
    assert(omni_mm_validate(&good, sizeof(good) + OMNI_MM_MAX_PAYLOAD) == OMNI_OK);

    good.command = 17;
    assert(omni_mm_validate(&good, sizeof(good) + OMNI_MM_MAX_PAYLOAD) == OMNI_ERR_COMMAND);

    good.command = 1;
    good.magic ^= UINT32_C(1);
    assert(omni_mm_validate(&good, sizeof(good)) == OMNI_ERR_MAGIC);

    omni_a11y_node node = {
        .id = 1,
        .parent_id = 0,
        .role = 1,
        .state = OMNI_A11Y_PASSWORD,
        .name_length = 1,
        .value_length = 0,
    };
    assert(omni_a11y_validate(&node) == OMNI_OK);

    node.value_length = 1;
    assert(omni_a11y_validate(&node) == OMNI_ERR_SECRET);

    node.state = 0;
    node.value_length = 0;
    node.name_length = 0;
    assert(omni_a11y_validate(&node) == OMNI_ERR_SIZE);
}

int main(void) {
    prove_mm_contract();
    prove_password_redaction();
    prove_non_vacuity_and_boundaries();
    return 0;
}
