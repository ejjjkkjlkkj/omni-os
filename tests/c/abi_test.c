/* Real C consumer of the ST v1 ABI, built with a C compiler against the shipped DLL.
 *
 *   gcc -O2 -I include tests/c/abi_test.c -L <dir with st_synth.dll> -lst_synth -o abi_test.exe
 *   abi_test.exe [--neural]
 *
 * Exit code 0 only if every check passes. Prints one PASS/FAIL line per check.
 */
#include <stdio.h>
#include <string.h>
#include <windows.h>
#include "st_synth.h"

static int failures = 0;
#define CHECK(cond, name) do { if (cond) printf("[PASS] %s\n", name); \
    else { char e[512]; st_last_error_v1(e, sizeof e); printf("[FAIL] %s (last error: %s)\n", name, e); failures++; } } while (0)

typedef struct { size_t samples, calls; float peak; uint32_t rate; int slow; } Acc;

static uint8_t collect(const float *pcm, size_t n, uint32_t rate, void *user) {
    Acc *a = (Acc *)user;
    a->rate = rate; a->calls++; a->samples += n;
    for (size_t i = 0; i < n; i++) { float v = pcm[i] < 0 ? -pcm[i] : pcm[i]; if (v > a->peak) a->peak = v; }
    if (a->slow) Sleep(5);
    return 1;
}
static uint8_t stop_now(const float *pcm, size_t n, uint32_t rate, void *user) {
    (void)pcm; (void)n; (void)rate; ((Acc *)user)->calls++; return 0;
}

static StEngine *create(const char *json) {
    StEngine *e = NULL;
    return st_engine_create_v1((const uint8_t *)json, strlen(json), &e) == 0 ? e : NULL;
}
static int32_t stream(StEngine *e, const char *text, st_audio_callback_v1 cb, Acc *a) {
    return st_engine_stream_v1(e, (const uint8_t *)text, strlen(text), cb, a);
}

typedef struct { StEngine *e; int delay_ms; } CancelArgs;
static DWORD WINAPI canceller(LPVOID p) {
    CancelArgs *c = (CancelArgs *)p; Sleep(c->delay_ms); st_engine_cancel_v1(c->e); return 0;
}
typedef struct { const char *json, *text; int32_t code; size_t samples; } Job;
static DWORD WINAPI worker(LPVOID p) {
    Job *j = (Job *)p; StEngine *e = create(j->json); Acc a = {0};
    j->code = e ? stream(e, j->text, collect, &a) : -1; j->samples = a.samples;
    st_engine_destroy_v1(e); return 0;
}

int main(int argc, char **argv) {
    int neural = argc > 1 && strcmp(argv[1], "--neural") == 0;
    const char *cfg = neural ? "{\"backend\":\"neural\",\"lang\":\"fr\"}" : "{\"backend\":\"compact\",\"lang\":\"fr\",\"voice\":\"female\"}";
    printf("backend: %s\n", neural ? "neural" : "compact");

    StEngine *bad = NULL;
    CHECK(st_engine_create_v1((const uint8_t *)"{", 1, &bad) == 1 && bad == NULL, "invalid JSON rejected");
    CHECK(st_last_error_v1(NULL, 0) > 0, "last error set");
    CHECK(st_engine_create_v1((const uint8_t *)"{\"x\":1}", 7, &bad) == 1, "unknown key rejected");

    StEngine *e = create(cfg);
    CHECK(e != NULL, "engine created");
    if (!e) return 1;

    Acc a = {0};
    CHECK(stream(e, "Bonjour, bienvenue dans ST.", collect, &a) == 0, "stream ok");
    CHECK(a.rate == 48000 && a.samples > 24000 && a.peak > 0.05f && a.peak < 1.0f, "48 kHz, audible, not clipped");

    Acc c = {0};
    CHECK(stream(e, "Premiere phrase. Deuxieme phrase. Troisieme.", stop_now, &c) == 4 && c.calls == 1, "callback cancel -> 4");

    Acc s = {0}; s.slow = 1;
    CancelArgs ca = { e, 30 };
    HANDLE t = CreateThread(NULL, 0, canceller, &ca, 0, NULL);
    int32_t code = stream(e, "Une. Deux. Trois. Quatre. Cinq. Six. Sept. Huit. Neuf. Dix.", collect, &s);
    WaitForSingleObject(t, INFINITE); CloseHandle(t);
    CHECK(code == 4, "st_engine_cancel_v1 from another thread -> 4");

    Acc after = {0};
    DWORD t0 = GetTickCount();
    CHECK(stream(e, "Fermer, bouton.", collect, &after) == 0 && after.samples > 0, "speech resumes after cancel");
    printf("       resume latency %lu ms\n", GetTickCount() - t0);

    uint8_t *wav = NULL; size_t wav_len = 0;
    CHECK(st_engine_wav_v1(e, (const uint8_t *)"OK.", 3, &wav, &wav_len) == 0 && wav_len > 44 && memcmp(wav, "RIFF", 4) == 0, "wav ok");
    st_free_wav(wav, wav_len);

    const uint8_t invalid_utf8[] = { 0xff, 0xfe };
    CHECK(st_engine_stream_v1(e, invalid_utf8, 2, collect, &a) == 1, "invalid UTF-8 -> 1");
    CHECK(st_engine_stream_v1(e, (const uint8_t *)"x", 0, collect, &a) == 1, "empty text -> 1");
    CHECK(st_engine_stream_v1(e, (const uint8_t *)"ok", 2, NULL, NULL) == 1, "NULL callback -> 1");
    st_engine_destroy_v1(e);
    st_engine_destroy_v1(NULL);
    CHECK(1, "destroy (and destroy NULL)");

    /* Two independent compact engines on two threads. */
    Job jobs[2] = { { "{\"lang\":\"fr\"}", "Thread un.", -1, 0 }, { "{\"lang\":\"en\"}", "Thread two.", -1, 0 } };
    HANDLE th[2];
    for (int i = 0; i < 2; i++) th[i] = CreateThread(NULL, 0, worker, &jobs[i], 0, NULL);
    WaitForMultipleObjects(2, th, TRUE, INFINITE);
    for (int i = 0; i < 2; i++) CloseHandle(th[i]);
    CHECK(jobs[0].code == 0 && jobs[1].code == 0 && jobs[0].samples && jobs[1].samples, "two engines on two threads");

    printf("%s: %d failure(s)\n", failures ? "FAIL" : "PASS", failures);
    return failures ? 1 : 0;
}
