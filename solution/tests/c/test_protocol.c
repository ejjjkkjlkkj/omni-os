#include <assert.h>
#include <string.h>
#include "omni_protocol.h"

static void test_mm(void) {
    unsigned char storage[sizeof(omni_mm_header) + 8] = {0};
    omni_mm_header *h = (omni_mm_header *)storage;

    assert(omni_mm_validate(NULL, 0) == OMNI_ERR_NULL);

    h->magic = OMNI_MM_MAGIC;
    h->version = OMNI_MM_VERSION;
    h->command = 1;
    h->payload_size = 8;

    assert(omni_mm_validate(h, sizeof(*h) - 1) == OMNI_ERR_SIZE);

    h->magic = 0;
    assert(omni_mm_validate(h, sizeof(storage)) == OMNI_ERR_MAGIC);
    h->magic = OMNI_MM_MAGIC;

    h->version = OMNI_MM_VERSION + 1;
    assert(omni_mm_validate(h, sizeof(storage)) == OMNI_ERR_VERSION);
    h->version = OMNI_MM_VERSION;

    h->payload_size = OMNI_MM_MAX_PAYLOAD + 1;
    assert(omni_mm_validate(h, sizeof(storage)) == OMNI_ERR_SIZE);

    h->payload_size = 8;
    assert(omni_mm_validate(h, sizeof(*h) + 7) == OMNI_ERR_SIZE);

    h->payload_size = 0;
    h->command = 0;
    assert(omni_mm_validate(h, sizeof(*h)) == OMNI_ERR_COMMAND);

    h->command = 17;
    assert(omni_mm_validate(h, sizeof(*h)) == OMNI_ERR_COMMAND);

    h->command = 1;
    h->payload_size = 8;
    assert(omni_mm_validate(h, sizeof(storage)) == OMNI_OK);
}

static void test_a11y(void) {
    assert(omni_a11y_validate(NULL) == OMNI_ERR_NULL);

    omni_a11y_node node = { .id = 1, .name_length = 0, .value_length = 0 };
    assert(omni_a11y_validate(&node) == OMNI_ERR_SIZE);

    node.name_length = 4;
    node.state = OMNI_A11Y_PASSWORD;
    node.value_length = 6;
    assert(omni_a11y_validate(&node) == OMNI_ERR_SECRET);

    node.value_length = 0;
    assert(omni_a11y_validate(&node) == OMNI_OK);

    node.state = 0;
    node.value_length = 12;
    assert(omni_a11y_validate(&node) == OMNI_OK);
}

int main(void) {
    test_mm();
    test_a11y();
    return 0;
}
