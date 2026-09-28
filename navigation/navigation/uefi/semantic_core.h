#ifndef QEV_SEMANTIC_CORE_H
#define QEV_SEMANTIC_CORE_H

#define QEV_UTTERANCE_CAP 128u

typedef enum {
    QEV_ROLE_UNKNOWN = 0,
    QEV_ROLE_FIRMWARE_SCREEN,
    QEV_ROLE_FIRMWARE_MENU,
    QEV_ROLE_SETTING,
    QEV_ROLE_CHOICE,
    QEV_ROLE_TOGGLE,
    QEV_ROLE_BOOT_ENTRY,
    QEV_ROLE_BOOT_DEVICE,
    QEV_ROLE_NUMERIC_SETTING,
    QEV_ROLE_TEXT_SETTING,
    QEV_ROLE_PASSWORD_FIELD,
    QEV_ROLE_CONFIRMATION,
    QEV_ROLE_STATUS,
    QEV_ROLE_PROGRESS,
    QEV_ROLE_BUTTON
} qev_semantic_role;

typedef enum {
    QEV_STATE_NONE = 0u,
    QEV_STATE_FOCUSED = 1u << 0,
    QEV_STATE_DISABLED = 1u << 1,
    QEV_STATE_SELECTED = 1u << 2,
    QEV_STATE_CHECKED = 1u << 3,
    QEV_STATE_MIXED = 1u << 4,
    QEV_STATE_EXPANDED = 1u << 5,
    QEV_STATE_COLLAPSED = 1u << 6,
    QEV_STATE_READ_ONLY = 1u << 7,
    QEV_STATE_INVALID = 1u << 8,
    QEV_STATE_BUSY = 1u << 9,
    QEV_STATE_PREVIEW = 1u << 10,
    QEV_STATE_PROTECTED = 1u << 11,
    QEV_STATE_RESET_REQUIRED = 1u << 12,
    QEV_STATE_RECONNECT_REQUIRED = 1u << 13,
    QEV_STATE_CALLBACK = 1u << 14,
    QEV_STATE_REQUIRED = 1u << 15
} qev_semantic_state;

typedef enum {
    QEV_SPEECH_BACKGROUND = 0,
    QEV_SPEECH_NORMAL,
    QEV_SPEECH_FOCUS,
    QEV_SPEECH_URGENT
} qev_speech_priority;

typedef enum {
    QEV_EVENT_BACKGROUND = 0,
    QEV_EVENT_STATUS,
    QEV_EVENT_PROGRESS,
    QEV_EVENT_VALUE_CHANGED,
    QEV_EVENT_FOCUS_CHANGED,
    QEV_EVENT_DIALOG_OPENED,
    QEV_EVENT_WARNING,
    QEV_EVENT_ERROR
} qev_semantic_event;

typedef struct {
    qev_semantic_role role;
    const char *native_role;
    const char *label;
    const char *value;
    unsigned int states;
} qev_semantic_node;

typedef struct {
    char text[QEV_UTTERANCE_CAP];
    qev_speech_priority priority;
    unsigned char interrupt;
    unsigned char truncated;
} qev_utterance;

const char *qev_semantic_role_name(qev_semantic_role role);
qev_speech_priority qev_semantic_event_priority(qev_semantic_event event);
unsigned char qev_semantic_event_interrupt(qev_semantic_event event);
int qev_semantic_focus_utterance(const qev_semantic_node *node,
                                 qev_utterance *utterance);

#endif
