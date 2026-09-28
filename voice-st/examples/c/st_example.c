/* Minimal screen-reader style integration of the ST v1 ABI.
 *
 * Build (MSVC):  cl /I ..\..\include st_example.c ..\..\st_synth.dll.lib
 * Run:           st_example.exe "Bonjour, bienvenue dans ST."
 *
 * One engine per voice, created once at startup (the neural model loads here),
 * then reused for every utterance. Audio arrives as 48 kHz mono float chunks;
 * return 0 from the callback to stop speech immediately (e.g. on a key press).
 */
#include <stdio.h>
#include <string.h>
#include "st_synth.h"

typedef struct { size_t samples; } Progress;

static uint8_t on_audio(const float *pcm, size_t n, uint32_t rate, void *user) {
    Progress *p = (Progress *)user;
    (void)pcm; /* hand the chunk to the audio device here */
    p->samples += n;
    printf("chunk: %zu samples @ %u Hz\n", n, rate);
    return 1; /* keep going */
}

static void report(const char *what, int32_t code) {
    char message[512];
    st_last_error_v1(message, sizeof message);
    fprintf(stderr, "%s failed (%d): %s\n", what, code, message);
}

int main(int argc, char **argv) {
    const char *text = argc > 1 ? argv[1] : "Bonjour, bienvenue dans ST.";
    const char *config = "{\"backend\":\"neural\",\"lang\":\"fr\",\"voice\":\"ff_siwis\"}";
    StEngine *engine = NULL;
    int32_t code = st_engine_create_v1((const uint8_t *)config, strlen(config), &engine);
    if (code != 0) {
        report("create (neural)", code);
        /* The compact engine has no external runtime and is always available. */
        config = "{\"backend\":\"compact\",\"lang\":\"fr\",\"voice\":\"female\"}";
        code = st_engine_create_v1((const uint8_t *)config, strlen(config), &engine);
        if (code != 0) { report("create (compact)", code); return 1; }
    }
    Progress progress = {0};
    code = st_engine_stream_v1(engine, (const uint8_t *)text, strlen(text), on_audio, &progress);
    if (code != 0) report("stream", code);
    else printf("%.2f s of audio\n", progress.samples / 48000.0);

    uint8_t *wav = NULL;
    size_t wav_len = 0;
    if (st_engine_wav_v1(engine, (const uint8_t *)text, strlen(text), &wav, &wav_len) == 0) {
        FILE *f = fopen("st_example.wav", "wb");
        if (f) { fwrite(wav, 1, wav_len, f); fclose(f); }
        st_free_wav(wav, wav_len);
    }
    st_engine_destroy_v1(engine);
    return code == 0 ? 0 : 1;
}
