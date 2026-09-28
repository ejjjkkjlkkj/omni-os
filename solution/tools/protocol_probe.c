#include "omni_protocol.h"
int main(void) {
    omni_mm_header h = { OMNI_MM_MAGIC, OMNI_MM_VERSION, 1, 0, 0 };
    omni_a11y_node n = { 1, 0, 1, 0, 1, 0 };
    return (omni_mm_validate(&h, sizeof(h)) == OMNI_OK &&
            omni_a11y_validate(&n) == OMNI_OK) ? 0 : 1;
}
