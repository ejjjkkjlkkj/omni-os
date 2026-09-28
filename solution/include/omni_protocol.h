#ifndef OMNI_PROTOCOL_H
#define OMNI_PROTOCOL_H
#include <stddef.h>
#include <stdint.h>

#define OMNI_MM_MAGIC UINT32_C(0x314D4D4F)
#define OMNI_MM_VERSION UINT16_C(1)
#define OMNI_MM_MAX_PAYLOAD UINT32_C(4096)

typedef enum {
    OMNI_OK = 0,
    OMNI_ERR_NULL = 1,
    OMNI_ERR_SIZE = 2,
    OMNI_ERR_MAGIC = 3,
    OMNI_ERR_VERSION = 4,
    OMNI_ERR_COMMAND = 5,
    OMNI_ERR_SECRET = 6
} omni_status;

typedef struct {
    uint32_t magic;
    uint16_t version;
    uint16_t command;
    uint32_t payload_size;
    uint32_t flags;
} omni_mm_header;

typedef struct {
    uint64_t id;
    uint64_t parent_id;
    uint32_t role;
    uint32_t state;
    uint32_t name_length;
    uint32_t value_length;
} omni_a11y_node;

enum { OMNI_A11Y_PASSWORD = 1u << 0 };

/*@
  requires header == \null || \valid_read(header);
  ensures \result == OMNI_OK ==> header != \null;
  ensures \result == OMNI_OK ==> buffer_size >= sizeof(omni_mm_header);
  ensures \result == OMNI_OK ==> header->magic == OMNI_MM_MAGIC;
  ensures \result == OMNI_OK ==> header->version == OMNI_MM_VERSION;
  ensures \result == OMNI_OK ==> header->payload_size <= OMNI_MM_MAX_PAYLOAD;
  ensures \result == OMNI_OK ==> header->payload_size <= buffer_size - sizeof(omni_mm_header);
  ensures \result == OMNI_OK ==> header->command >= 1 && header->command <= 16;
  ensures header != \null &&
          buffer_size >= sizeof(omni_mm_header) &&
          header->magic == OMNI_MM_MAGIC &&
          header->version == OMNI_MM_VERSION &&
          header->payload_size <= OMNI_MM_MAX_PAYLOAD &&
          header->payload_size <= buffer_size - sizeof(omni_mm_header) &&
          1 <= header->command <= 16 ==> \result == OMNI_OK;
  assigns \nothing;
*/
omni_status omni_mm_validate(const omni_mm_header *header, size_t buffer_size);

/*@
  requires node == \null || \valid_read(node);
  ensures \result == OMNI_OK ==> node != \null;
  ensures \result == OMNI_OK ==> node->name_length > 0;
  ensures \result == OMNI_OK ==> ((node->state & OMNI_A11Y_PASSWORD) == 0 || node->value_length == 0);
  ensures node != \null &&
          node->name_length > 0 &&
          ((node->state & OMNI_A11Y_PASSWORD) == 0 || node->value_length == 0)
          ==> \result == OMNI_OK;
  ensures node != \null && node->name_length > 0 &&
          (node->state & OMNI_A11Y_PASSWORD) != 0 && node->value_length != 0
          ==> \result == OMNI_ERR_SECRET;
  assigns \nothing;
*/
omni_status omni_a11y_validate(const omni_a11y_node *node);

#endif
