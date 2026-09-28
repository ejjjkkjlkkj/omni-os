#ifndef OMNI_VOICE_FRONTEND_H
#define OMNI_VOICE_FRONTEND_H

#include <stdint.h>

#define OMNI_VOICE_FRONTEND_STREAM_VERSION UINT32_C(1)
#define OMNI_VOICE_MAX_TEXT_CHARS UINT32_C(4096)
#define OMNI_VOICE_MAX_STREAM_BYTES UINT32_C(32768)
#define OMNI_VOICE_PCM_SAMPLE_RATE UINT32_C(48000)
#define OMNI_VOICE_PCM_MAX_CHANNELS UINT32_C(2)

typedef enum {
    OMNI_PH_AA = 1,
    OMNI_PH_AE = 2,
    OMNI_PH_AH = 3,
    OMNI_PH_AO = 4,
    OMNI_PH_EH = 5,
    OMNI_PH_ER = 6,
    OMNI_PH_IH = 7,
    OMNI_PH_IY = 8,
    OMNI_PH_UH = 9,
    OMNI_PH_UW = 10,
    OMNI_PH_EY = 11,
    OMNI_PH_AY = 12,
    OMNI_PH_OY = 13,
    OMNI_PH_AW = 14,
    OMNI_PH_OW = 15,
    OMNI_PH_M = 16,
    OMNI_PH_N = 17,
    OMNI_PH_NG = 18,
    OMNI_PH_L = 19,
    OMNI_PH_R = 20,
    OMNI_PH_W = 21,
    OMNI_PH_Y = 22,
    OMNI_PH_F = 23,
    OMNI_PH_V = 24,
    OMNI_PH_S = 25,
    OMNI_PH_Z = 26,
    OMNI_PH_SH = 27,
    OMNI_PH_ZH = 28,
    OMNI_PH_TH = 29,
    OMNI_PH_DH = 30,
    OMNI_PH_HH = 31,
    OMNI_PH_CH = 32,
    OMNI_PH_JH = 33,
    OMNI_PH_P = 34,
    OMNI_PH_B = 35,
    OMNI_PH_T = 36,
    OMNI_PH_D = 37,
    OMNI_PH_K = 38,
    OMNI_PH_G = 39,
    OMNI_PH_FR_A = 40,
    OMNI_PH_FR_E_CLOSE = 41,
    OMNI_PH_FR_O_CLOSE = 42,
    OMNI_PH_FR_Y = 43,
    OMNI_PH_EU = 44,
    OMNI_PH_EU_OPEN = 45,
    OMNI_PH_SCHWA = 46,
    OMNI_PH_NAN = 47,
    OMNI_PH_NON = 48,
    OMNI_PH_NIN = 49,
    OMNI_PH_NUN = 50,
    OMNI_PH_NY = 51,
    OMNI_PH_WORD_BOUNDARY = 52,
    OMNI_PH_CLAUSE_END = 53,
    OMNI_PH_PAUSE = 54
} omni_voice_phoneme_id;

#endif
