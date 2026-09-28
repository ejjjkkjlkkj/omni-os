#include "semantic_core.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

static void test_toggle_focus(void) {
    qev_semantic_node node = {
        QEV_ROLE_TOGGLE, 0, "Secure Boot", "enabled",
        QEV_STATE_FOCUSED | QEV_STATE_CHECKED
    };
    qev_utterance out;
    assert(qev_semantic_focus_utterance(&node, &out));
    assert(strcmp(out.text, "toggle checked Secure Boot enabled") == 0);
    assert(out.priority == QEV_SPEECH_FOCUS);
    assert(out.interrupt == 1u);
    assert(out.truncated == 0u);
}

static void test_button_without_value_is_spoken(void) {
    qev_semantic_node node = {
        QEV_ROLE_BUTTON, 0, "Load defaults", 0,
        QEV_STATE_FOCUSED
    };
    qev_utterance out;
    assert(qev_semantic_focus_utterance(&node, &out));
    assert(strcmp(out.text, "button Load defaults") == 0);
    assert(out.priority == QEV_SPEECH_FOCUS);
    assert(out.interrupt == 1u);
}

static void test_password_is_redacted(void) {
    qev_semantic_node node = {
        QEV_ROLE_PASSWORD_FIELD, 0, "Administrator password", "secret-value",
        QEV_STATE_PROTECTED
    };
    qev_utterance out;
    assert(qev_semantic_focus_utterance(&node, &out));
    assert(strstr(out.text, "protected") != 0);
    assert(strstr(out.text, "secret-value") == 0);
}

static void test_firmware_states(void) {
    qev_semantic_node node = {
        QEV_ROLE_NUMERIC_SETTING, 0, "CPU limit", "45",
        QEV_STATE_PREVIEW | QEV_STATE_READ_ONLY | QEV_STATE_RESET_REQUIRED
    };
    qev_utterance out;
    assert(qev_semantic_focus_utterance(&node, &out));
    assert(strcmp(out.text,
                  "numeric setting read only preview reset required CPU limit 45") == 0);
}

static void test_unknown_native_role_fallback(void) {
    qev_semantic_node node = {
        QEV_ROLE_UNKNOWN, "vendor control", "Vendor option", "active", 0u
    };
    qev_utterance out;
    assert(qev_semantic_focus_utterance(&node, &out));
    assert(strcmp(out.text, "vendor control Vendor option active") == 0);
}

static void test_event_policy(void) {
    assert(qev_semantic_event_priority(QEV_EVENT_ERROR) == QEV_SPEECH_URGENT);
    assert(qev_semantic_event_interrupt(QEV_EVENT_ERROR) == 1u);
    assert(qev_semantic_event_priority(QEV_EVENT_FOCUS_CHANGED) == QEV_SPEECH_FOCUS);
    assert(qev_semantic_event_interrupt(QEV_EVENT_FOCUS_CHANGED) == 1u);
    assert(qev_semantic_event_priority(QEV_EVENT_PROGRESS) == QEV_SPEECH_BACKGROUND);
    assert(qev_semantic_event_interrupt(QEV_EVENT_PROGRESS) == 0u);
}

static void test_empty_node_is_silent(void) {
    qev_semantic_node node = {QEV_ROLE_UNKNOWN, 0, 0, 0, 0u};
    qev_utterance out;
    assert(!qev_semantic_focus_utterance(&node, &out));
    assert(out.text[0] == 0);
}


static void assert_firmware_alphabet(const char *text) {
    assert(text != 0);
    for (; *text; ++text) {
        unsigned char ch = (unsigned char)*text;
        assert((ch >= (unsigned char)'a' && ch <= (unsigned char)'z') ||
               (ch >= (unsigned char)'A' && ch <= (unsigned char)'Z') ||
               (ch >= (unsigned char)'0' && ch <= (unsigned char)'9') ||
               ch == (unsigned char)' ');
    }
}

static void test_firmware_alphabet_only(void) {
    qev_semantic_node node = {
        QEV_ROLE_PASSWORD_FIELD, 0, "Admin Password", "never-spoken",
        QEV_STATE_PROTECTED | QEV_STATE_READ_ONLY | QEV_STATE_RESET_REQUIRED
    };
    qev_utterance out;
    assert(qev_semantic_focus_utterance(&node, &out));
    assert_firmware_alphabet(out.text);
    assert(strstr(out.text, "never-spoken") == 0);
}

static void test_truncation_is_nul_safe(void) {
    static const char long_label[] =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    qev_semantic_node node = {QEV_ROLE_SETTING, 0, long_label, "value", 0u};
    qev_utterance out;
    assert(qev_semantic_focus_utterance(&node, &out));
    assert(out.truncated == 1u);
    assert(out.text[QEV_UTTERANCE_CAP - 1u] == 0);
    assert(strlen(out.text) == QEV_UTTERANCE_CAP - 1u);
}

int main(void) {
    test_toggle_focus();
    test_button_without_value_is_spoken();
    test_password_is_redacted();
    test_firmware_states();
    test_unknown_native_role_fallback();
    test_event_policy();
    test_empty_node_is_silent();
    test_firmware_alphabet_only();
    test_truncation_is_nul_safe();
    puts("UEFI_SEMANTIC_CORE_TESTS=PASS");
    return 0;
}
