#include "semantic_core.h"

static int qev_text_nonempty(const char *text) {
    return text && *text;
}

static unsigned int qev_append_raw(char *out, unsigned int n,
                                   unsigned int cap, const char *text,
                                   unsigned char *truncated) {
    if (!out || !cap || !text) return n;
    while (*text) {
        if (n + 1u >= cap) {
            if (truncated) *truncated = 1u;
            break;
        }
        out[n++] = *text++;
    }
    out[n] = 0;
    return n;
}

static unsigned int qev_append_token(char *out, unsigned int n,
                                     unsigned int cap, const char *text,
                                     unsigned char *truncated) {
    if (!qev_text_nonempty(text)) return n;
    if (n) {
        n = qev_append_raw(out, n, cap, " ", truncated);
    }
    return qev_append_raw(out, n, cap, text, truncated);
}

const char *qev_semantic_role_name(qev_semantic_role role) {
    switch (role) {
        case QEV_ROLE_FIRMWARE_SCREEN: return "firmware screen";
        case QEV_ROLE_FIRMWARE_MENU: return "firmware menu";
        case QEV_ROLE_SETTING: return "setting";
        case QEV_ROLE_CHOICE: return "choice";
        case QEV_ROLE_TOGGLE: return "toggle";
        case QEV_ROLE_BOOT_ENTRY: return "boot entry";
        case QEV_ROLE_BOOT_DEVICE: return "boot device";
        case QEV_ROLE_NUMERIC_SETTING: return "numeric setting";
        case QEV_ROLE_TEXT_SETTING: return "text setting";
        case QEV_ROLE_PASSWORD_FIELD: return "password field";
        case QEV_ROLE_CONFIRMATION: return "confirmation";
        case QEV_ROLE_STATUS: return "status";
        case QEV_ROLE_PROGRESS: return "progress";
        case QEV_ROLE_BUTTON: return "button";
        case QEV_ROLE_UNKNOWN:
        default: return "";
    }
}

qev_speech_priority qev_semantic_event_priority(qev_semantic_event event) {
    switch (event) {
        case QEV_EVENT_ERROR:
        case QEV_EVENT_WARNING:
        case QEV_EVENT_DIALOG_OPENED:
            return QEV_SPEECH_URGENT;
        case QEV_EVENT_FOCUS_CHANGED:
            return QEV_SPEECH_FOCUS;
        case QEV_EVENT_VALUE_CHANGED:
        case QEV_EVENT_STATUS:
            return QEV_SPEECH_NORMAL;
        case QEV_EVENT_PROGRESS:
        case QEV_EVENT_BACKGROUND:
        default:
            return QEV_SPEECH_BACKGROUND;
    }
}

unsigned char qev_semantic_event_interrupt(qev_semantic_event event) {
    switch (event) {
        case QEV_EVENT_ERROR:
        case QEV_EVENT_WARNING:
        case QEV_EVENT_DIALOG_OPENED:
        case QEV_EVENT_FOCUS_CHANGED:
        case QEV_EVENT_VALUE_CHANGED:
            return 1u;
        case QEV_EVENT_STATUS:
        case QEV_EVENT_PROGRESS:
        case QEV_EVENT_BACKGROUND:
        default:
            return 0u;
    }
}

int qev_semantic_focus_utterance(const qev_semantic_node *node,
                                 qev_utterance *utterance) {
    if (!node || !utterance) return 0;

    utterance->text[0] = 0;
    utterance->priority = QEV_SPEECH_FOCUS;
    utterance->interrupt = 1u;
    utterance->truncated = 0u;

    unsigned int n = 0u;
    const char *role = qev_semantic_role_name(node->role);
    if (!qev_text_nonempty(role) && qev_text_nonempty(node->native_role))
        role = node->native_role;
    n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                         role, &utterance->truncated);

    if (node->states & QEV_STATE_DISABLED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "disabled", &utterance->truncated);
    if (node->states & QEV_STATE_READ_ONLY)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "read only", &utterance->truncated);
    if (node->states & QEV_STATE_PREVIEW)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "preview", &utterance->truncated);
    if (node->states & QEV_STATE_SELECTED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "selected", &utterance->truncated);
    if (node->states & QEV_STATE_CHECKED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "checked", &utterance->truncated);
    else if (node->states & QEV_STATE_MIXED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "partially checked", &utterance->truncated);
    if (node->states & QEV_STATE_EXPANDED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "expanded", &utterance->truncated);
    else if (node->states & QEV_STATE_COLLAPSED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "collapsed", &utterance->truncated);
    if (node->states & QEV_STATE_REQUIRED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "required", &utterance->truncated);
    if (node->states & QEV_STATE_INVALID)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "invalid", &utterance->truncated);
    if (node->states & QEV_STATE_BUSY)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "busy", &utterance->truncated);
    if (node->states & QEV_STATE_CALLBACK)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "callback", &utterance->truncated);
    if (node->states & QEV_STATE_RESET_REQUIRED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "reset required", &utterance->truncated);
    if (node->states & QEV_STATE_RECONNECT_REQUIRED)
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "reconnect required", &utterance->truncated);

    n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                         node->label, &utterance->truncated);

    if (node->role == QEV_ROLE_PASSWORD_FIELD ||
        (node->states & QEV_STATE_PROTECTED)) {
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             "protected", &utterance->truncated);
    } else {
        n = qev_append_token(utterance->text, n, QEV_UTTERANCE_CAP,
                             node->value, &utterance->truncated);
    }

    return n != 0u;
}
