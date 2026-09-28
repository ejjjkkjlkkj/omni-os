#include "omni_protocol.h"

omni_status omni_mm_validate(const omni_mm_header *header, size_t buffer_size) {
    if (header == NULL) return OMNI_ERR_NULL;
    if (buffer_size < sizeof(*header)) return OMNI_ERR_SIZE;
    if (header->magic != OMNI_MM_MAGIC) return OMNI_ERR_MAGIC;
    if (header->version != OMNI_MM_VERSION) return OMNI_ERR_VERSION;
    if (header->payload_size > OMNI_MM_MAX_PAYLOAD) return OMNI_ERR_SIZE;
    if ((size_t)header->payload_size > buffer_size - sizeof(*header)) return OMNI_ERR_SIZE;
    if (header->command == 0 || header->command > 16) return OMNI_ERR_COMMAND;
    return OMNI_OK;
}

omni_status omni_a11y_validate(const omni_a11y_node *node) {
    if (node == NULL) return OMNI_ERR_NULL;
    if (node->name_length == 0) return OMNI_ERR_SIZE;
    if ((node->state & OMNI_A11Y_PASSWORD) != 0 && node->value_length != 0) return OMNI_ERR_SECRET;
    return OMNI_OK;
}
