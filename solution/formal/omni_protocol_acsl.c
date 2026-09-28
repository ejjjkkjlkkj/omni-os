#include "omni_protocol.h"

/*@
  requires header == \null || \valid_read(header);
  ensures \result == OMNI_OK ==>
      header != \null &&
      buffer_size >= sizeof(*header) &&
      header->magic == OMNI_MM_MAGIC &&
      header->version == OMNI_MM_VERSION &&
      header->payload_size <= OMNI_MM_MAX_PAYLOAD &&
      header->payload_size <= buffer_size - sizeof(*header) &&
      1 <= header->command <= 16;
  ensures header != \null &&
      buffer_size >= sizeof(*header) &&
      header->magic == OMNI_MM_MAGIC &&
      header->version == OMNI_MM_VERSION &&
      header->payload_size <= OMNI_MM_MAX_PAYLOAD &&
      header->payload_size <= buffer_size - sizeof(*header) &&
      1 <= header->command <= 16 ==> \result == OMNI_OK;
  assigns \nothing;
*/
omni_status omni_mm_validate_contract(const omni_mm_header *header, size_t buffer_size) {
    return omni_mm_validate(header, buffer_size);
}

/*@
  requires node == \null || \valid_read(node);
  ensures \result == OMNI_OK ==> node != \null && node->name_length > 0;
  ensures \result == OMNI_OK && (node->state & OMNI_A11Y_PASSWORD) != 0 ==>
      node->value_length == 0;
  ensures node != \null && node->name_length > 0 &&
      ((node->state & OMNI_A11Y_PASSWORD) == 0 || node->value_length == 0)
      ==> \result == OMNI_OK;
  ensures node != \null && node->name_length > 0 &&
      (node->state & OMNI_A11Y_PASSWORD) != 0 && node->value_length != 0
      ==> \result == OMNI_ERR_SECRET;
  assigns \nothing;
*/
omni_status omni_a11y_validate_contract(const omni_a11y_node *node) {
    return omni_a11y_validate(node);
}
