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

/* v1 instance API: UTF-8 JSON config, e.g.
   {"backend":"neural","lang":"fr","voice":"ff_siwis"}.
   Legacy API above retains PCM16/32 kHz. v1 WAV is PCM24/48 kHz mono.
   Codes: 0 OK, 1 invalid input, 2 backend/audio error, 3 busy/panic, 4 cancelled.
   Each live handle must be destroyed exactly once; no concurrent destruction.
   Callback samples are borrowed until return; return 0 to cancel.
   No callback re-entry on the same handle. Creation loads the neural model once.
   All pointer/length pairs must reference valid buffers for the entire call. */
typedef struct StEngine StEngine;
typedef uint8_t (*st_audio_callback_v1)(const float *, size_t, uint32_t, void *);
int32_t st_engine_create_v1(const uint8_t *json, size_t len, StEngine **out);
int32_t st_engine_stream_v1(StEngine *, const uint8_t *, size_t, st_audio_callback_v1, void *);
int32_t st_engine_wav_v1(StEngine *, const uint8_t *, size_t, uint8_t **out, size_t *out_len);
void st_engine_destroy_v1(StEngine *);
#ifdef __cplusplus
}
#endif
#endif
