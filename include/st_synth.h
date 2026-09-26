#ifndef ST_SYNTH_H
#define ST_SYNTH_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Serialize synthesis/configuration calls. Input is UTF-8, output is WAV.
   Free a successful result exactly once using its original output length. */
uint8_t *st_synthesize_wav(const uint8_t *text, size_t text_len, bool french,
                         uint32_t rate, uint32_t pitch, size_t *out_len);
void st_free_wav(uint8_t *ptr, size_t len);
#ifdef __cplusplus
}
#endif
#endif
