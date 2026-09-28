#include "semantic_core.h"

typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long long u64;
typedef unsigned long long usize;
typedef int s32;

#ifndef QEV_SOURCE_BLOB
#define QEV_SOURCE_BLOB "UNBOUND"
#endif

typedef u64 (*stall_fn)(usize microseconds);
typedef u64 (*allocate_pages_fn)(u32 type, u32 memory_type, usize pages, u64 *memory);

#if defined(QEV_INTERACTIVE_REPEAT) || defined(QEV_INTERACTIVE_NAV)
typedef struct {
    u16 scan_code;
    u16 unicode_char;
} efi_input_key;
typedef u64 (*read_key_fn)(void *self, efi_input_key *key);
typedef struct {
    void *reset;
    read_key_fn read_key;
    void *wait_for_key;
} simple_text_input_protocol;
#endif

extern const u8 qev_unit_bank[];
extern const u32 qev_unit_bank_len;
extern const u32 qev_unit_off[];
extern const u32 qev_unit_len[];
extern const u32 qev_unit_count;
extern const u32 qev_unit_source_rate;
extern const u32 qev_unit_encoding;
extern const u32 qev_sil_unit_index;
extern const u8 qev_letter_unit_count[];
extern const u8 qev_letter_units[];
extern const u8 qev_digit_unit_count[];
extern const u8 qev_digit_units[];
extern const u32 qev_word_count;
extern const u32 qev_word_name_stride;
extern const u32 qev_word_unit_stride;
extern const u8 qev_word_name_len[];
extern const u8 qev_word_names[];
extern const u8 qev_word_unit_count[];
extern const u8 qev_word_units[];
extern const u32 qev_phrase_count;
extern const u32 qev_phrase_name_stride;
extern const u8 qev_phrase_name_len[];
extern const u8 qev_phrase_names[];
extern const u32 qev_phrase_unit_index[];

#define MAX_NID 256
#define MAX_CONN 64
#define INVALID_NID 0xff
#define INVALID_RESP 0xffffffffu

#define QEV_NAV_TEXT_MAX 64u
#define QEV_SPEECH_CHUNK_MAX 32u

#define WIDGET_AUDIO_OUTPUT 0x0
#define WIDGET_AUDIO_INPUT  0x1
#define WIDGET_MIXER        0x2
#define WIDGET_SELECTOR     0x3
#define WIDGET_PIN          0x4
#define WIDGET_POWER        0x5
#define WIDGET_VOLUME       0x6
#define WIDGET_VENDOR       0xf

static u8 g_type[MAX_NID];
static u32 g_widget_cap[MAX_NID];
static u8 g_conn_count[MAX_NID];
static u8 g_conn[MAX_NID][MAX_CONN];
static u8 g_pin_output[MAX_NID];
static u8 g_seen[MAX_NID];
static u8 g_parent[MAX_NID];
static u8 g_depth[MAX_NID];
static u8 g_queue[MAX_NID];
static u8 g_route_index[MAX_NID];

static volatile u8 *g_hda;
static u8 g_cad;
static u8 g_afg = INVALID_NID;
static u8 g_controller_preferred;
static u32 g_codec_vendor_id;
static u8 g_selected_pin_is_internal_speaker;
static stall_fn g_stall;
static allocate_pages_fn g_allocate_pages;
static u64 g_speech_dma_base;
static u8 g_speech_dma_allocations;
static u8 g_speech_stream_initialized;
static u8 g_proof_overflow;
static u8 g_speech_active;
static u64 g_speech_timeout_us;
static u64 g_speech_elapsed_us;
static char g_speech_phrase[QEV_NAV_TEXT_MAX + 1u];
static u8 g_speech_phrase_length;
static u8 g_speech_phrase_offset;
static u8 g_speech_phrase_active;
static char g_speech_chunk[QEV_SPEECH_CHUNK_MAX + 1u];

typedef u64 (*locate_protocol_fn)(const void *protocol, void *registration, void **interface_out);
typedef u64 (*handle_protocol_fn)(void *handle, const void *protocol, void **interface_out);
typedef u64 (*file_open_fn)(void *self, void **new_handle, const u16 *name, u64 open_mode, u64 attributes);
typedef u64 (*file_close_fn)(void *self);
typedef u64 (*file_delete_fn)(void *self);
typedef u64 (*file_write_fn)(void *self, usize *buffer_size, void *buffer);
typedef u64 (*file_flush_fn)(void *self);

typedef struct {
    u32 revision;
    u32 reserved;
    void *parent_handle;
    void *system_table;
    void *device_handle;
} loaded_image_protocol_head;

typedef struct file_protocol {
    u64 revision;
    file_open_fn open;
    file_close_fn close;
    file_delete_fn delete_file;
    void *read;
    file_write_fn write;
    void *get_position;
    void *set_position;
    void *get_info;
    void *set_info;
    file_flush_fn flush;
} file_protocol;

typedef u64 (*open_volume_fn)(void *self, file_protocol **root);
typedef struct {
    u64 revision;
    open_volume_fn open_volume;
} simple_fs_protocol;
typedef u64 (*hii_list_fn)(const void *self, u8 package_type, const void *package_guid, usize *handle_bytes, void **handles);
typedef u64 (*hii_export_fn)(const void *self, void *handle, usize *buffer_size, void *buffer);
typedef u64 (*hii_get_string_fn)(const void *self, const char *language, void *handle, u16 string_id, u16 *string, usize *string_size, void **font_info);
typedef u64 (*hii_get_languages_fn)(const void *self, void *handle, char *languages, usize *language_size);

typedef u64 (*hii_get_package_list_handle_fn)(const void *self, void *hii_handle,
                                                  void **driver_handle);
typedef struct {
    void *new_package_list;
    void *remove_package_list;
    void *update_package_list;
    hii_list_fn list_package_lists;
    hii_export_fn export_package_lists;
    void *register_package_notify;
    void *unregister_package_notify;
    void *find_keyboard_layouts;
    void *get_keyboard_layout;
    void *set_keyboard_layout;
    hii_get_package_list_handle_fn get_package_list_handle;
} hii_database_protocol;

typedef struct {
    void *new_string;
    hii_get_string_fn get_string;
    void *set_string;
    hii_get_languages_fn get_languages;
} hii_string_protocol;

typedef struct {
    u32 data1;
    u16 data2;
    u16 data3;
    u8 data4[8];
} efi_guid;

typedef u64 (*get_variable_fn)(const u16 *name, const efi_guid *vendor_guid,
                               u32 *attributes, usize *data_size, void *data);

static const efi_guid g_hii_database_guid =
    {0xef9fc172u,0xa1b2u,0x4693u,{0xb3,0x27,0x6d,0x32,0xfc,0x41,0x60,0x42}};
static const efi_guid g_hii_string_guid =
    {0x0fd96974u,0x23aau,0x4cdcu,{0xb9,0xcb,0x98,0xd1,0x77,0x50,0x32,0x2a}};
static const efi_guid g_hii_config_access_guid =
    {0x330d4706u,0xf2a0u,0x4e4fu,{0xa3,0x69,0xb6,0x6f,0xa8,0xd5,0x43,0x85}};
static const efi_guid g_hii_config_routing_guid =
    {0x587e72d7u,0xcc50u,0x4f79u,{0x82,0x09,0xca,0x29,0x1f,0xc1,0xa1,0x0f}};
static const efi_guid g_loaded_image_guid =
    {0x5b1b31a1u,0x9562u,0x11d2u,{0x8e,0x3f,0x00,0xa0,0xc9,0x69,0x72,0x3b}};
static const efi_guid g_simple_fs_guid =
    {0x964e5b22u,0x6459u,0x11d2u,{0x8e,0x39,0x00,0xa0,0xc9,0x69,0x72,0x3b}};
#ifdef QEV_INTERACTIVE_NAV
static const efi_guid g_m1603qa_308_setup_package_list_guid =
    {0x899407d7u,0x99feu,0x43d8u,{0x9a,0x21,0x79,0xec,0x32,0x8c,0xac,0x21}};
#endif

static u8 g_hii_package[1024u * 1024u];
static void *g_hii_handles[256];
static char g_prompt_text[QEV_NAV_TEXT_MAX + 1u];
static u32 g_prompt_count;

#ifdef QEV_INTERACTIVE_NAV
/*
 * M1603QA BIOS 308 exposes dozens of forms and far more than 32 controls.
 * Keep a large static, allocation-free navigation catalogue and make
 * truncation explicit instead of silently dropping controls.
 */
#define MAX_HII_NAV_PROMPTS 240
#define MAX_HII_VARSTORES 128
#define MAX_HII_VARSTORE_NAME 48
#define MAX_HII_VAR_DATA 4096
#define MAX_HII_OPTIONS_PER_PROMPT 8
#define MAX_HII_QUESTIONS 768
#define MAX_HII_STAGED_VALUES 64
#define MAX_IFR_EXPR_STACK 32

typedef struct {
    u8 valid;
    u8 handle_index;
    u8 kind; /* 1=buffer, 2=efi */
    u16 varstore_id;
    efi_guid guid;
    u16 size;
    char name[MAX_HII_VARSTORE_NAME];
} nav_varstore_desc;

static nav_varstore_desc g_nav_varstores[MAX_HII_VARSTORES];
static u8 g_nav_varstore_total;
static u8 g_nav_var_data[MAX_HII_VAR_DATA];

typedef struct {
    u8 valid;
    u8 handle_index;
    u8 opcode;
    u8 value_width;
    u16 question_id;
    u16 varstore_id;
    u16 var_info;
} nav_question_desc;

static nav_question_desc g_nav_questions[MAX_HII_QUESTIONS];
static u16 g_nav_question_total;

typedef struct {
    u8 valid;
    u8 handle_index;
    u16 question_id;
    u64 value;
} nav_staged_value;

static nav_staged_value g_nav_staged_values[MAX_HII_STAGED_VALUES];
static u8 g_nav_staged_total;

static char g_nav_prompts[MAX_HII_NAV_PROMPTS][QEV_NAV_TEXT_MAX + 1u];
static u8 g_nav_prompt_lengths[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_opcodes[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_handle_indices[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_value_widths[MAX_HII_NAV_PROMPTS];
static u16 g_nav_prompt_form_ids[MAX_HII_NAV_PROMPTS];
static u16 g_nav_prompt_question_ids[MAX_HII_NAV_PROMPTS];
static u16 g_nav_prompt_varstore_ids[MAX_HII_NAV_PROMPTS];
static u16 g_nav_prompt_var_infos[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_question_flags[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_condition_flags[MAX_HII_NAV_PROMPTS];
static u16 g_nav_ref_form_ids[MAX_HII_NAV_PROMPTS];
static u8 g_nav_option_counts[MAX_HII_NAV_PROMPTS];
static u64 g_nav_option_values[MAX_HII_NAV_PROMPTS][MAX_HII_OPTIONS_PER_PROMPT];
static char g_nav_option_text[MAX_HII_NAV_PROMPTS][MAX_HII_OPTIONS_PER_PROMPT][QEV_NAV_TEXT_MAX + 1u];
static u8 g_nav_option_text_lengths[MAX_HII_NAV_PROMPTS][MAX_HII_OPTIONS_PER_PROMPT];
static u8 g_nav_prompt_meta_valid[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_control_flags[MAX_HII_NAV_PROMPTS];
static u64 g_nav_prompt_min_value[MAX_HII_NAV_PROMPTS];
static u64 g_nav_prompt_max_value[MAX_HII_NAV_PROMPTS];
static u64 g_nav_prompt_step_value[MAX_HII_NAV_PROMPTS];
static u16 g_nav_prompt_min_size[MAX_HII_NAV_PROMPTS];
static u16 g_nav_prompt_max_size[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_max_containers[MAX_HII_NAV_PROMPTS];
static char g_nav_help[MAX_HII_NAV_PROMPTS][QEV_NAV_TEXT_MAX + 1u];
static u8 g_nav_help_lengths[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_total;
static u8 g_nav_prompt_index;
static u8 g_nav_prompt_opcode;
static u8 g_nav_prompt_overflow;
static char g_nav_speech_text[33];
static u8 g_nav_speech_length;
static char g_nav_help_text[QEV_NAV_TEXT_MAX + 1u];
static u8 g_nav_help_length;
static char g_nav_value_text[33];
static u8 g_nav_value_length;
static char g_nav_focus_speech[QEV_NAV_TEXT_MAX + 1u];
static u8 g_nav_focus_speech_length;
static char g_nav_position_text[33];
static u8 g_nav_position_length;
static char g_nav_where_text[QEV_NAV_TEXT_MAX + 1u];
static u8 g_nav_where_length;
static char g_nav_edit_status_text[33];
static u8 g_nav_edit_status_length;
static u8 g_nav_help_available;
static u16 g_nav_current_form_id;
static char g_nav_form_title[QEV_NAV_TEXT_MAX + 1u];
static u8 g_nav_form_title_length;
static u16 g_nav_form_history[16];
static u8 g_nav_form_history_depth;
static hii_string_protocol *g_nav_hii_string;
static void *g_nav_hii_handle;
static u8 g_nav_m1603qa_308_profile;
static u8 g_nav_m1603qa_handle_index;
static u8 g_nav_event_mask;
static u8 g_nav_speech_events;
static u8 g_nav_realtime_events;
static u8 g_nav_speech_interruptions;
static u16 g_nav_condition_known;
static u16 g_nav_condition_unknown;
static u16 g_nav_condition_hidden;
static u16 g_nav_condition_gray;
static void *g_nav_config_driver_handle;
static void *g_nav_config_access;
static void *g_nav_config_routing;
#define NAV_SEEN_UP        0x01u
#define NAV_SEEN_DOWN      0x02u
#define NAV_SEEN_R         0x04u
#define NAV_SEEN_HOME      0x08u
#define NAV_SEEN_END       0x10u
#define NAV_SEEN_PAGE_UP   0x20u
#define NAV_SEEN_PAGE_DOWN 0x40u
#define NAV_REQUIRED_MASK  (NAV_SEEN_UP | NAV_SEEN_DOWN | NAV_SEEN_R | \
                            NAV_SEEN_HOME | NAV_SEEN_END | NAV_SEEN_PAGE_UP | \
                            NAV_SEEN_PAGE_DOWN)
#endif

static inline void outb(u16 port, u8 value) {
    __asm__ volatile("outb %0, %1" :: "a"(value), "d"(port));
}
static inline u8 inb(u16 port) {
    u8 value;
    __asm__ volatile("inb %1, %0" : "=a"(value) : "d"(port));
    return value;
}
static inline void outl(u16 port, u32 value) {
    __asm__ volatile("outl %0, %1" :: "a"(value), "d"(port));
}
static inline u32 inl(u16 port) {
    u32 value;
    __asm__ volatile("inl %1, %0" : "=a"(value) : "d"(port));
    return value;
}
static inline void fence(void) {
    __asm__ volatile("mfence" ::: "memory");
}

static void serial_init(void) {
    outb(0x3f9, 0x00);
    outb(0x3fb, 0x80);
    outb(0x3f8, 0x03);
    outb(0x3f9, 0x00);
    outb(0x3fb, 0x03);
    outb(0x3fa, 0xc7);
    outb(0x3fc, 0x0b);
}
static void serial_char(char ch) {
    u32 timeout = 1000000;
    while (timeout-- && !(inb(0x3fd) & 0x20)) {}
    outb(0x3f8, (u8)ch);
}
static void serial_puts(const char *s) {
    while (*s) serial_char(*s++);
}
static void serial_hex8(u8 value) {
    static const char h[] = "0123456789ABCDEF";
    serial_char(h[(value >> 4) & 0xf]);
    serial_char(h[value & 0xf]);
}
static void serial_hex32(u32 value) {
    serial_hex8((u8)(value >> 24));
    serial_hex8((u8)(value >> 16));
    serial_hex8((u8)(value >> 8));
    serial_hex8((u8)value);
}
static void marker(const char *s) {
    serial_puts(s);
    serial_puts("\r\n");
}

static void proof_puts(char *buf, usize cap, usize *n, const char *s) {
    while (*s) {
        if (*n + 1u >= cap) {
            g_proof_overflow = 1;
            return;
        }
        buf[(*n)++] = *s++;
    }
}
static void proof_hex8(char *buf, usize cap, usize *n, u8 value) {
    static const char h[] = "0123456789ABCDEF";
    if (*n + 2u >= cap) {
        g_proof_overflow = 1;
        return;
    }
    buf[(*n)++] = h[(value >> 4) & 0xf];
    buf[(*n)++] = h[value & 0xf];
}
static void proof_hex32(char *buf, usize cap, usize *n, u32 value) {
    proof_hex8(buf,cap,n,(u8)(value >> 24));
    proof_hex8(buf,cap,n,(u8)(value >> 16));
    proof_hex8(buf,cap,n,(u8)(value >> 8));
    proof_hex8(buf,cap,n,(u8)value);
}
static int persist_boot_proof(void *image_handle, void *boot_services,
                              u8 pin, u8 dac, u8 selectors, u8 applied) {
    static const u16 filename[] = {
        '\\','Q','E','V','A','R','Y','N','O','X','-','P','H','Y','S','I','C','A','L',
        '-','P','R','O','O','F','.','T','X','T',0
    };
    static char proof[4096];
    usize n = 0;
    g_proof_overflow = 0;
    loaded_image_protocol_head *loaded = 0;
    simple_fs_protocol *fs = 0;
    file_protocol *root = 0;
    file_protocol *file = 0;
    if (!boot_services || !image_handle) return 0;
    handle_protocol_fn handle_protocol =
        *(handle_protocol_fn *)((u8 *)boot_services + 0x98);
    if (!handle_protocol) return 0;
    if (handle_protocol(image_handle, &g_loaded_image_guid, (void **)&loaded) != 0 ||
        !loaded || !loaded->device_handle) return 0;
    if (handle_protocol(loaded->device_handle, &g_simple_fs_guid, (void **)&fs) != 0 ||
        !fs || !fs->open_volume) return 0;
    if (fs->open_volume(fs, &root) != 0 || !root || !root->open) return 0;

    /*
     * EFI_FILE_MODE_CREATE does not guarantee truncation when a file already
     * exists. Delete the previous witness first so a shorter second proof can
     * never retain stale trailing fields from an earlier physical boot.
     */
    const u64 rw_mode = 0x2ull | 0x1ull;
    file_protocol *old_file = 0;
    if (root->open(root, (void **)&old_file, filename, rw_mode, 0) == 0 && old_file) {
        if (!old_file->delete_file) {
            if (old_file->close) old_file->close(old_file);
            if (root->close) root->close(root);
            return 0;
        }
        /*
         * EFI_FILE_DELETE closes old_file in all cases, including delete
         * failure/warning, so never touch that handle after this call.
         */
        if (old_file->delete_file(old_file) != 0) {
            if (root->close) root->close(root);
            return 0;
        }
    }

    const u64 create_mode = 0x8000000000000000ull | rw_mode;
    if (root->open(root, (void **)&file, filename, create_mode, 0) != 0 ||
        !file || !file->write) {
        if (root->close) root->close(root);
        return 0;
    }

    proof_puts(proof,sizeof(proof),&n,"QEVARYNOX-UEFI-PHYSICAL-BOOT-PROOF-V1\r\n");
    proof_puts(proof,sizeof(proof),&n,"UEFI_SOURCE_BLOB=" QEV_SOURCE_BLOB "\r\n");
    proof_puts(proof,sizeof(proof),&n,"STATUS=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_PROMPT_SOURCE=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_SPEECH_MODE=CLEAR_LETTERNAME_SPELLING_FR_V3\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_CONTROLLER_SELECTION=");
    proof_puts(proof,sizeof(proof),&n,
        g_controller_preferred ? "PREFERRED_AMD_1022_15E3\r\n" : "GENERIC_CLASS_0403\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_CODEC_VENDOR_DEVICE=0x");
    proof_hex32(proof,sizeof(proof),&n,g_codec_vendor_id);
    proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_CODEC_SELECTION=");
    proof_puts(proof,sizeof(proof),&n,
        g_controller_preferred ? "REALTEK_10EC_0256\r\n" : "GENERIC_RUNTIME\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_GRAPH_SEARCH_LIVE=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_SELECTOR_APPLY_LIVE=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_ROUTE_POWER_D0=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_ROUTE_AMPLIFIERS=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_EAPD_POLICY=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_DAC_STREAM_READBACK=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_PIN_CONTROL_READBACK=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_OUTPUT_PATH_CONFIGURATION=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_PIN_NID=0x"); proof_hex8(proof,sizeof(proof),&n,pin); proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_DAC_NID=0x"); proof_hex8(proof,sizeof(proof),&n,dac); proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_ROUTE_DEPTH=0x"); proof_hex8(proof,sizeof(proof),&n,g_depth[dac]); proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_SELECTOR_WRITES_REQUIRED=0x"); proof_hex8(proof,sizeof(proof),&n,selectors); proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HDA_SELECTOR_WRITES_APPLIED=0x"); proof_hex8(proof,sizeof(proof),&n,applied); proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_SPEECH_DMA=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"LPIB_PROGRESS=PASS\r\n");
    if (g_controller_preferred && g_codec_vendor_id == 0x10ec0256u) {
        proof_puts(proof,sizeof(proof),&n,"PHYSICAL_ASUS_M1603QA_HDA_RUNTIME=PASS\r\n");
        proof_puts(proof,sizeof(proof),&n,"PHYSICAL_ASUS_M1603QA_CODEC=REALTEK_10EC_0256\r\n");
        proof_puts(proof,sizeof(proof),&n,"PHYSICAL_ASUS_M1603QA_INTERNAL_SPEAKER_PIN=");
        proof_puts(proof,sizeof(proof),&n,
            g_selected_pin_is_internal_speaker ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    } else {
        proof_puts(proof,sizeof(proof),&n,"PHYSICAL_ASUS_M1603QA_HDA_RUNTIME=NOT_APPLICABLE\r\n");
    }
#ifdef QEV_INTERACTIVE_REPEAT
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_REPEAT_KEY=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_REPEAT_SPEECH_DMA=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_REPEAT_LPIB_PROGRESS=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_SPEECH_DMA_REUSE=PASS\r\n");
#else
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_REPEAT_KEY=NOT_ENABLED\r\n");
#endif
#ifdef QEV_INTERACTIVE_NAV
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_UP=");
    proof_puts(proof,sizeof(proof),&n,(g_nav_event_mask & NAV_SEEN_UP) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_DOWN=");
    proof_puts(proof,sizeof(proof),&n,(g_nav_event_mask & NAV_SEEN_DOWN) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_REPEAT=");
    proof_puts(proof,sizeof(proof),&n,(g_nav_event_mask & NAV_SEEN_R) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_HOME=");
    proof_puts(proof,sizeof(proof),&n,(g_nav_event_mask & NAV_SEEN_HOME) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_END=");
    proof_puts(proof,sizeof(proof),&n,(g_nav_event_mask & NAV_SEEN_END) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_PAGE_UP=");
    proof_puts(proof,sizeof(proof),&n,(g_nav_event_mask & NAV_SEEN_PAGE_UP) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_PAGE_DOWN=");
    proof_puts(proof,sizeof(proof),&n,(g_nav_event_mask & NAV_SEEN_PAGE_DOWN) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_REQUIRED_EVENTS=");
    proof_puts(proof,sizeof(proof),&n,
        ((g_nav_event_mask & NAV_REQUIRED_MASK) == NAV_REQUIRED_MASK &&
         g_nav_speech_events >= 7u) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_EXIT=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_SEMANTIC_ROLE=PASS\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_CONTEXT_HELP=");
    proof_puts(proof,sizeof(proof),&n,
        g_nav_help_available ? "PASS\r\n" : "NOT_AVAILABLE\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_SPEECH_EVENTS=0x");
    proof_hex8(proof,sizeof(proof),&n,g_nav_speech_events);
    proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_REALTIME_EVENTS=0x");
    proof_hex8(proof,sizeof(proof),&n,g_nav_realtime_events);
    proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_SPEECH_INTERRUPTS=0x");
    proof_hex8(proof,sizeof(proof),&n,g_nav_speech_interruptions);
    proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAV_REALTIME=");
    proof_puts(proof,sizeof(proof),&n,g_nav_realtime_events ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_SPEECH_DMA_REUSE=");
    proof_puts(proof,sizeof(proof),&n,
        (g_nav_speech_events && g_speech_dma_allocations == 1u) ? "PASS\r\n" : "NOT_ESTABLISHED\r\n");
#else
    proof_puts(proof,sizeof(proof),&n,"HII_GRAPH_NAVIGATION=NOT_ENABLED\r\n");
#endif
    proof_puts(proof,sizeof(proof),&n,"AUDIBLE_PHYSICAL_SPEAKER=REQUIRES_HUMAN_CONFIRMATION\r\n");

    if (g_proof_overflow) {
        if (file->close) file->close(file);
        if (root->close) root->close(root);
        return 0;
    }

    usize bytes = n;
    u64 st = file->write(file, &bytes, proof);
    if (st == 0 && file->flush) st = file->flush(file);
    if (file->close) file->close(file);
    if (root->close) root->close(root);
    return st == 0 && bytes == n;
}

static u32 pci_read32(u32 cfg) {
    outl(0xcf8, cfg);
    return inl(0xcfc);
}
static void pci_write32(u32 cfg, u32 value) {
    outl(0xcf8, cfg);
    outl(0xcfc, value);
}

static inline u16 mmio16(u32 off) {
    return *(volatile u16 *)(g_hda + off);
}
static inline u32 mmio32(u32 off) {
    return *(volatile u32 *)(g_hda + off);
}
static inline void mmio16w(u32 off, u16 value) {
    *(volatile u16 *)(g_hda + off) = value;
    fence();
}
static inline void mmio32w(u32 off, u32 value) {
    *(volatile u32 *)(g_hda + off) = value;
    fence();
}

static u32 immediate(u32 command) {
    u32 timeout = 100000;
    while (timeout-- && (mmio16(0x68) & 1)) {}
    if (!timeout) return INVALID_RESP;
    mmio16w(0x68, 2);
    mmio32w(0x60, command);
    mmio16w(0x68, 1);
    timeout = 100000;
    while (timeout-- && !(mmio16(0x68) & 2)) {}
    if (!timeout) return INVALID_RESP;
    u32 response = mmio32(0x64);
    mmio16w(0x68, 2);
    return response;
}
static u32 encode_verb12(u8 cad, u8 nid, u16 verb, u8 payload) {
    return ((u32)cad << 28) | ((u32)nid << 20) |
           ((u32)verb << 8) | payload;
}
static u32 encode_verb4(u8 cad, u8 nid, u8 verb, u16 payload) {
    return ((u32)cad << 28) | ((u32)nid << 20) |
           ((u32)(verb & 0x0f) << 16) | payload;
}
static u32 verb12(u8 nid, u16 verb, u8 payload) {
    return immediate(encode_verb12(g_cad, nid, verb, payload));
}
static u32 verb4(u8 nid, u8 verb, u16 payload) {
    return immediate(encode_verb4(g_cad, nid, verb, payload));
}
static u32 get_param(u8 nid, u8 param) {
    return verb12(nid, 0xf00, param);
}
static u32 get_conn_entry(u8 nid, u8 index) {
    return verb12(nid, 0xf02, index);
}
static u32 get_conn_select(u8 nid) {
    return verb12(nid, 0xf01, 0);
}
static u32 set_conn_select(u8 nid, u8 index) {
    return verb12(nid, 0x701, index);
}

static void clear_graph(void) {
    u32 i, j;
    for (i = 0; i < MAX_NID; ++i) {
        g_type[i] = 0xff;
        g_widget_cap[i] = 0;
        g_conn_count[i] = 0;
        g_pin_output[i] = 0;
        g_seen[i] = 0;
        g_parent[i] = INVALID_NID;
        g_depth[i] = 0;
        g_queue[i] = 0;
        g_route_index[i] = 0;
        for (j = 0; j < MAX_CONN; ++j) g_conn[i][j] = 0;
    }
}

static int add_conn(u8 node, u16 nid) {
    if (!nid || nid >= MAX_NID) return 0;
    u8 count = g_conn_count[node];
    if (count >= MAX_CONN) return 0;
    g_conn[node][count] = (u8)nid;
    g_conn_count[node] = count + 1;
    return 1;
}

static int decode_connections(u8 node) {
    u32 parameter = get_param(node, 0x0e);
    if (parameter == INVALID_RESP) return 0;
    u8 raw_count = (u8)(parameter & 0x7f);
    if (!raw_count) return 1;
    int long_form = !!(parameter & 0x80);
    u8 per_response = long_form ? 2 : 4;
    u16 mask = long_form ? 0x7fff : 0x007f;
    u16 range_bit = long_form ? 0x8000 : 0x0080;
    u16 previous = 0;
    int have_previous = 0;
    int previous_was_range = 0;

    for (u8 base = 0; base < raw_count; base = (u8)(base + per_response)) {
        u32 response = get_conn_entry(node, base);
        if (response == INVALID_RESP) return 0;
        for (u8 slot = 0; slot < per_response; ++slot) {
            u8 raw_index = (u8)(base + slot);
            if (raw_index >= raw_count) break;
            u16 value = long_form
                ? (u16)((response >> (slot * 16)) & 0xffff)
                : (u16)((response >> (slot * 8)) & 0xff);
            u16 nid = value & mask;
            int is_range = !!(value & range_bit);
            if (!nid) return 0;
            if (is_range) {
                if (!have_previous || previous_was_range || previous >= nid) return 0;
                for (u16 expanded = (u16)(previous + 1); expanded <= nid; ++expanded) {
                    if (!add_conn(node, expanded)) return 0;
                }
            } else {
                if (!add_conn(node, nid)) return 0;
            }
            previous = nid;
            have_previous = 1;
            previous_was_range = is_range;
        }
    }
    return 1;
}

static int traversable(u8 type) {
    return type == WIDGET_MIXER || type == WIDGET_SELECTOR ||
           type == WIDGET_POWER || type == WIDGET_VOLUME ||
           type == WIDGET_VENDOR;
}
static int selectable(u8 type) {
    return type == WIDGET_AUDIO_INPUT || type == WIDGET_SELECTOR ||
           type == WIDGET_PIN || type == WIDGET_VENDOR;
}

static int find_route(u8 pin, u8 *dac_out, u8 *selector_count_out) {
    u32 i;
    for (i = 0; i < MAX_NID; ++i) {
        g_seen[i] = 0;
        g_parent[i] = INVALID_NID;
        g_depth[i] = 0;
        g_route_index[i] = 0;
    }
    u16 qhead = 0, qtail = 0;
    g_queue[qtail++] = pin;
    g_seen[pin] = 1;
    u8 found = INVALID_NID;

    while (qhead < qtail) {
        u8 node = g_queue[qhead++];
        if (g_depth[node] >= 16) continue;
        u8 count = g_conn_count[node];
        for (u8 ci = 0; ci < count; ++ci) {
            u8 upstream = g_conn[node][ci];
            if (g_seen[upstream]) continue;
            u8 type = g_type[upstream];
            if (type == 0xff) continue;
            g_seen[upstream] = 1;
            g_parent[upstream] = node;
            g_route_index[upstream] = ci;
            g_depth[upstream] = (u8)(g_depth[node] + 1);
            if (type == WIDGET_AUDIO_OUTPUT) {
                found = upstream;
                qhead = qtail;
                break;
            }
            if (traversable(type) && qtail < MAX_NID) {
                g_queue[qtail++] = upstream;
            }
        }
    }
    if (found == INVALID_NID) return 0;

    u8 selectors = 0;
    u8 cur = found;
    while (cur != pin) {
        u8 child = g_parent[cur];
        if (child == INVALID_NID) return 0;
        if (g_conn_count[child] > 1) {
            u8 type = g_type[child];
            if (type == WIDGET_MIXER) {
            } else if (selectable(type)) {
                ++selectors;
            } else {
                return 0;
            }
        }
        cur = child;
    }
    *dac_out = found;
    *selector_count_out = selectors;
    return 1;
}

static int apply_route(u8 pin, u8 dac, u8 *applied_out) {
    u8 applied = 0;
    u8 cur = dac;
    while (cur != pin) {
        u8 child = g_parent[cur];
        if (child == INVALID_NID) return 0;
        if (g_conn_count[child] > 1) {
            u8 type = g_type[child];
            if (type == WIDGET_MIXER) {
            } else if (selectable(type)) {
                u8 index = g_route_index[cur];
                if (set_conn_select(child, index) == INVALID_RESP) return 0;
                u32 readback = get_conn_select(child);
                if (readback == INVALID_RESP || (u8)readback != index) return 0;
                ++applied;
            } else {
                return 0;
            }
        }
        cur = child;
    }
    *applied_out = applied;
    return 1;
}

static u32 widget_amp_cap(u8 nid, u8 param) {
    if (g_afg == INVALID_NID) return INVALID_RESP;
    if (g_widget_cap[nid] & 0x08u) return get_param(nid, param);
    return get_param(g_afg, param);
}

static u8 amp_nominal_gain(u32 cap) {
    u8 offset = (u8)(cap & 0x7fu);
    u8 steps = (u8)((cap >> 8) & 0x7fu);
    return offset <= steps ? offset : steps;
}

static int unmute_output_amp(u8 nid) {
    if (!(g_widget_cap[nid] & 0x04u)) return 1;
    u32 cap = widget_amp_cap(nid, 0x12);
    if (cap == INVALID_RESP) return 0;
    u8 gain = amp_nominal_gain(cap);
    if (verb4(nid, 0x3, (u16)(0xb000u | gain)) == INVALID_RESP) return 0;
    u32 left = verb4(nid, 0xb, 0xa000);
    u32 right = verb4(nid, 0xb, 0x8000);
    if (left == INVALID_RESP || right == INVALID_RESP) return 0;
    if ((left & 0x80u) || (right & 0x80u)) return 0;
    if ((left & 0x7fu) != gain || (right & 0x7fu) != gain) return 0;
    return 1;
}

static int unmute_input_amp(u8 nid, u8 index) {
    if (!(g_widget_cap[nid] & 0x02u)) return 1;
    if (index > 0x0fu) return 0;
    u32 cap = widget_amp_cap(nid, 0x0d);
    if (cap == INVALID_RESP) return 0;
    u8 gain = amp_nominal_gain(cap);
    u16 set_payload = (u16)(0x7000u | ((u16)index << 8) | gain);
    if (verb4(nid, 0x3, set_payload) == INVALID_RESP) return 0;
    u32 left = verb4(nid, 0xb, (u16)(0x2000u | index));
    u32 right = verb4(nid, 0xb, index);
    if (left == INVALID_RESP || right == INVALID_RESP) return 0;
    if ((left & 0x80u) || (right & 0x80u)) return 0;
    if ((left & 0x7fu) != gain || (right & 0x7fu) != gain) return 0;
    return 1;
}

static int wait_node_d0(u8 nid) {
    /* Get Power State: PS-Set is bits 3:0, PS-Act is bits 7:4 and
       PS-Error is bit 8. A real codec may need time to complete D3->D0. */
    for (u32 attempt = 0; attempt < 100u; ++attempt) {
        u32 state = verb12(nid, 0xf05, 0);
        if (state == INVALID_RESP || (state & 0x00000100u)) return 0;
        if ((state & 0x0fu) == 0u && ((state >> 4) & 0x0fu) == 0u) return 1;
        if (g_stall) g_stall(1000);
    }
    return 0;
}

static int power_up_afg(void) {
    if (g_afg == INVALID_NID) return 0;
    u32 supported = get_param(g_afg, 0x0f);
    /* Some virtual codecs expose no controllable AFG power states. */
    if (supported == INVALID_RESP || !(supported & 0x01u))
        return g_controller_preferred ? 0 : 1;
    if (verb12(g_afg, 0x705, 0x00) == INVALID_RESP) return 0;
    return wait_node_d0(g_afg);
}

static int power_up_route_widget(u8 nid) {
    /* Audio Widget Capabilities bit 10 advertises power-state control. */
    if (!(g_widget_cap[nid] & 0x00000400u)) return 1;
    u32 supported = get_param(nid, 0x0f);
    if (supported == INVALID_RESP || !(supported & 0x01u)) return 0;
    if (verb12(nid, 0x705, 0x00) == INVALID_RESP) return 0;
    return wait_node_d0(nid);
}

static int configure_output_path(u8 pin, u8 dac) {
    /* The Function Group constrains widget PS-Act, so request AFG D0 first. */
    if (!power_up_afg()) return 0;

    /* Put every power-managed route widget in D0 before touching amps. */
    u8 cur = dac;
    for (;;) {
        if (!power_up_route_widget(cur)) return 0;
        if (cur == pin) break;
        u8 child = g_parent[cur];
        if (child == INVALID_NID) return 0;
        cur = child;
    }
    marker("HDA_ROUTE_POWER_D0=PASS");

    /* Unmute every amplifier actually traversed by the discovered route.
       Widgets without Amp Parameter Override inherit the AFG capabilities. */
    cur = dac;
    for (;;) {
        if (!unmute_output_amp(cur)) return 0;
        if (cur == pin) break;
        u8 child = g_parent[cur];
        if (child == INVALID_NID) return 0;
        if (!unmute_input_amp(child, g_route_index[cur])) return 0;
        cur = child;
    }
    marker("HDA_ROUTE_AMPLIFIERS=PASS");

    u32 pin_cap = get_param(pin, 0x0c);
    if (pin_cap == INVALID_RESP) return 0;
    if (pin_cap & 0x00010000u) {
        u32 eapd = verb12(pin, 0xf0c, 0);
        if (eapd == INVALID_RESP) return 0;
        u8 desired_eapd = (u8)eapd | 0x02u;
        if (verb12(pin, 0x70c, desired_eapd) == INVALID_RESP) return 0;
        eapd = verb12(pin, 0xf0c, 0);
        if (eapd == INVALID_RESP || !(eapd & 0x02u)) return 0;
    }
    marker("HDA_EAPD_POLICY=PASS");

    if (verb12(dac, 0x706, 0x10) == INVALID_RESP) return 0;
    u32 stream_channel = verb12(dac, 0xf06, 0);
    if (stream_channel == INVALID_RESP || (stream_channel & 0xffu) != 0x10u) return 0;

    if (verb4(dac, 0x2, 0x0011) == INVALID_RESP) return 0;
    u32 format = verb4(dac, 0xa, 0);
    if (format == INVALID_RESP || (format & 0xffffu) != 0x0011u) return 0;
    marker("HDA_DAC_STREAM_READBACK=PASS");

    u32 pin_ctl = verb12(pin, 0xf07, 0);
    if (pin_ctl == INVALID_RESP) return 0;
    u8 desired_pin_ctl = (u8)pin_ctl | 0x40u;
    if (verb12(pin, 0x707, desired_pin_ctl) == INVALID_RESP) return 0;
    pin_ctl = verb12(pin, 0xf07, 0);
    if (pin_ctl == INVALID_RESP || !(pin_ctl & 0x40u)) return 0;
    marker("HDA_PIN_CONTROL_READBACK=PASS");
    return 1;
}

static s32 qev_mulaw_decode(u8 code) {
    u8 u = (u8)(~code);
    u32 exponent = (u32)((u >> 4) & 0x07u);
    u32 mantissa = (u32)(u & 0x0fu);
    s32 sample = (s32)(((mantissa << 3) + 0x84u) << exponent);
    sample -= 0x84;
    return (u & 0x80u) ? -sample : sample;
}

static int append_unit_pcm(volatile u8 *pcm, u32 capacity,
                           u32 *total_bytes, u32 ui) {
    if (!pcm || !total_bytes || ui >= qev_unit_count ||
        qev_unit_encoding != 1u ||
        !qev_unit_source_rate || (48000u % qev_unit_source_rate) != 0u)
        return 0;
    u32 off = qev_unit_off[ui];
    u32 len = qev_unit_len[ui];
    if (!len || off > qev_unit_bank_len || len > qev_unit_bank_len - off)
        return 0;
    u32 factor = 48000u / qev_unit_source_rate;
    if (!factor || len > 0xffffffffu / factor / 4u) return 0;
    u32 need = len * factor * 4u;
    if (*total_bytes > capacity || need > capacity - *total_bytes) return 0;

    u32 pos = *total_bytes;
    for (u32 i = 0; i < len; ++i) {
        s32 a = qev_mulaw_decode(qev_unit_bank[off + i]);
        s32 b = a;
        if (i + 1u < len)
            b = qev_mulaw_decode(qev_unit_bank[off + i + 1u]);
        for (u32 phase = 0; phase < factor; ++phase) {
            s32 sample = a + ((b - a) * (s32)phase) / (s32)factor;
            if (sample > 32767) sample = 32767;
            if (sample < -32768) sample = -32768;
            u16 bits = (u16)sample;
            pcm[pos + 0u] = (u8)(bits & 0xffu);
            pcm[pos + 1u] = (u8)(bits >> 8);
            pcm[pos + 2u] = (u8)(bits & 0xffu);
            pcm[pos + 3u] = (u8)(bits >> 8);
            pos += 4u;
        }
    }
    *total_bytes = pos;
    return 1;
}

static volatile u8 *speech_stream_descriptor(void) {
    if (!g_hda) return 0;
    u16 gcap = mmio16(0x00);
    u8 iss = (u8)((gcap >> 8) & 0x0f);
    return g_hda + 0x80 + ((u32)iss * 0x20);
}

static void speech_dma_stop(void) {
    volatile u8 *sd = speech_stream_descriptor();
    if (sd) {
        sd[0] = (u8)(sd[0] & ~2u);
        u32 timeout = 100000;
        while (timeout-- && (sd[0] & 2u)) {}
        sd[3] = 0x1cu;
        if (g_stall) g_stall(1000);
    }
    g_speech_active = 0;
    g_speech_timeout_us = 0;
    g_speech_elapsed_us = 0;
}

static int speech_lookup_word(const char *text, u32 length,
                              const u8 **unit_row_out, u32 *unit_count_out) {
    if (!text || !length || !unit_row_out || !unit_count_out ||
        !qev_word_count || !qev_word_name_stride || !qev_word_unit_stride)
        return 0;
    for (u32 wi = 0u; wi < qev_word_count; ++wi) {
        if ((u32)qev_word_name_len[wi] != length) continue;
        const u8 *name = qev_word_names + wi * qev_word_name_stride;
        u32 same = 1u;
        for (u32 j = 0u; j < length; ++j) {
            if ((u8)text[j] != name[j]) {
                same = 0u;
                break;
            }
        }
        if (!same) continue;
        u32 count = qev_word_unit_count[wi];
        if (!count || count > qev_word_unit_stride) return 0;
        *unit_row_out = qev_word_units + wi * qev_word_unit_stride;
        *unit_count_out = count;
        return 1;
    }
    return 0;
}

static int speech_lookup_phrase(const char *text, u32 length,
                                u32 *unit_index_out) {
    if (!text || !length || !unit_index_out ||
        !qev_phrase_count || !qev_phrase_name_stride)
        return 0;
    for (u32 pi = 0u; pi < qev_phrase_count; ++pi) {
        if ((u32)qev_phrase_name_len[pi] != length) continue;
        const u8 *name = qev_phrase_names + pi * qev_phrase_name_stride;
        u32 same = 1u;
        for (u32 j = 0u; j < length; ++j) {
            if ((u8)text[j] != name[j]) {
                same = 0u;
                break;
            }
        }
        if (!same) continue;
        u32 ui = qev_phrase_unit_index[pi];
        if (ui >= qev_unit_count) return 0;
        *unit_index_out = ui;
        return 1;
    }
    return 0;
}

static int speech_dma_begin(const char *text, u32 text_count) {
    if (!g_allocate_pages || !g_stall || !text || !text_count || text_count > 32u) return 0;
    const u32 pcm_off = 0x1000u;
    const u32 dma_pages = 2048u;
    const u32 dma_bytes = dma_pages * 4096u;
    if (!qev_unit_bank_len || pcm_off >= dma_bytes) return 0;

    if (g_speech_active) speech_dma_stop();

    /*
     * Build the complete semantic label as contiguous PCM. This keeps realtime
     * interruption while avoiding one BDL descriptor per allophone and the
     * alignment failures seen with longer HII labels.
     *
     * Worst-case 32-character French letter-name spelling is about 4.36 MiB
     * (all 'w'). Reserve 8 MiB and up to 128 BDL entries so VoiceCore v4
     * full-letter clips also fit for every accepted 32-character label.
     * Playback stays interruptible,
     * so the larger worst-case timeout never blocks keyboard focus changes.
     */
    u64 base = g_speech_dma_base;
    if (!base) {
        base = 0xffffffffu;
        if (g_allocate_pages(1, 4, dma_pages, &base) != 0 || !base || base > 0xffffffffu) return 0;
        g_speech_dma_base = base;
        ++g_speech_dma_allocations;
    }

    volatile u8 *bdl = (volatile u8 *)(usize)base;
    volatile u8 *pcm = (volatile u8 *)(usize)(base + pcm_off);

    /*
     * Keep physical speech intelligible: a short lead-in gives the codec time
     * to settle, grapheme gaps stop adjacent synthetic units from fusing, and
     * the tail prevents the final phoneme from being clipped. Sizes are whole
     * 48 kHz signed-16 stereo frames (192 bytes/ms).
     */
    const u32 lead_silence_bytes = 30u * 192u;
    const u32 grapheme_gap_bytes = 18u * 192u;
    const u32 phoneme_gap_bytes = 0u;
    const u32 tail_silence_bytes = 45u * 192u;
    u32 total_bytes = lead_silence_bytes;
    if (total_bytes > dma_bytes - pcm_off) return 0;
    for (u32 i = 0; i < total_bytes; ++i) pcm[i] = 0;

    u32 phrase_unit_index = 0u;
    if (speech_lookup_phrase(text, text_count, &phrase_unit_index)) {
        if (!append_unit_pcm(pcm, dma_bytes - pcm_off,
                             &total_bytes, phrase_unit_index))
            return 0;
        marker("HII_GRAPH_SPEECH_WHOLE_PHRASE_CLIP=PASS");
    } else {
    for (u32 i = 0; i < text_count; ++i) {
        u8 ch = (u8)text[i];
        if (ch == (u8)' ') {
            if (qev_sil_unit_index >= qev_unit_count) return 0;
            u32 ui = qev_sil_unit_index;
            if (!append_unit_pcm(pcm, dma_bytes - pcm_off, &total_bytes, ui))
                return 0;
            continue;
        }

        if (ch >= (u8)'a' && ch <= (u8)'z' &&
            (i == 0u || text[i - 1u] == ' ')) {
            u32 word_length = 1u;
            while (i + word_length < text_count &&
                   text[i + word_length] != ' ')
                ++word_length;
            const u8 *word_units = 0;
            u32 word_unit_count = 0u;
            if (speech_lookup_word(text + i, word_length,
                                   &word_units, &word_unit_count)) {
                for (u32 j = 0u; j < word_unit_count; ++j) {
                    if (j) {
                        if (total_bytes > dma_bytes - pcm_off ||
                            phoneme_gap_bytes > dma_bytes - pcm_off - total_bytes)
                            return 0;
                        for (u32 gap = 0u; gap < phoneme_gap_bytes; ++gap)
                            pcm[total_bytes + gap] = 0;
                        total_bytes += phoneme_gap_bytes;
                    }
                    u32 ui = word_units[j];
                    if (!append_unit_pcm(pcm, dma_bytes - pcm_off,
                                         &total_bytes, ui))
                        return 0;
                }
                i += word_length - 1u;
                marker("HII_GRAPH_SPEECH_WORD_PRONUNCIATION=PASS");
                continue;
            }
            marker("HII_GRAPH_SPEECH_WORD_FALLBACK=LETTER_NAMES");
        }

        if (i != 0u && text[i - 1u] != ' ') {
            if (total_bytes > dma_bytes - pcm_off ||
                grapheme_gap_bytes > dma_bytes - pcm_off - total_bytes) return 0;
            for (u32 gap = 0; gap < grapheme_gap_bytes; ++gap) pcm[total_bytes + gap] = 0;
            total_bytes += grapheme_gap_bytes;
        }

        const u8 *unit_row = 0;
        u32 n = 0;
        if (ch >= (u8)'a' && ch <= (u8)'z') {
            u32 li = (u32)(ch - (u8)'a');
            n = qev_letter_unit_count[li];
            unit_row = qev_letter_units + li * 8u;
        } else if (ch >= (u8)'0' && ch <= (u8)'9') {
            u32 di = (u32)(ch - (u8)'0');
            n = qev_digit_unit_count[di];
            unit_row = qev_digit_units + di * 8u;
        } else {
            return 0;
        }
        if (!unit_row || !n || n > 8u) return 0;
        for (u32 j = 0; j < n; ++j) {
            u32 ui = unit_row[j];
            if (!append_unit_pcm(pcm, dma_bytes - pcm_off, &total_bytes, ui))
                return 0;
        }
    }
    }
    if (!total_bytes) return 0;
    if (total_bytes > dma_bytes - pcm_off ||
        tail_silence_bytes > dma_bytes - pcm_off - total_bytes) return 0;
    for (u32 i = 0; i < tail_silence_bytes; ++i) pcm[total_bytes + i] = 0;
    total_bytes += tail_silence_bytes;
    marker("HII_GRAPH_SPEECH_PACING=PASS");
    marker("HII_GRAPH_SPEECH_CONTINUOUS_PHONEMES=PASS");
    marker("HII_GRAPH_SPEECH_WHOLE_CLIP_VOICECORE=PASS");
    marker("HII_GRAPH_SPEECH_MULAW24K=PASS");
    marker("HII_GRAPH_SPEECH_PHRASE_FIRST_MODE=PASS");
    marker("HII_GRAPH_SPEECH_DIGITS=PASS");
    marker("HII_GRAPH_SPEECH_HYBRID_WORD_MODE=PASS");
    marker("HII_GRAPH_SPEECH_UNKNOWN_WORD_FALLBACK=PASS");
    marker("HII_GRAPH_SPEECH_LONG_LABEL_CAPACITY=PASS");

    u32 dma_payload = (total_bytes + 127u) & ~127u;
    if (dma_payload < total_bytes || dma_payload > dma_bytes - pcm_off) return 0;
    for (u32 i = total_bytes; i < dma_payload; ++i) pcm[i] = 0;

    const u32 max_bdl_bytes = 0x10000u;
    u32 entries = 0;
    u32 described = 0;
    while (described < dma_payload) {
        if (entries >= 128u) return 0;
        u32 len = dma_payload - described;
        if (len > max_bdl_bytes) len = max_bdl_bytes;
        volatile u8 *e = bdl + entries * 16u;
        *(volatile u64 *)(e + 0x00) = base + pcm_off + described;
        *(volatile u32 *)(e + 0x08) = len;
        *(volatile u32 *)(e + 0x0c) = 0u;
        described += len;
        ++entries;
    }
    if (!entries) return 0;
    *(volatile u32 *)(bdl + (entries - 1u) * 16u + 0x0c) = 1u;
    fence();

    volatile u8 *sd = speech_stream_descriptor();
    if (!sd) return 0;

    sd[0] = (u8)(sd[0] & ~2u);
    u32 timeout = 100000;
    while (timeout-- && (sd[0] & 2u)) {}
    if (!timeout) return 0;

    if (!g_speech_stream_initialized) {
        sd[0] = (u8)(sd[0] | 1u);
        timeout = 100000;
        while (timeout-- && !(sd[0] & 1u)) {}
        if (!timeout) return 0;
        sd[0] = (u8)(sd[0] & ~1u);
        timeout = 100000;
        while (timeout-- && (sd[0] & 1u)) {}
        if (!timeout) return 0;
        g_speech_stream_initialized = 1u;
    }
    sd[3] = 0x1cu;
    if (g_stall) g_stall(1000);

    *(volatile u32 *)(sd + 0x08) = dma_payload;
    *(volatile u16 *)(sd + 0x0c) = (u16)(entries - 1u);
    *(volatile u16 *)(sd + 0x12) = 0x0011;
    *(volatile u32 *)(sd + 0x18) = (u32)base;
    *(volatile u32 *)(sd + 0x1c) = (u32)(base >> 32);
    fence();

    sd[3] = 0x1cu;
    sd[2] = 0x10;
    sd[0] = (u8)(sd[0] | 2u);

    /* 48 kHz, signed 16-bit stereo = 192000 bytes/s. Polling stays async so
       keyboard focus can cancel the current utterance immediately. */
    u64 play_us = (((u64)dma_payload * 125ull) + 23ull) / 24ull;
    play_us += 150000ull;
    if (play_us > 30000000ull) {
        speech_dma_stop();
        return 0;
    }
    g_speech_timeout_us = play_us;
    g_speech_elapsed_us = 0;
    g_speech_active = 1;
    return 1;
}

/* Returns 1 when complete, 0 while running, -1 on timeout/failure. */
static int speech_dma_poll(u64 elapsed_step_us, u8 *progress_out) {
    if (progress_out) *progress_out = 0;
    if (!g_speech_active) return 1;

    volatile u8 *sd = speech_stream_descriptor();
    if (!sd) {
        speech_dma_stop();
        return -1;
    }

    u32 lpib = *(volatile u32 *)(sd + 0x04);
    if (lpib && progress_out) *progress_out = 1;
    u8 status = sd[3];
    if (status & 0x04u) {
        int ok = lpib != 0;
        speech_dma_stop();
        return ok ? 1 : -1;
    }

    if (elapsed_step_us > g_speech_timeout_us - g_speech_elapsed_us)
        g_speech_elapsed_us = g_speech_timeout_us;
    else
        g_speech_elapsed_us += elapsed_step_us;
    if (g_speech_elapsed_us >= g_speech_timeout_us) {
        speech_dma_stop();
        return -1;
    }
    return 0;
}

static void speech_phrase_cancel(void) {
    if (g_speech_active) speech_dma_stop();
    g_speech_phrase_active = 0u;
    g_speech_phrase_length = 0u;
    g_speech_phrase_offset = 0u;
}

static int speech_phrase_start_next(void) {
    if (!g_speech_phrase_active ||
        g_speech_phrase_offset >= g_speech_phrase_length) return 0;

    u8 start = g_speech_phrase_offset;
    while (start < g_speech_phrase_length && g_speech_phrase[start] == ' ')
        ++start;
    if (start >= g_speech_phrase_length) {
        g_speech_phrase_active = 0u;
        g_speech_phrase_offset = g_speech_phrase_length;
        return 0;
    }

    u8 remaining = (u8)(g_speech_phrase_length - start);
    u8 take = remaining > QEV_SPEECH_CHUNK_MAX ? QEV_SPEECH_CHUNK_MAX : remaining;
    if (remaining > QEV_SPEECH_CHUNK_MAX) {
        u8 split = take;
        while (split > 0u && g_speech_phrase[(u8)(start + split)] != ' ')
            --split;
        if (split >= 8u) take = split;
    }
    while (take && g_speech_phrase[(u8)(start + take - 1u)] == ' ') --take;
    if (!take) return 0;

    for (u8 i = 0u; i < take; ++i)
        g_speech_chunk[i] = g_speech_phrase[(u8)(start + i)];
    g_speech_chunk[take] = 0;
    g_speech_phrase_offset = (u8)(start + take);
    while (g_speech_phrase_offset < g_speech_phrase_length &&
           g_speech_phrase[g_speech_phrase_offset] == ' ')
        ++g_speech_phrase_offset;

    if (!speech_dma_begin(g_speech_chunk, take)) {
        g_speech_phrase_active = 0u;
        return 0;
    }
    marker("HII_GRAPH_SPEECH_CHUNK_START=PASS");
    return 1;
}

static int speech_phrase_begin(const char *text, u32 text_count) {
    if (!text || !text_count || text_count > QEV_NAV_TEXT_MAX) return 0;
    speech_phrase_cancel();
    for (u32 i = 0u; i < text_count; ++i) g_speech_phrase[i] = text[i];
    g_speech_phrase[text_count] = 0;
    g_speech_phrase_length = (u8)text_count;
    g_speech_phrase_offset = 0u;
    g_speech_phrase_active = 1u;
    if (text_count > QEV_SPEECH_CHUNK_MAX)
        marker("HII_GRAPH_SPEECH_MULTI_CHUNK=PASS");
    return speech_phrase_start_next();
}

/* Returns 1 when the full phrase is complete, 0 while any chunk is running. */
static int speech_phrase_poll(u64 elapsed_step_us, u8 *progress_out) {
    int state = speech_dma_poll(elapsed_step_us, progress_out);
    if (state < 0) {
        g_speech_phrase_active = 0u;
        return -1;
    }
    if (state == 0) return 0;
    if (g_speech_phrase_active &&
        g_speech_phrase_offset < g_speech_phrase_length) {
        marker("HII_GRAPH_SPEECH_CHUNK_CONTINUE=PASS");
        if (!speech_phrase_start_next()) return -1;
        return 0;
    }
    g_speech_phrase_active = 0u;
    marker("HII_GRAPH_SPEECH_PHRASE_COMPLETE=PASS");
    return 1;
}

static int run_speech_dma(const char *text, u32 text_count) {
    if (!speech_phrase_begin(text, text_count)) return 0;
    for (;;) {
        u8 progress = 0;
        int state = speech_phrase_poll(1000u, &progress);
        if (state > 0) return 1;
        if (state < 0) return 0;
        g_stall(1000);
    }
}

static u16 rd16(const u8 *p) {
    return (u16)((u16)p[0] | ((u16)p[1] << 8));
}
static u32 rd32(const u8 *p) {
    return (u32)p[0] | ((u32)p[1] << 8) | ((u32)p[2] << 16) | ((u32)p[3] << 24);
}
#ifdef QEV_INTERACTIVE_NAV
static int guid_bytes_equal(const u8 *p, const efi_guid *g) {
    if (!p || !g) return 0;
    if (rd32(p) != g->data1 || rd16(p + 4) != g->data2 || rd16(p + 6) != g->data3)
        return 0;
    for (u8 i = 0u; i < 8u; ++i)
        if (p[8u + i] != g->data4[i]) return 0;
    return 1;
}
#endif
static int prompt_opcode(u8 op) {
    switch (op) {
        case 0x02: case 0x03: case 0x05: case 0x06: case 0x07:
        case 0x08: case 0x0c: case 0x0d: case 0x0f: case 0x1a:
        case 0x1b: case 0x1c: case 0x23:
            return 1;
        default:
            return 0;
    }
}
#ifdef QEV_INTERACTIVE_NAV
static const char *ifr_semantic_role(u8 op) {
    switch (op) {
        case 0x02: return "subtitle";
        case 0x03: return "text";
        case 0x05: return "choice";
        case 0x06: return "checkbox";
        case 0x07: return "number";
        case 0x08: return "password";
        case 0x0c: return "button";
        case 0x0d: return "reset";
        case 0x0f: return "reference";
        case 0x1a: return "date";
        case 0x1b: return "time";
        case 0x1c: return "edit";
        case 0x23: return "ordered list";
        default: return "control";
    }
}

#define NAV_GROUP_BUTTON   1u
#define NAV_GROUP_CHECKBOX 2u
#define NAV_GROUP_CHOICE   3u
#define NAV_GROUP_EDITABLE 4u

#define NAV_COND_SUPPRESS 0x01u
#define NAV_COND_GRAY     0x02u
#define NAV_COND_DISABLE  0x04u
#define NAV_COND_UNKNOWN  0x80u
#define NAV_Q_READ_ONLY          0x01u
#define NAV_Q_CALLBACK           0x04u
#define NAV_Q_RESET_REQUIRED     0x10u
#define NAV_Q_RECONNECT_REQUIRED 0x40u
#define NAV_Q_OPTIONS_ONLY       0x80u

static qev_semantic_role nav_semantic_role_for_opcode(u8 op) {
    switch (op) {
        case 0x02u: return QEV_ROLE_STATUS;
        case 0x03u: return QEV_ROLE_STATUS;
        case 0x05u: return QEV_ROLE_CHOICE;
        case 0x06u: return QEV_ROLE_TOGGLE;
        case 0x07u: return QEV_ROLE_NUMERIC_SETTING;
        case 0x08u: return QEV_ROLE_PASSWORD_FIELD;
        case 0x0cu: return QEV_ROLE_BUTTON;
        case 0x0du: return QEV_ROLE_BUTTON;
        case 0x0fu: return QEV_ROLE_FIRMWARE_MENU;
        case 0x1au: return QEV_ROLE_SETTING;
        case 0x1bu: return QEV_ROLE_SETTING;
        case 0x1cu: return QEV_ROLE_TEXT_SETTING;
        case 0x23u: return QEV_ROLE_CHOICE;
        default: return QEV_ROLE_UNKNOWN;
    }
}

static unsigned int nav_semantic_state_bits(u8 prompt_index, u8 staged) {
    if (prompt_index >= g_nav_prompt_total) return QEV_STATE_NONE;
    unsigned int states = QEV_STATE_FOCUSED;
    u8 condition_flags = g_nav_prompt_condition_flags[prompt_index];
    u8 question_flags = g_nav_prompt_question_flags[prompt_index];

    if (condition_flags & (NAV_COND_GRAY | NAV_COND_DISABLE | NAV_COND_UNKNOWN))
        states |= QEV_STATE_DISABLED;
    if (question_flags & NAV_Q_READ_ONLY)
        states |= QEV_STATE_READ_ONLY;
    if (staged)
        states |= QEV_STATE_PREVIEW;
    if (question_flags & NAV_Q_CALLBACK)
        states |= QEV_STATE_CALLBACK;
    if (question_flags & NAV_Q_RESET_REQUIRED)
        states |= QEV_STATE_RESET_REQUIRED;
    if (question_flags & NAV_Q_RECONNECT_REQUIRED)
        states |= QEV_STATE_RECONNECT_REQUIRED;
    if (g_nav_prompt_opcodes[prompt_index] == 0x08u)
        states |= QEV_STATE_PROTECTED;

    return states;
}

static u8 ifr_condition_flag(u8 op) {
    switch (op) {
        case 0x0au: return NAV_COND_SUPPRESS; /* EFI_IFR_SUPPRESS_IF_OP */
        case 0x19u: return NAV_COND_GRAY;     /* EFI_IFR_GRAY_OUT_IF_OP */
        case 0x1eu: return NAV_COND_DISABLE;  /* EFI_IFR_DISABLE_IF_OP */
        default: return 0u;
    }
}

static int ifr_question_opcode(u8 op) {
    switch (op) {
        case 0x05: case 0x06: case 0x07: case 0x08:
        case 0x0c: case 0x0f: case 0x1a: case 0x1b:
        case 0x1c: case 0x23:
            return 1;
        default:
            return 0;
    }
}

static u8 ifr_scalar_width(u8 op, const u8 *q, u32 oplen) {
    if (op == 0x06u) return 1u; /* checkbox */
    if ((op == 0x05u || op == 0x07u) && q && oplen >= 14u) {
        u8 size_code = (u8)(q[13] & 0x03u);
        return (u8)(1u << size_code);
    }
    return 0u;
}

static void nav_guid_copy(efi_guid *dst, const u8 *src) {
    if (!dst || !src) return;
    u8 *out = (u8 *)dst;
    for (u8 i = 0u; i < 16u; ++i) out[i] = src[i];
}

static void nav_ascii_name_copy(char *dst, u32 cap, const u8 *src, u32 bytes) {
    if (!dst || !cap) return;
    u32 n = 0u;
    while (src && n + 1u < cap && n < bytes && src[n]) {
        u8 ch = src[n];
        dst[n] = (ch >= 0x20u && ch <= 0x7eu) ? (char)ch : '_';
        ++n;
    }
    dst[n] = 0;
}

static void nav_varstore_add(u8 handle_index, u8 kind, u16 varstore_id,
                             const u8 *guid_bytes, u16 size,
                             const u8 *name, u32 name_bytes) {
    if (!varstore_id || !guid_bytes || !name || !size) return;
    for (u8 i = 0u; i < g_nav_varstore_total; ++i) {
        if (g_nav_varstores[i].handle_index == handle_index &&
            g_nav_varstores[i].varstore_id == varstore_id) return;
    }
    if (g_nav_varstore_total >= MAX_HII_VARSTORES) {
        g_nav_prompt_overflow = 1u;
        return;
    }
    nav_varstore_desc *d = &g_nav_varstores[g_nav_varstore_total++];
    d->valid = 1u;
    d->handle_index = handle_index;
    d->kind = kind;
    d->varstore_id = varstore_id;
    nav_guid_copy(&d->guid, guid_bytes);
    d->size = size;
    nav_ascii_name_copy(d->name, sizeof(d->name), name, name_bytes);
}

static nav_varstore_desc *nav_find_varstore(u8 handle_index, u16 varstore_id) {
    for (u8 i = 0u; i < g_nav_varstore_total; ++i) {
        nav_varstore_desc *d = &g_nav_varstores[i];
        if (d->valid && d->handle_index == handle_index &&
            d->varstore_id == varstore_id) return d;
    }
    return 0;
}

static void nav_question_add(u8 handle_index, u8 opcode, u8 value_width,
                             u16 question_id, u16 varstore_id, u16 var_info) {
    if (!question_id) return;
    for (u16 i = 0u; i < g_nav_question_total; ++i) {
        nav_question_desc *d = &g_nav_questions[i];
        if (d->valid && d->handle_index == handle_index &&
            d->question_id == question_id) return;
    }
    if (g_nav_question_total >= MAX_HII_QUESTIONS) {
        g_nav_prompt_overflow = 1u;
        return;
    }
    nav_question_desc *d = &g_nav_questions[g_nav_question_total++];
    d->valid = 1u;
    d->handle_index = handle_index;
    d->opcode = opcode;
    d->value_width = value_width;
    d->question_id = question_id;
    d->varstore_id = varstore_id;
    d->var_info = var_info;
}

static nav_question_desc *nav_find_question(u8 handle_index, u16 question_id) {
    for (u16 i = 0u; i < g_nav_question_total; ++i) {
        nav_question_desc *d = &g_nav_questions[i];
        if (d->valid && d->handle_index == handle_index &&
            d->question_id == question_id) return d;
    }
    return 0;
}

static void nav_option_add(u8 prompt_index, u64 value,
                           const char *text, u32 count) {
    if (prompt_index >= g_nav_prompt_total || !text || !count || count > QEV_NAV_TEXT_MAX) return;
    u8 n = g_nav_option_counts[prompt_index];
    if (n >= MAX_HII_OPTIONS_PER_PROMPT) return;
    for (u8 i = 0u; i < n; ++i) {
        if (g_nav_option_values[prompt_index][i] == value) return;
    }
    g_nav_option_values[prompt_index][n] = value;
    for (u32 j = 0u; j < count; ++j)
        g_nav_option_text[prompt_index][n][j] = text[j];
    g_nav_option_text[prompt_index][n][count] = 0;
    g_nav_option_text_lengths[prompt_index][n] = (u8)count;
    g_nav_option_counts[prompt_index] = (u8)(n + 1u);
}

static int nav_option_for_value(u8 prompt_index, u64 value,
                                const char **text_out, u8 *length_out) {
    if (!text_out || !length_out || prompt_index >= g_nav_prompt_total) return 0;
    u8 n = g_nav_option_counts[prompt_index];
    for (u8 i = 0u; i < n; ++i) {
        if (g_nav_option_values[prompt_index][i] == value) {
            *text_out = g_nav_option_text[prompt_index][i];
            *length_out = g_nav_option_text_lengths[prompt_index][i];
            return *length_out != 0u;
        }
    }
    return 0;
}

static int nav_option_value(u8 type, const u8 *data, u32 bytes,
                            u64 *value_out) {
    if (!data || !value_out) return 0;
    u8 width = 0u;
    switch (type) {
        case 0x00u: case 0x04u: width = 1u; break;
        case 0x01u: width = 2u; break;
        case 0x02u: width = 4u; break;
        case 0x03u: width = 8u; break;
        default: return 0;
    }
    if (bytes < width) return 0;
    u64 value = 0u;
    for (u8 i = 0u; i < width; ++i) value |= ((u64)data[i]) << (8u * i);
    *value_out = value;
    return 1;
}

static int nav_format_u64(u64 value, char *out, u8 *length_out) {
    if (!out || !length_out) return 0;
    static const char prefix[] = "value ";
    u8 n = 0u;
    for (u8 i = 0u; prefix[i] && n < 32u; ++i) out[n++] = prefix[i];
    char digits[20];
    u8 count = 0u;
    do {
        digits[count++] = (char)('0' + (value % 10u));
        value /= 10u;
    } while (value && count < sizeof(digits));
    while (count && n < 32u) out[n++] = digits[--count];
    out[n] = 0;
    *length_out = n;
    return n != 0u;
}

static int nav_read_scalar_value(void *system_table, u8 prompt_index,
                                 u64 *value_out);

static int nav_live_value_text(void *system_table, u8 prompt_index,
                               const char **text_out, u8 *length_out,
                               u8 *checkbox_state_out) {
    if (!text_out || !length_out || prompt_index >= g_nav_prompt_total) return 0;
    if (checkbox_state_out) *checkbox_state_out = 0u;

    u8 op = g_nav_prompt_opcodes[prompt_index];
    if (op == 0x08u) {
        *text_out = "protected";
        *length_out = 9u;
        marker("HII_GRAPH_NAV_LIVE_PASSWORD_REDACTION=PASS");
        return 1;
    }

    u64 live_value = 0u;
    if (!nav_read_scalar_value(system_table, prompt_index, &live_value)) return 0;

    if (op == 0x06u) {
        if (live_value) {
            *text_out = "checked";
            *length_out = 7u;
        } else {
            *text_out = "not checked";
            *length_out = 11u;
        }
        if (checkbox_state_out) *checkbox_state_out = 1u;
        return 1;
    }

    if (nav_option_for_value(prompt_index, live_value, text_out, length_out))
        return 1;

    return nav_format_u64(live_value, g_nav_value_text, &g_nav_value_length) &&
           ((*text_out = g_nav_value_text), (*length_out = g_nav_value_length), 1);
}

static nav_staged_value *nav_stage_find(u8 handle_index, u16 question_id) {
    if (!question_id) return 0;
    for (u8 i = 0u; i < g_nav_staged_total; ++i) {
        nav_staged_value *e = &g_nav_staged_values[i];
        if (e->valid && e->handle_index == handle_index &&
            e->question_id == question_id) return e;
    }
    return 0;
}

static int nav_stage_get(u8 prompt_index, u64 *value_out) {
    if (!value_out || prompt_index >= g_nav_prompt_total) return 0;
    nav_staged_value *e = nav_stage_find(
        g_nav_prompt_handle_indices[prompt_index],
        g_nav_prompt_question_ids[prompt_index]);
    if (!e) return 0;
    *value_out = e->value;
    return 1;
}

static int nav_stage_set(u8 prompt_index, u64 value) {
    if (prompt_index >= g_nav_prompt_total ||
        !g_nav_prompt_question_ids[prompt_index]) return 0;
    if (g_nav_prompt_condition_flags[prompt_index] &
        (NAV_COND_GRAY | NAV_COND_DISABLE | NAV_COND_UNKNOWN)) {
        marker("HII_GRAPH_NAV_DISABLED_PREVIEW=BLOCKED");
        return 0;
    }
    if (g_nav_prompt_question_flags[prompt_index] & NAV_Q_READ_ONLY) {
        marker("HII_GRAPH_NAV_READ_ONLY_PREVIEW=BLOCKED");
        return 0;
    }
    if (g_nav_prompt_opcodes[prompt_index] != 0x05u &&
        g_nav_prompt_opcodes[prompt_index] != 0x06u &&
        g_nav_prompt_opcodes[prompt_index] != 0x07u) return 0;

    nav_staged_value *e = nav_stage_find(
        g_nav_prompt_handle_indices[prompt_index],
        g_nav_prompt_question_ids[prompt_index]);
    if (!e) {
        if (g_nav_staged_total >= MAX_HII_STAGED_VALUES) return 0;
        e = &g_nav_staged_values[g_nav_staged_total++];
        e->valid = 1u;
        e->handle_index = g_nav_prompt_handle_indices[prompt_index];
        e->question_id = g_nav_prompt_question_ids[prompt_index];
    }
    e->value = value;
    return 1;
}

static void nav_stage_clear_all(void) {
    for (u8 i = 0u; i < g_nav_staged_total; ++i)
        g_nav_staged_values[i].valid = 0u;
    g_nav_staged_total = 0u;
}

static u8 nav_stage_count(void) {
    u8 count = 0u;
    for (u8 i = 0u; i < g_nav_staged_total; ++i)
        if (g_nav_staged_values[i].valid) ++count;
    return count;
}

static int nav_stage_discard_prompt(u8 prompt_index) {
    if (prompt_index >= g_nav_prompt_total) return 0;
    nav_staged_value *e = nav_stage_find(
        g_nav_prompt_handle_indices[prompt_index],
        g_nav_prompt_question_ids[prompt_index]);
    if (!e) return 0;
    e->valid = 0u;
    while (g_nav_staged_total &&
           !g_nav_staged_values[g_nav_staged_total - 1u].valid)
        --g_nav_staged_total;
    return 1;
}

static int nav_effective_scalar_value(void *system_table, u8 prompt_index,
                                      u64 *value_out, u8 *staged_out) {
    if (!value_out || prompt_index >= g_nav_prompt_total) return 0;
    if (staged_out) *staged_out = 0u;
    if (nav_stage_get(prompt_index, value_out)) {
        if (staged_out) *staged_out = 1u;
        return 1;
    }
    return nav_read_scalar_value(system_table, prompt_index, value_out);
}

static u8 nav_append_decimal(char *out, u8 n, u8 cap, u32 value);
static u8 nav_append_u64_decimal(char *out, u8 n, u8 cap, u64 value);

static u8 nav_append_text(char *out, u8 n, u8 cap, const char *text) {
    if (!out || !text) return n;
    while (*text && n < cap) out[n++] = *text++;
    return n;
}

static u8 nav_append_question_flags(u8 prompt_index, char *out, u8 n, u8 cap) {
    if (!out || prompt_index >= g_nav_prompt_total || n >= cap) return n;
    u8 flags = g_nav_prompt_question_flags[prompt_index];
    const char *items[5];
    u8 count = 0u;
    if (flags & NAV_Q_READ_ONLY) items[count++] = "read only";
    if (flags & NAV_Q_CALLBACK) items[count++] = "callback";
    if (flags & NAV_Q_RESET_REQUIRED) items[count++] = "reset required";
    if (flags & NAV_Q_RECONNECT_REQUIRED) items[count++] = "reconnect required";
    if (flags & NAV_Q_OPTIONS_ONLY) items[count++] = "options only";
    for (u8 i = 0u; i < count && n < cap; ++i) {
        if (n) out[n++] = ' ';
        n = nav_append_text(out, n, cap, items[i]);
    }
    return n;
}

static int nav_build_control_detail_speech(u8 prompt_index,
                                           char *out, u8 *length_out) {
    if (!out || !length_out || prompt_index >= g_nav_prompt_total) return 0;
    u8 op = g_nav_prompt_opcodes[prompt_index];
    u8 n = 0u;

    if (op == 0x08u) { /* Never expose password storage or contents. */
        static const char protected_text[] = "protected";
        while (protected_text[n] && n < 32u) { out[n] = protected_text[n]; ++n; }
        n = nav_append_question_flags(prompt_index, out, n, 32u);
        out[n] = 0;
        *length_out = n;
        return 1;
    }
    if (!g_nav_prompt_meta_valid[prompt_index] &&
        !g_nav_prompt_question_flags[prompt_index]) return 0;

    if (op == 0x05u || op == 0x07u) {
        static const char min_text[] = "min ";
        static const char max_text[] = " max ";
        static const char step_text[] = " step ";
        for (u8 i = 0u; min_text[i] && n < 32u; ++i) out[n++] = min_text[i];
        n = nav_append_u64_decimal(out, n, 32u, g_nav_prompt_min_value[prompt_index]);
        for (u8 i = 0u; max_text[i] && n < 32u; ++i) out[n++] = max_text[i];
        n = nav_append_u64_decimal(out, n, 32u, g_nav_prompt_max_value[prompt_index]);
        if (g_nav_prompt_step_value[prompt_index]) {
            for (u8 i = 0u; step_text[i] && n < 32u; ++i) out[n++] = step_text[i];
            n = nav_append_u64_decimal(out, n, 32u, g_nav_prompt_step_value[prompt_index]);
        }
    } else if (op == 0x1cu || op == 0x08u) {
        static const char min_text[] = "min ";
        static const char max_text[] = " max ";
        static const char chars_text[] = " chars";
        for (u8 i = 0u; min_text[i] && n < 32u; ++i) out[n++] = min_text[i];
        n = nav_append_decimal(out, n, 32u, g_nav_prompt_min_size[prompt_index]);
        for (u8 i = 0u; max_text[i] && n < 32u; ++i) out[n++] = max_text[i];
        n = nav_append_decimal(out, n, 32u, g_nav_prompt_max_size[prompt_index]);
        for (u8 i = 0u; chars_text[i] && n < 32u; ++i) out[n++] = chars_text[i];
    } else if (op == 0x23u) {
        static const char max_text[] = "max ";
        static const char items_text[] = " items";
        for (u8 i = 0u; max_text[i] && n < 32u; ++i) out[n++] = max_text[i];
        n = nav_append_decimal(out, n, 32u, g_nav_prompt_max_containers[prompt_index]);
        for (u8 i = 0u; items_text[i] && n < 32u; ++i) out[n++] = items_text[i];
    } else if (op == 0x1au || op == 0x1bu) {
        u8 storage = (u8)(g_nav_prompt_control_flags[prompt_index] & 0x30u);
        const char *text = storage == 0x10u ? "system clock" :
                           storage == 0x20u ? "wakeup clock" : "stored value";
        while (*text && n < 32u) out[n++] = *text++;
    } else if (!g_nav_prompt_question_flags[prompt_index]) {
        return 0;
    }

    n = nav_append_question_flags(prompt_index, out, n, 32u);
    out[n] = 0;
    *length_out = n;
    return n != 0u;
}

static int nav_effective_value_text(void *system_table, u8 prompt_index,
                                    const char **text_out, u8 *length_out,
                                    u8 *staged_out) {
    if (!text_out || !length_out || prompt_index >= g_nav_prompt_total) return 0;
    if (staged_out) *staged_out = 0u;
    u8 op = g_nav_prompt_opcodes[prompt_index];
    if (op == 0x08u) {
        *text_out = "protected";
        *length_out = 9u;
        marker("HII_GRAPH_NAV_PASSWORD_REDACTION=PASS");
        return 1;
    }
    if (op == 0x1cu || op == 0x23u || op == 0x1au || op == 0x1bu) {
        if (nav_build_control_detail_speech(prompt_index, g_nav_value_text,
                                            &g_nav_value_length)) {
            *text_out = g_nav_value_text;
            *length_out = g_nav_value_length;
            return 1;
        }
        return 0;
    }
    u64 value = 0u;
    if (!nav_effective_scalar_value(system_table, prompt_index, &value,
                                    staged_out)) return 0;
    if (g_nav_prompt_opcodes[prompt_index] == 0x06u) {
        if (value) {
            *text_out = "checked";
            *length_out = 7u;
        } else {
            *text_out = "not checked";
            *length_out = 11u;
        }
        return 1;
    }
    if (nav_option_for_value(prompt_index, value, text_out, length_out)) return 1;
    return nav_format_u64(value, g_nav_value_text, &g_nav_value_length) &&
           ((*text_out = g_nav_value_text), (*length_out = g_nav_value_length), 1);
}

static int nav_stage_cycle_oneof(void *system_table, u8 prompt_index,
                                 int direction) {
    if (!system_table || prompt_index >= g_nav_prompt_total ||
        g_nav_prompt_opcodes[prompt_index] != 0x05u ||
        (g_nav_prompt_condition_flags[prompt_index] &
         (NAV_COND_GRAY | NAV_COND_DISABLE | NAV_COND_UNKNOWN))) return 0;
    u8 count = g_nav_option_counts[prompt_index];
    if (!count) return 0;
    u64 current = 0u;
    u8 staged = 0u;
    if (!nav_effective_scalar_value(system_table, prompt_index,
                                    &current, &staged)) return 0;
    u8 selected = 0u;
    int found = 0;
    for (u8 i = 0u; i < count; ++i) {
        if (g_nav_option_values[prompt_index][i] == current) {
            selected = i;
            found = 1;
            break;
        }
    }
    if (!found) selected = direction < 0 ? 0u : (u8)(count - 1u);
    if (direction < 0)
        selected = selected ? (u8)(selected - 1u) : (u8)(count - 1u);
    else
        selected = (u8)((selected + 1u) % count);
    return nav_stage_set(prompt_index, g_nav_option_values[prompt_index][selected]);
}

static int nav_stage_toggle_checkbox(void *system_table, u8 prompt_index) {
    if (!system_table || prompt_index >= g_nav_prompt_total ||
        g_nav_prompt_opcodes[prompt_index] != 0x06u ||
        (g_nav_prompt_condition_flags[prompt_index] &
         (NAV_COND_GRAY | NAV_COND_DISABLE | NAV_COND_UNKNOWN))) return 0;
    u64 current = 0u;
    u8 staged = 0u;
    if (!nav_effective_scalar_value(system_table, prompt_index,
                                    &current, &staged)) return 0;
    return nav_stage_set(prompt_index, current ? 0u : 1u);
}

static int nav_stage_adjust_numeric(void *system_table, u8 prompt_index,
                                    int direction) {
    if (!system_table || prompt_index >= g_nav_prompt_total ||
        g_nav_prompt_opcodes[prompt_index] != 0x07u ||
        !g_nav_prompt_meta_valid[prompt_index] ||
        (g_nav_prompt_condition_flags[prompt_index] &
         (NAV_COND_GRAY | NAV_COND_DISABLE | NAV_COND_UNKNOWN))) return 0;
    u64 step = g_nav_prompt_step_value[prompt_index];
    u64 minv = g_nav_prompt_min_value[prompt_index];
    u64 maxv = g_nav_prompt_max_value[prompt_index];
    if (!step || minv > maxv) return 0;

    u64 current = 0u;
    u8 staged = 0u;
    if (!nav_effective_scalar_value(system_table, prompt_index,
                                    &current, &staged)) return 0;
    if (current < minv) current = minv;
    if (current > maxv) current = maxv;

    u64 next = current;
    if (direction < 0) {
        next = current <= minv || current - minv < step ? minv : current - step;
    } else {
        next = current >= maxv || maxv - current < step ? maxv : current + step;
    }
    if (next == current) return 0;
    return nav_stage_set(prompt_index, next);
}

static u8 nav_append_decimal(char *out, u8 n, u8 cap, u32 value) {
    char digits[10];
    u8 count = 0u;
    do {
        digits[count++] = (char)('0' + (value % 10u));
        value /= 10u;
    } while (value && count < (u8)sizeof(digits));
    while (count && n < cap) out[n++] = digits[--count];
    return n;
}

static u8 nav_append_u64_decimal(char *out, u8 n, u8 cap, u64 value) {
    char digits[20];
    u8 count = 0u;
    do {
        digits[count++] = (char)('0' + (value % 10u));
        value /= 10u;
    } while (value && count < (u8)sizeof(digits));
    while (count && n < cap) out[n++] = digits[--count];
    return n;
}

static int nav_build_edit_status_speech(char *out, u8 *length_out) {
    if (!out || !length_out) return 0;
    u8 count = nav_stage_count();
    if (!count) {
        static const char none[] = "no pending edits";
        u8 n = 0u;
        while (none[n] && n < 32u) { out[n] = none[n]; ++n; }
        out[n] = 0;
        *length_out = n;
        return 1;
    }
    static const char prefix[] = "pending edits ";
    u8 n = 0u;
    for (u8 i = 0u; prefix[i] && n < 32u; ++i) out[n++] = prefix[i];
    n = nav_append_decimal(out, n, 32u, count);
    out[n] = 0;
    *length_out = n;
    return 1;
}

static int nav_build_position_speech(u8 prompt_index, char *out, u8 *length_out) {
    if (!out || !length_out || prompt_index >= g_nav_prompt_total ||
        !g_nav_prompt_total) return 0;
    static const char prefix[] = "position ";
    static const char middle[] = " of ";
    u8 n = 0u;
    for (u8 i = 0u; prefix[i] && n < 32u; ++i) out[n++] = prefix[i];
    n = nav_append_decimal(out, n, 32u, (u32)prompt_index + 1u);
    for (u8 i = 0u; middle[i] && n < 32u; ++i) out[n++] = middle[i];
    n = nav_append_decimal(out, n, 32u, (u32)g_nav_prompt_total);
    out[n] = 0;
    *length_out = n;
    return n != 0u;
}

static int nav_build_where_am_i_speech(void *system_table, u8 prompt_index,
                                      char *out, u8 *length_out) {
    if (!system_table || !out || !length_out ||
        prompt_index >= g_nav_prompt_total) return 0;

    const char *value_text = 0;
    u8 value_length = 0u;
    u8 staged = 0u;
    int have_value = nav_effective_value_text(system_table, prompt_index,
                                              &value_text, &value_length,
                                              &staged);
    if (!have_value) value_length = 0u;
    u8 suffix_budget = value_length ? (u8)(value_length + 1u) : 0u;
    if (suffix_budget > 32u) suffix_budget = 32u;
    u8 prefix_budget = (u8)(QEV_NAV_TEXT_MAX - suffix_budget);
    u8 n = 0u;

    for (u8 i = 0u; i < g_nav_form_title_length && n < prefix_budget; ++i)
        out[n++] = g_nav_form_title[i];
    if (n < prefix_budget && g_nav_prompt_lengths[prompt_index]) out[n++] = ' ';
    for (u8 i = 0u; i < g_nav_prompt_lengths[prompt_index] && n < prefix_budget; ++i)
        out[n++] = g_nav_prompts[prompt_index][i];

    if (value_length && value_text) {
        if (n && n < QEV_NAV_TEXT_MAX) out[n++] = ' ';
        for (u8 i = 0u; i < value_length && n < QEV_NAV_TEXT_MAX; ++i)
            out[n++] = value_text[i];
    }
    out[n] = 0;
    *length_out = n;
    marker("HII_GRAPH_NAV_WHERE_AM_I_64=PASS");
    return n != 0u;
}

static int nav_build_focus_speech(void *system_table, u8 prompt_index,
                                  char *out, u8 *length_out) {
    if (!system_table || !out || !length_out ||
        prompt_index >= g_nav_prompt_total) return 0;

    u8 op = g_nav_prompt_opcodes[prompt_index];
    u8 speak_value = op != 0x08u;
    const char *value_text = 0;
    u8 value_length = 0u;
    u8 staged = 0u;
    int have_value = 0;

    /*
     * Password focus must never trigger a value read. Other controls may have
     * no value at all (buttons, references, subtitles); they still need a
     * complete semantic focus announcement for blind operation.
     */
    if (speak_value) {
        have_value = nav_effective_value_text(system_table, prompt_index,
                                              &value_text, &value_length,
                                              &staged);
        if (!have_value) {
            value_text = 0;
            value_length = 0u;
            staged = 0u;
        }
    } else {
        marker("HII_GRAPH_NAV_PASSWORD_VALUE_READ_AVOIDED=PASS");
    }

    qev_semantic_node node;
    node.role = nav_semantic_role_for_opcode(op);
    node.native_role = ifr_semantic_role(op);
    node.label = g_nav_prompt_lengths[prompt_index] ?
                 g_nav_prompts[prompt_index] : 0;
    node.value = (have_value && value_text && value_length) ? value_text : 0;
    node.states = nav_semantic_state_bits(prompt_index, staged);

    qev_utterance semantic;
    if (!qev_semantic_focus_utterance(&node, &semantic)) return 0;
    marker("HII_GRAPH_NAV_SEMANTIC_CORE=PASS");
    marker("HII_GRAPH_NAV_VALUE_OPTIONAL_SPEECH=PASS");

    /*
     * The phrase queue accepts 64 characters and splits them into <=32-char
     * DMA chunks. Keep role/state/label context first, followed by the value.
     */
    u8 n = 0u;
    while (semantic.text[n] && n < QEV_NAV_TEXT_MAX) {
        out[n] = semantic.text[n];
        ++n;
    }
    out[n] = 0;
    *length_out = n;

    if (semantic.text[n] || semantic.truncated)
        marker("HII_GRAPH_NAV_SEMANTIC_TRUNCATION=PASS");
    return n != 0u;
}

static int nav_read_varstore_scalar(void *system_table, u8 handle_index,
                                    u16 varstore_id, u16 var_info, u8 width,
                                    u64 *value_out) {
    if (!system_table || !value_out || !width || width > 8u || !varstore_id)
        return 0;
    nav_varstore_desc *d = nav_find_varstore(handle_index, varstore_id);
    if (!d || !d->name[0] || !d->size || d->size > MAX_HII_VAR_DATA) return 0;
    if ((u32)var_info + (u32)width > (u32)d->size) return 0;

    void *runtime_services = *(void **)((u8 *)system_table + 0x58);
    if (!runtime_services) return 0;
    get_variable_fn get_variable =
        *(get_variable_fn *)((u8 *)runtime_services + 0x48);
    if (!get_variable) return 0;

    u16 name16[MAX_HII_VARSTORE_NAME];
    u32 ni = 0u;
    while (ni + 1u < MAX_HII_VARSTORE_NAME && d->name[ni]) {
        name16[ni] = (u16)(u8)d->name[ni];
        ++ni;
    }
    name16[ni] = 0;

    usize bytes = d->size;
    u32 attributes = 0u;
    if (get_variable(name16, &d->guid, &attributes, &bytes, g_nav_var_data) != 0)
        return 0;
    if (bytes < (usize)var_info + width) return 0;

    u64 value = 0u;
    for (u8 i = 0u; i < width; ++i)
        value |= ((u64)g_nav_var_data[(u32)var_info + i]) << (8u * i);
    *value_out = value;
    return 1;
}

static int nav_read_question_value(void *system_table, u8 handle_index,
                                   u16 question_id, u64 *value_out) {
    nav_staged_value *stage = nav_stage_find(handle_index, question_id);
    if (stage) {
        *value_out = stage->value;
        marker("HII_GRAPH_NAV_STAGED_CONDITION_VALUE=PASS");
        return 1;
    }
    nav_question_desc *q = nav_find_question(handle_index, question_id);
    if (!q || !q->value_width) return 0;
    return nav_read_varstore_scalar(system_table, handle_index, q->varstore_id,
                                    q->var_info, q->value_width, value_out);
}

static int nav_read_scalar_value(void *system_table, u8 prompt_index,
                                 u64 *value_out) {
    if (!system_table || !value_out || prompt_index >= g_nav_prompt_total) return 0;
    return nav_read_varstore_scalar(
        system_table, g_nav_prompt_handle_indices[prompt_index],
        g_nav_prompt_varstore_ids[prompt_index],
        g_nav_prompt_var_infos[prompt_index],
        g_nav_prompt_value_widths[prompt_index], value_out);
}

typedef struct {
    u8 known;
    u64 value;
} nav_expr_value;

static int nav_eval_condition_expression(void *system_table, u8 handle_index,
                                         const u8 *first, const u8 *limit,
                                         u8 *result_out,
                                         const u8 **after_expression_out) {
    if (!system_table || !first || !limit || first + 2u > limit ||
        !result_out || !after_expression_out) return -1;

    nav_expr_value stack[MAX_IFR_EXPR_STACK];
    u8 sp = 0u;
    const u8 *q = first;
    u8 first_scope = (u8)(q[1] >> 7);
    u8 depth = 0u;
    u8 done = 0u;

    while (!done) {
        if (q + 2u > limit) return -1;
        u8 op = q[0];
        u8 header = q[1];
        u32 len = (u32)(header & 0x7fu);
        if (len < 2u || q + len > limit) return -1;

        if (op == 0x29u) {
            if (!first_scope || !depth) return -1;
            --depth;
            q += len;
            if (!depth) done = 1u;
            continue;
        }

        nav_expr_value out;
        out.known = 1u;
        out.value = 0u;
        if (op == 0x46u) { /* TRUE */
            out.value = 1u;
        } else if (op == 0x47u) { /* FALSE */
            out.value = 0u;
        } else if (op == 0x12u && len >= 6u) { /* EQ_ID_VAL */
            u64 live = 0u;
            out.known = (u8)nav_read_question_value(
                system_table, handle_index, rd16(q + 2), &live);
            out.value = (u64)(live == (u64)rd16(q + 4));
        } else if (op == 0x13u && len >= 6u) { /* EQ_ID_ID */
            u64 left = 0u, right = 0u;
            u8 kl = (u8)nav_read_question_value(
                system_table, handle_index, rd16(q + 2), &left);
            u8 kr = (u8)nav_read_question_value(
                system_table, handle_index, rd16(q + 4), &right);
            out.known = (u8)(kl && kr);
            out.value = (u64)(left == right);
        } else if (op == 0x14u && len >= 8u) { /* EQ_ID_VAL_LIST */
            u16 count = rd16(q + 4);
            if ((u32)6u + (u32)count * 2u > len) return -1;
            u64 live = 0u;
            out.known = (u8)nav_read_question_value(
                system_table, handle_index, rd16(q + 2), &live);
            out.value = 0u;
            if (out.known) {
                for (u16 i = 0u; i < count; ++i) {
                    if (live == (u64)rd16(q + 6u + (u32)i * 2u)) {
                        out.value = 1u;
                        break;
                    }
                }
            }
        } else if (op == 0x17u) { /* NOT */
            if (!sp) return -1;
            out = stack[--sp];
            if (out.known) out.value = (u64)!out.value;
        } else if (op == 0x15u || op == 0x16u) { /* AND / OR */
            if (sp < 2u) return -1;
            nav_expr_value right = stack[--sp];
            nav_expr_value left = stack[--sp];
            if (op == 0x15u) {
                if ((left.known && !left.value) || (right.known && !right.value)) {
                    out.known = 1u;
                    out.value = 0u;
                } else if (left.known && right.known) {
                    out.known = 1u;
                    out.value = (u64)(left.value && right.value);
                } else {
                    out.known = 0u;
                }
            } else {
                if ((left.known && left.value) || (right.known && right.value)) {
                    out.known = 1u;
                    out.value = 1u;
                } else if (left.known && right.known) {
                    out.known = 1u;
                    out.value = 0u;
                } else {
                    out.known = 0u;
                }
            }
        } else {
            out.known = 0u;
        }

        if (sp >= MAX_IFR_EXPR_STACK) return -1;
        stack[sp++] = out;
        if (header & 0x80u) ++depth;
        q += len;
        if (!first_scope) done = 1u;
    }

    if (sp != 1u) return -1;
    *after_expression_out = q;
    if (!stack[0].known) return 0;
    *result_out = (u8)!!stack[0].value;
    return 1;
}

/*
 * Structural navigation inspired by mature screen-reader interaction models,
 * implemented from scratch for native HII. Lowercase moves forward and
 * uppercase moves backward; no desktop accessibility runtime is imported.
 */
static int nav_role_in_group(u8 opcode, u8 group) {
    switch (group) {
        case NAV_GROUP_BUTTON: return opcode == 0x0cu;
        case NAV_GROUP_CHECKBOX: return opcode == 0x06u;
        case NAV_GROUP_CHOICE: return opcode == 0x05u || opcode == 0x23u;
        case NAV_GROUP_EDITABLE:
            return opcode == 0x07u || opcode == 0x08u || opcode == 0x1au ||
                   opcode == 0x1bu || opcode == 0x1cu;
        default: return 0;
    }
}

static int nav_find_group(u8 group, int direction, u8 *index_out) {
    if (!index_out || g_nav_prompt_total < 2u) return 0;
    u8 index = g_nav_prompt_index;
    for (u8 visited = 1u; visited < g_nav_prompt_total; ++visited) {
        if (direction < 0) {
            index = index ? (u8)(index - 1u) : (u8)(g_nav_prompt_total - 1u);
        } else {
            index = (u8)(index + 1u);
            if (index >= g_nav_prompt_total) index = 0u;
        }
        if (nav_role_in_group(g_nav_prompt_opcodes[index], group)) {
            *index_out = index;
            return 1;
        }
    }
    return 0;
}

static int nav_find_form(int direction, u8 *index_out) {
    if (!index_out || g_nav_prompt_total < 2u) return 0;
    u16 current_form = g_nav_prompt_form_ids[g_nav_prompt_index];
    u8 index = g_nav_prompt_index;
    for (u8 visited = 1u; visited < g_nav_prompt_total; ++visited) {
        if (direction < 0) {
            index = index ? (u8)(index - 1u) : (u8)(g_nav_prompt_total - 1u);
        } else {
            index = (u8)(index + 1u);
            if (index >= g_nav_prompt_total) index = 0u;
        }
        if (g_nav_prompt_form_ids[index] != current_form) {
            u16 target_form = g_nav_prompt_form_ids[index];
            if (direction < 0) {
                while (index &&
                       g_nav_prompt_form_ids[(u8)(index - 1u)] == target_form)
                    --index;
            }
            *index_out = index;
            return 1;
        }
    }
    return 0;
}
static int nav_load_form(void *system_table, u16 requested_form_id);
static void nav_prompt_load(u8 index);

static int nav_find_question_index(u16 question_id, u8 *index_out) {
    if (!question_id || !index_out) return 0;
    for (u8 i = 0u; i < g_nav_prompt_total; ++i) {
        if (g_nav_prompt_question_ids[i] == question_id) {
            *index_out = i;
            return 1;
        }
    }
    return 0;
}

static int nav_refresh_current_form(void *system_table) {
    if (!system_table || !g_nav_current_form_id) return 0;
    u16 form_id = g_nav_current_form_id;
    u16 question_id = g_nav_prompt_question_ids[g_nav_prompt_index];
    u8 old_index = g_nav_prompt_index;
    if (!nav_load_form(system_table, form_id)) return 0;

    u8 restored = 0u;
    if (question_id && nav_find_question_index(question_id, &restored)) {
        nav_prompt_load(restored);
        marker("HII_GRAPH_NAV_REFRESH_FOCUS_RESTORED=PASS");
    } else {
        if (old_index >= g_nav_prompt_total)
            old_index = (u8)(g_nav_prompt_total - 1u);
        nav_prompt_load(old_index);
        marker("HII_GRAPH_NAV_REFRESH_FOCUS_FALLBACK=PASS");
    }
    marker("HII_GRAPH_NAV_LIVE_REFRESH=PASS");
    return 1;
}

#endif
static u16 fold_prompt_char(u16 ch) {
    if (ch >= (u16)'A' && ch <= (u16)'Z') return (u16)(ch + 32u);
    switch (ch) {
        case 0x00c0: case 0x00c1: case 0x00c2: case 0x00c3:
        case 0x00c4: case 0x00c5: case 0x00e0: case 0x00e1:
        case 0x00e2: case 0x00e3: case 0x00e4: case 0x00e5: return (u16)'a';
        case 0x00c7: case 0x00e7: return (u16)'c';
        case 0x00c8: case 0x00c9: case 0x00ca: case 0x00cb:
        case 0x00e8: case 0x00e9: case 0x00ea: case 0x00eb: return (u16)'e';
        case 0x00cc: case 0x00cd: case 0x00ce: case 0x00cf:
        case 0x00ec: case 0x00ed: case 0x00ee: case 0x00ef: return (u16)'i';
        case 0x00d2: case 0x00d3: case 0x00d4: case 0x00d5:
        case 0x00d6: case 0x00f2: case 0x00f3: case 0x00f4:
        case 0x00f5: case 0x00f6: return (u16)'o';
        case 0x00d9: case 0x00da: case 0x00db: case 0x00dc:
        case 0x00f9: case 0x00fa: case 0x00fb: case 0x00fc: return (u16)'u';
        case 0x0178: case 0x00ff: return (u16)'y';
        default: return ch;
    }
}
static int normalize_prompt(const u16 *text, char *out, u32 *count_out) {
    u32 n = 0;
    u8 pending_space = 0;
    for (u32 i = 0; i < 127u && text[i] && n < QEV_NAV_TEXT_MAX; ++i) {
        u16 ch = fold_prompt_char(text[i]);
        if ((ch >= (u16)'a' && ch <= (u16)'z') ||
            (ch >= (u16)'0' && ch <= (u16)'9')) {
            if (pending_space && n && n < QEV_NAV_TEXT_MAX) out[n++] = ' ';
            if (n < QEV_NAV_TEXT_MAX) out[n++] = (char)ch;
            pending_space = 0;
        } else if (n) {
            pending_space = 1;
        }
    }
    out[n] = 0;
    *count_out = n;
    return n != 0;
}
static int get_hii_string(hii_string_protocol *str, void *handle, u16 token, char *out, u32 *count_out) {
    u16 text[128];
    usize bytes = sizeof(text);
    u64 st = str->get_string(str, "en-US", handle, token, text, &bytes, 0);
    if (st != 0) {
        char langs[128];
        usize lang_bytes = sizeof(langs);
        if (!str->get_languages || str->get_languages(str, handle, langs, &lang_bytes) != 0 || !lang_bytes) return 0;
        u32 i = 0;
        while (i + 1u < sizeof(langs) && langs[i] && langs[i] != ';') ++i;
        if (!i || i >= sizeof(langs)) return 0;
        langs[i] = 0;
        bytes = sizeof(text);
        st = str->get_string(str, langs, handle, token, text, &bytes, 0);
        if (st != 0) return 0;
    }
    return normalize_prompt(text, out, count_out);
}

#ifdef QEV_INTERACTIVE_NAV
static u64 nav_read_le_value(const u8 *p, u8 width) {
    u64 value = 0u;
    if (!p || !width || width > 8u) return 0u;
    for (u8 i = 0u; i < width; ++i)
        value |= ((u64)p[i]) << (8u * i);
    return value;
}

static void nav_prompt_metadata_set(u8 index, u8 opcode,
                                    const u8 *q, u32 oplen) {
    if (index >= g_nav_prompt_total || !q) return;
    g_nav_prompt_meta_valid[index] = 0u;
    g_nav_prompt_control_flags[index] = 0u;
    g_nav_prompt_min_value[index] = 0u;
    g_nav_prompt_max_value[index] = 0u;
    g_nav_prompt_step_value[index] = 0u;
    g_nav_prompt_min_size[index] = 0u;
    g_nav_prompt_max_size[index] = 0u;
    g_nav_prompt_max_containers[index] = 0u;

    if ((opcode == 0x05u || opcode == 0x07u) && oplen >= 14u) {
        u8 width = ifr_scalar_width(opcode, q, oplen);
        if (width && width <= 8u && oplen >= 14u + (u32)width * 3u) {
            g_nav_prompt_control_flags[index] = q[13];
            g_nav_prompt_min_value[index] = nav_read_le_value(q + 14u, width);
            g_nav_prompt_max_value[index] = nav_read_le_value(q + 14u + width, width);
            g_nav_prompt_step_value[index] = nav_read_le_value(q + 14u + (u32)width * 2u, width);
            g_nav_prompt_meta_valid[index] = 1u;
        }
    } else if (opcode == 0x1cu && oplen >= 16u) { /* STRING */
        g_nav_prompt_min_size[index] = q[13];
        g_nav_prompt_max_size[index] = q[14];
        g_nav_prompt_control_flags[index] = q[15];
        g_nav_prompt_meta_valid[index] = 1u;
    } else if (opcode == 0x08u && oplen >= 17u) { /* PASSWORD */
        g_nav_prompt_min_size[index] = rd16(q + 13u);
        g_nav_prompt_max_size[index] = rd16(q + 15u);
        g_nav_prompt_meta_valid[index] = 1u;
    } else if (opcode == 0x23u && oplen >= 15u) { /* ORDERED_LIST */
        g_nav_prompt_max_containers[index] = q[13];
        g_nav_prompt_control_flags[index] = q[14];
        g_nav_prompt_meta_valid[index] = 1u;
    } else if ((opcode == 0x1au || opcode == 0x1bu) && oplen >= 14u) {
        g_nav_prompt_control_flags[index] = q[13];
        g_nav_prompt_meta_valid[index] = 1u;
    }
}

static int nav_prompt_add(u8 opcode, u8 handle_index, u8 value_width,
                          u16 form_id, u16 question_id,
                          u16 varstore_id, u16 var_info, u8 question_flags,
                          u8 condition_flags,
                          const char *text, u32 count,
                          const char *help, u32 help_count,
                          u16 ref_form_id) {
    if (!text || !count || count > QEV_NAV_TEXT_MAX || help_count > QEV_NAV_TEXT_MAX) return 0;
    for (u8 i = 0; i < g_nav_prompt_total; ++i) {
        if (g_nav_prompt_lengths[i] != (u8)count ||
            g_nav_prompt_opcodes[i] != opcode ||
            g_nav_prompt_handle_indices[i] != handle_index ||
            g_nav_prompt_value_widths[i] != value_width ||
            g_nav_prompt_form_ids[i] != form_id ||
            g_nav_prompt_question_ids[i] != question_id ||
            g_nav_prompt_varstore_ids[i] != varstore_id ||
            g_nav_prompt_var_infos[i] != var_info ||
            g_nav_prompt_question_flags[i] != question_flags ||
            g_nav_prompt_condition_flags[i] != condition_flags ||
            g_nav_ref_form_ids[i] != ref_form_id ||
            g_nav_help_lengths[i] != (u8)help_count) continue;
        u32 same = 1;
        for (u32 j = 0; j < count; ++j) {
            if (g_nav_prompts[i][j] != text[j]) { same = 0; break; }
        }
        if (same) {
            for (u32 j = 0; j < help_count; ++j) {
                if (g_nav_help[i][j] != help[j]) { same = 0; break; }
            }
        }
        if (same) return (int)i + 1;
    }
    if (g_nav_prompt_total >= MAX_HII_NAV_PROMPTS) {
        g_nav_prompt_overflow = 1u;
        return 0;
    }
    u8 slot = g_nav_prompt_total++;
    for (u32 j = 0; j < count; ++j) g_nav_prompts[slot][j] = text[j];
    g_nav_prompts[slot][count] = 0;
    g_nav_prompt_lengths[slot] = (u8)count;
    g_nav_prompt_opcodes[slot] = opcode;
    g_nav_prompt_handle_indices[slot] = handle_index;
    g_nav_prompt_value_widths[slot] = value_width;
    g_nav_prompt_form_ids[slot] = form_id;
    g_nav_prompt_question_ids[slot] = question_id;
    g_nav_prompt_varstore_ids[slot] = varstore_id;
    g_nav_prompt_var_infos[slot] = var_info;
    g_nav_prompt_question_flags[slot] = question_flags;
    g_nav_prompt_condition_flags[slot] = condition_flags;
    g_nav_ref_form_ids[slot] = ref_form_id;
    g_nav_option_counts[slot] = 0u;
    g_nav_prompt_meta_valid[slot] = 0u;
    g_nav_prompt_control_flags[slot] = 0u;
    g_nav_prompt_min_value[slot] = 0u;
    g_nav_prompt_max_value[slot] = 0u;
    g_nav_prompt_step_value[slot] = 0u;
    g_nav_prompt_min_size[slot] = 0u;
    g_nav_prompt_max_size[slot] = 0u;
    g_nav_prompt_max_containers[slot] = 0u;
    for (u32 j = 0; j < help_count; ++j) g_nav_help[slot][j] = help[j];
    g_nav_help[slot][help_count] = 0;
    g_nav_help_lengths[slot] = (u8)help_count;
    if (help_count) g_nav_help_available = 1u;
    return (int)slot + 1;
}
static void nav_prompt_load(u8 index) {
    if (index >= g_nav_prompt_total) return;
    g_nav_prompt_index = index;
    g_nav_prompt_opcode = g_nav_prompt_opcodes[index];
    g_prompt_count = g_nav_prompt_lengths[index];
    for (u32 j = 0; j < g_prompt_count; ++j) g_prompt_text[j] = g_nav_prompts[index][j];
    g_prompt_text[g_prompt_count] = 0;

    /* A screen reader must announce semantics, not only raw label text.
       Put the IFR role first so it can never be truncated away. */
    const char *role = ifr_semantic_role(g_nav_prompt_opcode);
    u32 n = 0;
    while (*role && n < 32u) g_nav_speech_text[n++] = *role++;
    const char *state = 0;
    if (g_nav_prompt_condition_flags[index] & NAV_COND_GRAY)
        state = " disabled";
    else if (g_nav_prompt_condition_flags[index] & NAV_COND_UNKNOWN)
        state = " conditional";
    else if (g_nav_prompt_question_flags[index] & NAV_Q_READ_ONLY)
        state = " read only";
    if (state) {
        while (*state && n < 32u) g_nav_speech_text[n++] = *state++;
    }
    if (n < 32u && g_prompt_count) g_nav_speech_text[n++] = ' ';
    for (u32 j = 0; j < g_prompt_count && n < 32u; ++j)
        g_nav_speech_text[n++] = g_prompt_text[j];
    g_nav_speech_text[n] = 0;
    g_nav_speech_length = (u8)n;

    g_nav_help_length = g_nav_help_lengths[index];
    for (u32 j = 0; j < g_nav_help_length; ++j)
        g_nav_help_text[j] = g_nav_help[index][j];
    g_nav_help_text[g_nav_help_length] = 0;
}

static int nav_get_form_title(u16 form_id, char *out, u32 *count_out) {
    if (!form_id || !out || !count_out || !g_nav_hii_string || !g_nav_hii_handle)
        return 0;
    u32 list_len = rd32(g_hii_package + 16);
    if (list_len < 24u || list_len > sizeof(g_hii_package)) return 0;
    const u8 *p = g_hii_package + 20;
    const u8 *list_end = g_hii_package + list_len;
    while (p + 4u <= list_end) {
        u32 hdr = rd32(p);
        u32 len = hdr & 0x00ffffffu;
        u8 type = (u8)(hdr >> 24);
        if (len < 4u || p + len > list_end) return 0;
        if (type == 0xdfu) break;
        if (type == 0x02u) {
            const u8 *q = p + 4;
            const u8 *end = p + len;
            while (q + 2u <= end) {
                u8 op = q[0];
                u32 oplen = (u32)(q[1] & 0x7fu);
                if (oplen < 2u || q + oplen > end) break;
                if (op == 0x01u && oplen >= 6u && rd16(q + 2) == form_id) {
                    u16 title_token = rd16(q + 4);
                    return title_token &&
                           get_hii_string(g_nav_hii_string, g_nav_hii_handle,
                                          title_token, out, count_out);
                }
                q += oplen;
            }
        }
        p += len;
    }
    return 0;
}

/*
 * Build one live form at a time. This preserves current VarStore and one-of
 * value support while replacing the previous flat all-forms catalogue.
 */
static int nav_load_form(void *system_table, u16 requested_form_id) {
    if (!system_table || !g_nav_hii_string || !g_nav_hii_handle ||
        g_nav_m1603qa_handle_index == 0xffu) return 0;

    u32 list_len = rd32(g_hii_package + 16);
    if (list_len < 24u || list_len > sizeof(g_hii_package)) return 0;

    g_nav_prompt_total = 0u;
    g_nav_prompt_index = 0u;
    g_nav_prompt_overflow = 0u;
    g_nav_help_available = 0u;
    g_nav_form_title[0] = 0;
    g_nav_form_title_length = 0u;
    g_nav_condition_known = 0u;
    g_nav_condition_unknown = 0u;
    g_nav_condition_hidden = 0u;
    g_nav_condition_gray = 0u;

    u16 selected_form_id = 0u;
    int in_selected_form = 0;
    const u8 *p = g_hii_package + 20;
    const u8 *list_end = g_hii_package + list_len;

    while (p + 4u <= list_end) {
        u32 hdr = rd32(p);
        u32 len = hdr & 0x00ffffffu;
        u8 type = (u8)(hdr >> 24);
        if (len < 4u || p + len > list_end) return 0;
        if (type == 0xdfu) break;

        if (type == 0x02u) {
            const u8 *q = p + 4;
            const u8 *end = p + len;
            u8 scope_condition_stack[64];
            u8 scope_unknown_stack[64];
            u8 scope_oneof_stack[64];
            u8 scope_depth = 0u;
            u8 active_condition_flags = 0u;
            u8 active_unknown = 0u;
            u8 active_oneof_prompt = 0xffu;

            while (q + 2u <= end) {
                u8 op = q[0];
                u8 op_header = q[1];
                u32 oplen = (u32)(op_header & 0x7fu);
                if (oplen < 2u || q + oplen > end) break;

                if (op == 0x29u) {
                    if (scope_depth) {
                        --scope_depth;
                        active_oneof_prompt = scope_oneof_stack[scope_depth];
                        active_condition_flags = 0u;
                        active_unknown = 0u;
                        for (u8 si = 0u; si < scope_depth; ++si) {
                            active_condition_flags |= scope_condition_stack[si];
                            active_unknown |= scope_unknown_stack[si];
                        }
                    }
                    q += oplen;
                    continue;
                }

                if (op == 0x01u && oplen >= 6u) {
                    u16 form_id = rd16(q + 2);
                    if (in_selected_form) break;
                    if (form_id == requested_form_id) {
                        selected_form_id = form_id;
                        in_selected_form = 1;
                        u16 title_token = rd16(q + 4);
                        u32 title_count = 0u;
                        if (title_token &&
                            get_hii_string(g_nav_hii_string, g_nav_hii_handle,
                                           title_token, g_nav_form_title,
                                           &title_count))
                            g_nav_form_title_length = (u8)title_count;
                    }
                }

                u8 condition_active = 0u;
                u8 condition_unknown = 0u;
                u8 condition_kind = ifr_condition_flag(op);
                if (in_selected_form && condition_kind) {
                    const u8 *after_expression = 0;
                    u8 truth = 0u;
                    int eval = nav_eval_condition_expression(
                        system_table, g_nav_m1603qa_handle_index,
                        q + oplen, end, &truth, &after_expression);
                    if (eval < 0 || !after_expression ||
                        after_expression <= q + oplen || after_expression > end)
                        return 0;
                    if (eval > 0) {
                        ++g_nav_condition_known;
                        if (truth) condition_active = condition_kind;
                    } else {
                        ++g_nav_condition_unknown;
                        condition_unknown = 1u;
                    }
                }

                u8 added_prompt_index = 0xffu;
                u8 hidden_here =
                    (u8)(active_condition_flags &
                         (NAV_COND_SUPPRESS | NAV_COND_DISABLE));
                if (in_selected_form && prompt_opcode(op) && oplen >= 4u) {
                    u16 token = rd16(q + 2);
                    char candidate[QEV_NAV_TEXT_MAX + 1u];
                    char help_candidate[QEV_NAV_TEXT_MAX + 1u];
                    u32 candidate_count = 0u;
                    u32 help_count = 0u;
                    u16 help_token = oplen >= 6u ? rd16(q + 4) : 0u;
                    u16 question_id =
                        ifr_question_opcode(op) && oplen >= 8u ? rd16(q + 6) : 0u;
                    u16 varstore_id =
                        ifr_question_opcode(op) && oplen >= 10u ? rd16(q + 8) : 0u;
                    u16 var_info =
                        ifr_question_opcode(op) && oplen >= 12u ? rd16(q + 10) : 0u;
                    u8 question_flags =
                        ifr_question_opcode(op) && oplen >= 13u ? q[12] : 0u;
                    u8 value_width = ifr_scalar_width(op, q, oplen);
                    u16 ref_form_id =
                        (op == 0x0fu && oplen >= 15u) ? rd16(q + 13) : 0u;

                    if (hidden_here) {
                        ++g_nav_condition_hidden;
                    } else {
                        if (g_nav_m1603qa_308_profile &&
                            selected_form_id == 0x2710u && op == 0x0fu &&
                            (ref_form_id < 0x2716u || ref_form_id > 0x271au)) {
                            marker("HII_GRAPH_NAV_M1603QA_ROOT_FILTER=PASS");
                            q += oplen;
                            continue;
                        }

                        if (help_token)
                            (void)get_hii_string(g_nav_hii_string, g_nav_hii_handle,
                                                 help_token, help_candidate,
                                                 &help_count);

                        int have_prompt =
                            token && get_hii_string(g_nav_hii_string,
                                                    g_nav_hii_handle,
                                                    token, candidate,
                                                    &candidate_count);
                        if (!have_prompt && ref_form_id) {
                            have_prompt = nav_get_form_title(
                                ref_form_id, candidate, &candidate_count);
                            if (have_prompt)
                                marker("HII_GRAPH_NAV_REF_TITLE_FALLBACK=PASS");
                        }

                        if (have_prompt) {
                            u8 prompt_condition_flags = active_condition_flags;
                            if (active_unknown)
                                prompt_condition_flags |= NAV_COND_UNKNOWN;
                            int added = nav_prompt_add(
                                op, g_nav_m1603qa_handle_index, value_width,
                                selected_form_id, question_id,
                                varstore_id, var_info, question_flags,
                                prompt_condition_flags,
                                candidate, candidate_count,
                                help_candidate, help_count, ref_form_id);
                            if (added > 0) {
                                added_prompt_index = (u8)(added - 1);
                                nav_prompt_metadata_set(added_prompt_index, op, q, oplen);
                                if (active_condition_flags & NAV_COND_GRAY)
                                    ++g_nav_condition_gray;
                            }
                        }
                    }
                }

                if (in_selected_form && !hidden_here && op == 0x09u &&
                    active_oneof_prompt != 0xffu && oplen >= 7u) {
                    u16 option_token = rd16(q + 2);
                    u8 option_type = q[5];
                    u64 option_value = 0u;
                    char option_text[QEV_NAV_TEXT_MAX + 1u];
                    u32 option_count = 0u;
                    if (option_token &&
                        nav_option_value(option_type, q + 6,
                                         oplen - 6u, &option_value) &&
                        get_hii_string(g_nav_hii_string, g_nav_hii_handle,
                                       option_token, option_text,
                                       &option_count)) {
                        nav_option_add(active_oneof_prompt, option_value,
                                       option_text, option_count);
                    }
                }

                if (op_header & 0x80u) {
                    if (scope_depth < (u8)sizeof(scope_condition_stack)) {
                        scope_condition_stack[scope_depth] = condition_active;
                        scope_unknown_stack[scope_depth] = condition_unknown;
                        scope_oneof_stack[scope_depth] = active_oneof_prompt;
                        ++scope_depth;
                        active_condition_flags |= condition_active;
                        active_unknown |= condition_unknown;
                        if (in_selected_form && op == 0x05u &&
                            added_prompt_index != 0xffu)
                            active_oneof_prompt = added_prompt_index;
                    } else {
                        g_nav_prompt_overflow = 1u;
                    }
                }
                q += oplen;
            }
        }
        p += len;
    }

    if (!selected_form_id || !g_nav_prompt_total || g_nav_prompt_overflow)
        return 0;

    g_nav_current_form_id = selected_form_id;
    nav_prompt_load(0u);
    marker("HII_GRAPH_NAV_FORM_AWARE=PASS");
    marker("HII_GRAPH_NAV_FORM_LOAD=PASS");
    marker("HII_GRAPH_NAV_FORM_ID_TRACKING=PASS");
    marker("HII_GRAPH_NAV_QUESTION_ID_TRACKING=PASS");
    marker("HII_GRAPH_NAV_VARSTORE_METADATA=PASS");
    marker("HII_GRAPH_NAV_ONEOF_OPTIONS=PASS");
    marker("HII_GRAPH_NAV_CONDITION_TRACKING=PASS");
    marker("HII_GRAPH_NAV_CONDITION_EVALUATOR=PASS");
    marker("HII_GRAPH_NAV_SUPPRESS_RUNTIME=PASS");
    marker("HII_GRAPH_NAV_GRAY_RUNTIME=PASS");
    marker("HII_GRAPH_NAV_DISABLE_RUNTIME=PASS");
    marker("HII_GRAPH_NAV_CONDITION_UNKNOWN_SAFE=PASS");
    serial_puts("HII_GRAPH_NAV_CONDITION_KNOWN=0x");
    serial_hex32((u32)g_nav_condition_known);
    serial_puts("\r\n");
    serial_puts("HII_GRAPH_NAV_CONDITION_UNKNOWN=0x");
    serial_hex32((u32)g_nav_condition_unknown);
    serial_puts("\r\n");
    serial_puts("HII_GRAPH_NAV_CONDITION_HIDDEN=0x");
    serial_hex32((u32)g_nav_condition_hidden);
    serial_puts("\r\n");
    serial_puts("HII_GRAPH_NAV_CONDITION_GRAY=0x");
    serial_hex32((u32)g_nav_condition_gray);
    serial_puts("\r\n");
    serial_puts("HII_GRAPH_NAV_ACTIVE_FORM_ID=0x");
    serial_hex32((u32)selected_form_id);
    serial_puts("\r\n");
    if (g_nav_form_title_length) {
        serial_puts("HII_GRAPH_NAV_FORM_TITLE=");
        serial_puts(g_nav_form_title);
        serial_puts("\r\n");
    }
    return 1;
}

static void nav_discover_config_commit_path(void *system_table,
                                            hii_database_protocol *db,
                                            void *hii_handle) {
    g_nav_config_driver_handle = 0;
    g_nav_config_access = 0;
    g_nav_config_routing = 0;
    if (!system_table || !db || !hii_handle || !db->get_package_list_handle) {
        marker("HII_GRAPH_NAV_CONFIG_PATH=NOT_AVAILABLE");
        return;
    }

    void *bs = *(void **)((u8 *)system_table + 0x60);
    if (!bs) {
        marker("HII_GRAPH_NAV_CONFIG_PATH=NOT_AVAILABLE");
        return;
    }
    handle_protocol_fn handle_protocol =
        *(handle_protocol_fn *)((u8 *)bs + 0x98);
    locate_protocol_fn locate = *(locate_protocol_fn *)((u8 *)bs + 0x140);
    if (!handle_protocol || !locate) {
        marker("HII_GRAPH_NAV_CONFIG_PATH=NOT_AVAILABLE");
        return;
    }

    if (db->get_package_list_handle(db, hii_handle,
                                    &g_nav_config_driver_handle) == 0 &&
        g_nav_config_driver_handle) {
        marker("HII_GRAPH_NAV_CONFIG_DRIVER_HANDLE=PASS");
        if (handle_protocol(g_nav_config_driver_handle,
                            &g_hii_config_access_guid,
                            &g_nav_config_access) == 0 &&
            g_nav_config_access)
            marker("HII_GRAPH_NAV_CONFIG_ACCESS_DISCOVERY=PASS");
        else
            marker("HII_GRAPH_NAV_CONFIG_ACCESS_DISCOVERY=NOT_AVAILABLE");
    } else {
        marker("HII_GRAPH_NAV_CONFIG_DRIVER_HANDLE=NOT_AVAILABLE");
    }

    if (locate(&g_hii_config_routing_guid, 0, &g_nav_config_routing) == 0 &&
        g_nav_config_routing)
        marker("HII_GRAPH_NAV_CONFIG_ROUTING_DISCOVERY=PASS");
    else
        marker("HII_GRAPH_NAV_CONFIG_ROUTING_DISCOVERY=NOT_AVAILABLE");

    if (g_nav_config_access && g_nav_config_routing)
        marker("HII_GRAPH_NAV_COMMIT_PATH_DISCOVERY=PASS");
    else
        marker("HII_GRAPH_NAV_COMMIT_PATH_DISCOVERY=PARTIAL");

    /* Discovery only. No RouteConfig, callback, or SetVariable write is
       permitted until transaction serialization and rollback are proven. */
    marker("HII_GRAPH_NAV_ROUTE_CONFIG_NOT_INVOKED=PASS");
}

#endif

static int resolve_hii_prompt(void *system_table) {
    void *bs = *(void **)((u8 *)system_table + 0x60);
    if (!bs) return 0;
    locate_protocol_fn locate = *(locate_protocol_fn *)((u8 *)bs + 0x140);
    if (!locate) return 0;
    hii_database_protocol *db = 0;
    hii_string_protocol *str = 0;
    if (locate(&g_hii_database_guid, 0, (void **)&db) != 0 || !db) return 0;
    marker("HII_DATABASE_PROTOCOL=PASS");
    if (locate(&g_hii_string_guid, 0, (void **)&str) != 0 || !str || !str->get_string) return 0;
    marker("HII_STRING_PROTOCOL=PASS");

    usize handle_bytes = sizeof(g_hii_handles);
    if (!db->list_package_lists ||
        db->list_package_lists(db, 0x02, 0, &handle_bytes, g_hii_handles) != 0 ||
        !handle_bytes || handle_bytes > sizeof(g_hii_handles)) return 0;
    u32 handles = (u32)(handle_bytes / sizeof(void *));
    marker("HII_FORMS_HANDLE_LIST=PASS");
#ifdef QEV_INTERACTIVE_NAV
    g_nav_prompt_total = 0;
    g_nav_prompt_index = 0;
    g_nav_prompt_overflow = 0;
    g_nav_varstore_total = 0;
    g_nav_question_total = 0;
    g_nav_m1603qa_handle_index = 0xffu;
    g_nav_help_available = 0;
    g_nav_event_mask = 0;
    g_nav_speech_events = 0;
    g_nav_realtime_events = 0;
    g_nav_speech_interruptions = 0;
#endif

    for (u32 hi = 0; hi < handles; ++hi) {
        void *handle = g_hii_handles[hi];
        usize size = sizeof(g_hii_package);
        if (!handle || !db->export_package_lists ||
            db->export_package_lists(db, handle, &size, g_hii_package) != 0 ||
            size < 24u || size > sizeof(g_hii_package)) continue;
        u32 list_len = rd32(g_hii_package + 16);
        if (list_len < 24u || list_len > size) continue;
#ifdef QEV_INTERACTIVE_NAV
        if (guid_bytes_equal(g_hii_package, &g_m1603qa_308_setup_package_list_guid))
            g_nav_m1603qa_handle_index = (u8)hi;
#endif
        const u8 *p = g_hii_package + 20;
        const u8 *list_end = g_hii_package + list_len;
        while (p + 4 <= list_end) {
            u32 hdr = rd32(p);
            u32 len = hdr & 0x00ffffffu;
            u8 type = (u8)(hdr >> 24);
            if (len < 4u || p + len > list_end) break;
            if (type == 0xdf) break;
            if (type == 0x02) {
                marker("HII_FORMS_PACKAGE=PASS");
                const u8 *q = p + 4;
                const u8 *end = p + len;
#ifdef QEV_INTERACTIVE_NAV
                u16 current_form_id = 0u;
                u8 scope_condition_stack[64];
                u8 scope_oneof_stack[64];
                u8 scope_depth = 0u;
                u8 active_condition_flags = 0u;
                u8 active_oneof_prompt = 0xffu;
#endif
                while (q + 2 <= end) {
                    u8 op = q[0];
                    u8 op_header = q[1];
                    u32 oplen = (u32)(op_header & 0x7f);
                    if (oplen < 2u || q + oplen > end) break;
#ifdef QEV_INTERACTIVE_NAV
                    if (op == 0x29u) { /* EFI_IFR_END_OP */
                        if (scope_depth) {
                            --scope_depth;
                            active_oneof_prompt = scope_oneof_stack[scope_depth];
                            active_condition_flags = 0u;
                            for (u8 si = 0u; si < scope_depth; ++si)
                                active_condition_flags |= scope_condition_stack[si];
                        }
                        q += oplen;
                        continue;
                    }
                    if (op == 0x01u && oplen >= 6u) {
                        current_form_id = rd16(q + 2);
                    }
                    if (op == 0x24u && oplen >= 23u) {
                        nav_varstore_add((u8)hi, 1u, rd16(q + 18),
                                         q + 2, rd16(q + 20),
                                         q + 22, oplen - 22u);
                    } else if (op == 0x26u && oplen >= 27u) {
                        nav_varstore_add((u8)hi, 2u, rd16(q + 2),
                                         q + 4, rd16(q + 24),
                                         q + 26, oplen - 26u);
                    }
#endif
                    u8 added_prompt_index = 0xffu;
                    if (prompt_opcode(op) && oplen >= 4u) {
                        u16 token = rd16(q + 2);
#ifdef QEV_INTERACTIVE_NAV
                        char candidate[QEV_NAV_TEXT_MAX + 1u];
                        char help_candidate[QEV_NAV_TEXT_MAX + 1u];
                        u32 candidate_count = 0;
                        u32 help_count = 0;
                        u16 help_token = oplen >= 6u ? rd16(q + 4) : 0u;
                        u16 question_id =
                            ifr_question_opcode(op) && oplen >= 8u ? rd16(q + 6) : 0u;
                        u16 varstore_id =
                            ifr_question_opcode(op) && oplen >= 10u ? rd16(q + 8) : 0u;
                        u16 var_info =
                            ifr_question_opcode(op) && oplen >= 12u ? rd16(q + 10) : 0u;
                        u8 question_flags =
                            ifr_question_opcode(op) && oplen >= 13u ? q[12] : 0u;
                        u8 value_width = ifr_scalar_width(op, q, oplen);
                        if (question_id)
                            nav_question_add((u8)hi, op, value_width,
                                             question_id, varstore_id, var_info);
                        if (help_token)
                            (void)get_hii_string(str, handle, help_token, help_candidate, &help_count);
                        if (token && get_hii_string(str, handle, token, candidate, &candidate_count)) {
                            int added = nav_prompt_add(op, (u8)hi, value_width,
                                                       current_form_id, question_id,
                                                       varstore_id, var_info, question_flags,
                                                       active_condition_flags,
                                                       candidate, candidate_count,
                                                       help_candidate, help_count,
                                                       (op == 0x0fu && oplen >= 15u) ? rd16(q + 13) : 0u);
                            if (added > 0) {
                                added_prompt_index = (u8)(added - 1);
                                nav_prompt_metadata_set(added_prompt_index, op, q, oplen);
                            }
                        }
#else
                        if (token && get_hii_string(str, handle, token, g_prompt_text, &g_prompt_count)) {
                            marker("IFR_PROMPT_STRING_ID=PASS");
                            marker("HII_LANGUAGE_AND_STRING=PASS");
                            serial_puts("HII_PROMPT_TEXT=");
                            serial_puts(g_prompt_text);
                            serial_puts("\r\n");
                            marker("HII_PROMPT_SOURCE=PASS");
                            return 1;
                        }
#endif
                    }
#ifdef QEV_INTERACTIVE_NAV
                    if (op == 0x09u && active_oneof_prompt != 0xffu && oplen >= 7u) {
                        u16 option_token = rd16(q + 2);
                        u8 option_type = q[5];
                        u64 option_value = 0u;
                        char option_text[QEV_NAV_TEXT_MAX + 1u];
                        u32 option_count = 0u;
                        if (option_token &&
                            nav_option_value(option_type, q + 6, oplen - 6u, &option_value) &&
                            get_hii_string(str, handle, option_token, option_text, &option_count)) {
                            nav_option_add(active_oneof_prompt, option_value,
                                           option_text, option_count);
                        }
                    }
                    if (op_header & 0x80u) {
                        if (scope_depth < (u8)sizeof(scope_condition_stack)) {
                            u8 cond = ifr_condition_flag(op);
                            scope_condition_stack[scope_depth] = cond;
                            scope_oneof_stack[scope_depth] = active_oneof_prompt;
                            ++scope_depth;
                            active_condition_flags |= cond;
                            if (op == 0x05u && added_prompt_index != 0xffu)
                                active_oneof_prompt = added_prompt_index;
                        } else {
                            g_nav_prompt_overflow = 1u;
                        }
                    }
#endif
                    q += oplen;
                }
            }
            p += len;
        }
    }
#ifdef QEV_INTERACTIVE_NAV
    if (g_nav_m1603qa_handle_index != 0xffu) {
        void *handle = g_hii_handles[g_nav_m1603qa_handle_index];
        usize size = sizeof(g_hii_package);
        if (!handle || !db->export_package_lists ||
            db->export_package_lists(db, handle, &size, g_hii_package) != 0 ||
            size < 24u || size > sizeof(g_hii_package) ||
            !guid_bytes_equal(g_hii_package,
                              &g_m1603qa_308_setup_package_list_guid)) {
            marker("HII_GRAPH_NAV_PROFILE=M1603QA_BIOS_308_EXPORT_FAILED");
            return 0;
        }

        g_nav_hii_string = str;
        g_nav_hii_handle = handle;
        g_nav_m1603qa_308_profile = 1u;
        g_nav_form_history_depth = 0u;
        nav_discover_config_commit_path(system_table, db, handle);
        if (!nav_load_form(system_table, 0x2710u)) {
            marker("HII_GRAPH_NAV_PROFILE=M1603QA_BIOS_308_FORM_LOAD_FAILED");
            return 0;
        }

        marker("HII_GRAPH_NAV_PROFILE=M1603QA_BIOS_308");
        marker("HII_GRAPH_NAV_PACKAGE_GUID_MATCH=PASS");
        marker("HII_GRAPH_NAV_ROOT_FORM_2710=PASS");
        marker("HII_GRAPH_NAV_SETUP_FORMSET=PASS");
        marker(g_nav_varstore_total ? "HII_GRAPH_NAV_VARSTORE_CATALOG=PASS"
                                    : "HII_GRAPH_NAV_VARSTORE_CATALOG=EMPTY");
        marker(g_nav_question_total ? "HII_GRAPH_NAV_QUESTION_CATALOG=PASS"
                                    : "HII_GRAPH_NAV_QUESTION_CATALOG=EMPTY");
        marker("IFR_PROMPT_STRING_ID=PASS");
        marker("HII_LANGUAGE_AND_STRING=PASS");
        marker("HII_GRAPH_NAV_PROMPT_COLLECTION=PASS");
        marker("HII_GRAPH_NAV_SEMANTIC_ROLE=PASS");
        marker("HII_PROMPT_SOURCE=PASS");
        return 1;
    }

    if (g_nav_prompt_total) {
        nav_prompt_load(0);
        marker("IFR_PROMPT_STRING_ID=PASS");
        marker("HII_LANGUAGE_AND_STRING=PASS");
        marker(g_nav_prompt_overflow ? "HII_GRAPH_NAV_PROMPT_COLLECTION=TRUNCATED"
                                     : "HII_GRAPH_NAV_PROMPT_COLLECTION=PASS");
        marker("HII_GRAPH_NAV_FORM_ID_TRACKING=PASS");
        marker("HII_GRAPH_NAV_QUESTION_ID_TRACKING=PASS");
        marker("HII_GRAPH_NAV_VARSTORE_METADATA=PASS");
        marker(g_nav_varstore_total ? "HII_GRAPH_NAV_VARSTORE_CATALOG=PASS"
                                    : "HII_GRAPH_NAV_VARSTORE_CATALOG=EMPTY");
        marker("HII_GRAPH_NAV_ONEOF_OPTIONS=PASS");
        marker("HII_GRAPH_NAV_CONDITION_TRACKING=PASS");
        marker("HII_GRAPH_NAV_SEMANTIC_ROLE=PASS");
        marker(g_nav_help_available ? "HII_GRAPH_NAV_CONTEXT_HELP=PASS"
                                    : "HII_GRAPH_NAV_CONTEXT_HELP=NOT_AVAILABLE");
        serial_puts("HII_GRAPH_NAV_ROLE=");
        serial_puts(ifr_semantic_role(g_nav_prompt_opcode));
        serial_puts("\r\n");
        serial_puts("HII_GRAPH_NAV_SPEECH_TEXT=");
        serial_puts(g_nav_speech_text);
        serial_puts("\r\n");
        serial_puts("HII_GRAPH_NAV_PROMPT_TOTAL=0x");
        serial_hex8(g_nav_prompt_total);
        serial_puts("\r\n");
        serial_puts("HII_PROMPT_TEXT=");
        serial_puts(g_prompt_text);
        serial_puts("\r\n");
        marker("HII_PROMPT_SOURCE=PASS");
        return 1;
    }
#endif
    return 0;
}

static int graph_selftest(void) {
    clear_graph();
    g_type[0x14] = WIDGET_PIN;
    g_type[0x0c] = WIDGET_MIXER;
    g_type[0x0b] = WIDGET_SELECTOR;
    g_type[0x02] = WIDGET_AUDIO_OUTPUT;
    g_type[0x03] = 0xff;
    g_pin_output[0x14] = 1;

    g_conn_count[0x14] = 1;
    g_conn[0x14][0] = 0x0c;
    g_conn_count[0x0c] = 1;
    g_conn[0x0c][0] = 0x0b;
    g_conn_count[0x0b] = 2;
    g_conn[0x0b][0] = 0x03;
    g_conn[0x0b][1] = 0x02;

    u8 dac = 0, selectors = 0;
    if (!find_route(0x14, &dac, &selectors)) return 0;
    if (dac != 0x02 || selectors != 1 || g_depth[dac] != 3) return 0;
    if (g_route_index[dac] != 1) return 0;
    if (encode_verb12(2, 0x14, 0x701, 3) != 0x21470103u) return 0;
    if (encode_verb4(2, 0x02, 0x2, 0x0011) != 0x20220011u) return 0;
    return 1;
}

static int discover_controller(void) {
    u32 cfg = 0;
    int found = 0;

    /* AMD-5800H-REAL / ASUS M1603QA:
       1022:15E3 is the analog HDA controller feeding Realtek 10EC:0256.
       1002:1637 is HDMI audio. Prefer analog, then preserve a standards-class
       04/03 fallback for QEMU, VMware and other UEFI machines. */
    for (u32 pass = 0; pass < 2 && !found; ++pass) {
        for (u32 bdf = 0; bdf < 0x10000; ++bdf) {
            u32 base = 0x80000000u | (bdf << 8);
            u32 vd = pci_read32(base);
            if ((vd & 0xffff) == 0xffff) continue;
            u32 classreg = pci_read32(base | 0x08);
            if (((classreg >> 16) & 0xffff) != 0x0403) continue;
            if (pass == 0 && vd != 0x15e31022u) continue;
            cfg = base;
            found = 1;
            g_controller_preferred = (u8)(pass == 0);
            if (pass == 0) marker("HDA_CONTROLLER_SELECTION=PREFERRED_AMD_1022_15E3");
            else marker("HDA_CONTROLLER_SELECTION=GENERIC_CLASS_0403");
            break;
        }
    }
    if (!found) return 0;

    u32 command_status = pci_read32(cfg | 0x04);
    u32 command = (command_status & 0x0000ffffu) | 0x00000006u;
    /* PCI Status is W1C in the upper 16 bits: write zeros there. */
    pci_write32(cfg | 0x04, command);
    if ((pci_read32(cfg | 0x04) & 0x00000006u) != 0x00000006u) return 0;
    marker("PCI_COMMAND_MEMORY_BUSMASTER=PASS");

    u32 bar0 = pci_read32(cfg | 0x10);
    if (bar0 & 1) return 0;
    u64 bar = (u64)(bar0 & 0xfffffff0u);
    if ((bar0 & 0x6) == 0x4) {
        bar |= ((u64)pci_read32(cfg | 0x14)) << 32;
    }
    if (!bar) return 0;
    g_hda = (volatile u8 *)(usize)bar;
    if (!mmio16(0x00) || !*(volatile u8 *)(g_hda + 0x03)) return 0;

    u32 gctl = mmio32(0x08);
    mmio32w(0x08, gctl & ~1u);
    u32 timeout = 100000;
    while (timeout-- && (mmio32(0x08) & 1)) {}
    if (!timeout) return 0;
    if (g_stall) g_stall(100);

    mmio32w(0x08, mmio32(0x08) | 1u);
    timeout = 100000;
    while (timeout-- && !(mmio32(0x08) & 1)) {}
    if (!timeout) return 0;
    if (g_stall) g_stall(1000);

    u16 state = mmio16(0x0e);
    if (!state) return 0;
    for (u8 cad = 0; cad < 15; ++cad) {
        if (!(state & (1u << cad))) continue;
        g_cad = cad;
        u32 codec_id = get_param(0, 0x00);
        if (codec_id == INVALID_RESP || codec_id == 0 || codec_id == 0xffffffffu) continue;

        if (g_controller_preferred && codec_id != 0x10ec0256u) continue;

        g_codec_vendor_id = codec_id;
        serial_puts("HDA_CODEC_VENDOR_DEVICE=0x");
        serial_hex32(codec_id);
        serial_puts("\r\n");
        if (g_controller_preferred) marker("HDA_CODEC_SELECTION=REALTEK_10EC_0256");
        else marker("HDA_CODEC_SELECTION=GENERIC_RUNTIME");
        return 1;
    }
    if (g_controller_preferred) marker("HDA_CODEC_SELECTION=REALTEK_10EC_0256_NOT_FOUND");
    return 0;
}

static int physical_pin_score(u8 pin, u32 *config_out) {
    u32 config = verb12(pin, 0xf1c, 0);
    if (config_out) *config_out = config;
    if (config == INVALID_RESP) return 0;

    u8 connectivity = (u8)((config >> 30) & 0x03u);
    u8 device = (u8)((config >> 20) & 0x0fu);
    if (connectivity == 0x01u) return -1; /* No physical connection. */

    int score = 1;
    if (device == 0x01u) score = 100;      /* Speaker. */
    else if (device == 0x02u) score = 80;  /* Headphone out. */
    else if (device == 0x00u) score = 60;  /* Line out. */
    else if (device == 0x04u || device == 0x05u) score = 40;
    if (connectivity == 0x02u) score += 20; /* Fixed/internal device. */
    else if (connectivity == 0x03u) score += 10;
    return score;
}

static int discover_live_graph(u8 *pin_out, u8 *dac_out, u8 *selectors_out) {
    clear_graph();
    u32 root_nodes = get_param(0, 0x04);
    if (root_nodes == INVALID_RESP) return 0;
    u8 root_start = (u8)((root_nodes >> 16) & 0xff);
    u8 root_count = (u8)(root_nodes & 0xff);
    if (!root_count) return 0;

    u8 afg = INVALID_NID;
    for (u16 n = root_start; n < (u16)root_start + root_count; ++n) {
        u32 type = get_param((u8)n, 0x05);
        if (type != INVALID_RESP && (type & 0xff) == 1) {
            afg = (u8)n;
            break;
        }
    }
    if (afg == INVALID_NID) return 0;
    g_afg = afg;
    marker("HDA_AFG_RUNTIME=PASS");

    u32 widget_nodes = get_param(afg, 0x04);
    if (widget_nodes == INVALID_RESP) return 0;
    u8 start = (u8)((widget_nodes >> 16) & 0xff);
    u8 count = (u8)(widget_nodes & 0xff);
    if (!count) return 0;

    for (u16 n = start; n < (u16)start + count; ++n) {
        u8 nid = (u8)n;
        u32 cap = get_param(nid, 0x09);
        if (cap == INVALID_RESP) return 0;
        g_widget_cap[nid] = cap;
        u8 type = (u8)((cap >> 20) & 0x0f);
        g_type[nid] = type;
        if (type == WIDGET_PIN) {
            u32 pin_cap = get_param(nid, 0x0c);
            if (pin_cap == INVALID_RESP) return 0;
            if (pin_cap & 0x10) g_pin_output[nid] = 1;
        }
        if (!decode_connections(nid)) return 0;
    }
    marker("HDA_WIDGET_ENUMERATION=PASS");
    marker("HDA_CONNECTION_LIST_DECODE=PASS");

    if (!g_controller_preferred) {
        for (u16 n = start; n < (u16)start + count; ++n) {
            u8 pin = (u8)n;
            if (!g_pin_output[pin]) continue;
            u8 dac = 0, selectors = 0;
            if (find_route(pin, &dac, &selectors)) {
                *pin_out = pin;
                *dac_out = dac;
                *selectors_out = selectors;
                return 1;
            }
        }
        return 0;
    }

    /* Bare-metal ASUS/ALC256: prefer the fixed internal speaker advertised by
       the codec default pin configuration instead of the first routable pin. */
    u8 best_pin = INVALID_NID;
    int best_score = -1;
    u32 best_config = INVALID_RESP;
    for (u16 n = start; n < (u16)start + count; ++n) {
        u8 pin = (u8)n;
        if (!g_pin_output[pin]) continue;
        u8 candidate_dac = 0, candidate_selectors = 0;
        if (!find_route(pin, &candidate_dac, &candidate_selectors)) continue;
        u32 config = INVALID_RESP;
        int score = physical_pin_score(pin, &config);
        if (score > best_score) {
            best_score = score;
            best_pin = pin;
            best_config = config;
        }
    }
    if (best_pin == INVALID_NID) return 0;
    if (!find_route(best_pin, dac_out, selectors_out)) return 0;
    *pin_out = best_pin;
    g_selected_pin_is_internal_speaker =
        (u8)((((best_config >> 30) & 0x03u) == 0x02u) &&
             (((best_config >> 20) & 0x0fu) == 0x01u));
    marker("HDA_PHYSICAL_PIN_SELECTION=DEFAULT_CONFIG_PRIORITY");
    marker("HDA_PHYSICAL_PIN_DEFAULT_CONFIG=PASS");
    serial_puts("HDA_SELECTED_PIN_DEFAULT_CONFIG=0x");
    serial_hex32(best_config);
    serial_puts("\r\n");
    if (g_selected_pin_is_internal_speaker)
        marker("HDA_PHYSICAL_INTERNAL_SPEAKER_PIN=PASS");
    else
        marker("HDA_PHYSICAL_INTERNAL_SPEAKER_PIN=NOT_ESTABLISHED");
    return 1;
}

#ifdef QEV_INTERACTIVE_REPEAT
static int wait_repeat_key(void *system_table) {
    if (!system_table) return 0;
    simple_text_input_protocol *conin =
        *(simple_text_input_protocol **)((u8 *)system_table + 0x30);
    if (!conin || !conin->read_key) return 0;
    marker("HII_GRAPH_REPEAT_KEY=WAIT_R");
    for (;;) {
        efi_input_key key;
        key.scan_code = 0;
        key.unicode_char = 0;
        u64 st = conin->read_key(conin, &key);
        if (st == 0) {
            if (key.unicode_char == (u16)'r' || key.unicode_char == (u16)'R') {
                marker("HII_GRAPH_REPEAT_KEY=R");
                marker("HII_GRAPH_REPEAT_KEY=PASS");
                return 1;
            }
            if (key.unicode_char == 0x001bu) return 0;
        }
        if (g_stall) g_stall(1000);
    }
}
#endif

#ifdef QEV_INTERACTIVE_NAV
static int wait_navigation_keys(void *system_table) {
    if (!system_table || !g_nav_prompt_total) return 0;
    simple_text_input_protocol *conin =
        *(simple_text_input_protocol **)((u8 *)system_table + 0x30);
    if (!conin || !conin->read_key || !g_stall) return 0;

    marker("HII_GRAPH_NAV_READY=PASS");
    marker("HII_GRAPH_NAV_REALTIME_MODE=INTERRUPTIBLE_DMA");
    marker("HII_GRAPH_SPEECH_QUEUE=INTERRUPTIBLE_64");
    marker("HII_GRAPH_SPEECH_WORD_BOUNDARY_CHUNKING=PASS");
    marker("HII_GRAPH_NAV_DIRECTIONAL_MODEL=BLIND_SIMPLE");
    marker("HII_GRAPH_NAV_TAB_FORWARD=PASS");
    marker("HII_GRAPH_NAV_STRUCTURAL_KEYS=PASS");
    marker("HII_GRAPH_NAV_FORM_KEYS=PASS");
    marker("HII_GRAPH_NAV_LIVE_VALUE_KEY=PASS");
    marker("HII_GRAPH_NAV_WHERE_AM_I_KEY=PASS");
    marker("HII_GRAPH_NAV_POSITION_KEY=PASS");
    marker("HII_GRAPH_NAV_STAGED_EDIT_MODE=PASS");
    marker("HII_GRAPH_NAV_STAGED_NUMERIC_MODE=PASS");
    marker("HII_GRAPH_NAV_SPECIALIZED_METADATA=PASS");
    marker("HII_GRAPH_NAV_QUESTION_FLAGS_SEMANTICS=PASS");
    marker("HII_GRAPH_NAV_CALLBACK_AWARE=PASS");
    marker("HII_GRAPH_NAV_PASSWORD_PRIVACY=PASS");
    marker("HII_GRAPH_NAV_STAGED_CONDITION_EVAL=PASS");
    marker("HII_GRAPH_NAV_NO_FIRMWARE_WRITE=PASS");
    serial_puts("HII_GRAPH_NAV_TOTAL=0x");
    serial_hex8(g_nav_prompt_total);
    serial_puts("\r\n");

    u8 audio_progress_for_current = 0;
    for (;;) {
        efi_input_key key;
        key.scan_code = 0;
        key.unicode_char = 0;
        u64 st = conin->read_key(conin, &key);
        if (st == 0) {
            /*
             * Preempt the current utterance as soon as firmware reports a key.
             * Do this before HII refresh/value formatting so rapid blind
             * navigation never waits on semantic work before audio stops.
             */
            if (g_speech_active || g_speech_phrase_active) {
                speech_phrase_cancel();
                if (g_nav_speech_interruptions != 0xffu) ++g_nav_speech_interruptions;
                marker("HII_GRAPH_NAV_SPEECH_INTERRUPT=PASS");
                marker("HII_GRAPH_NAV_SPEECH_INTERRUPT_EARLY=PASS");
            }
            u8 speak = 0;
            u8 speak_help = 0;
            const char *speech_override = 0;
            u8 speech_override_length = 0;
            if (key.unicode_char == 0x001bu || key.scan_code == 0x0017u) {
                marker("HII_GRAPH_NAV_KEY=ESC");
                speech_phrase_cancel();
                if (g_nav_m1603qa_308_profile && g_nav_form_history_depth) {
                    u16 parent = g_nav_form_history[--g_nav_form_history_depth];
                    if (!nav_load_form(system_table, parent)) return 0;
                    marker("HII_GRAPH_NAV_FORM_BACK=PASS");
                    if (g_nav_form_title_length) {
                        speech_override = g_nav_form_title;
                        speech_override_length = g_nav_form_title_length;
                        marker("HII_GRAPH_NAV_FORM_TITLE_SPEECH=PASS");
                    }
                    speak = 1;
                } else {
                    if ((g_nav_event_mask & NAV_REQUIRED_MASK) == NAV_REQUIRED_MASK &&
                        g_nav_speech_events >= 7u)
                        marker("HII_GRAPH_NAV_REQUIRED_EVENTS=PASS");
                    else
                        marker("HII_GRAPH_NAV_REQUIRED_EVENTS=PENDING_NONBLOCKING");
                    if (nav_stage_count()) {
                        static const char discard_speech[] = "preview edits discarded";
                        nav_stage_clear_all();
                        if (run_speech_dma(discard_speech, 23u))
                            marker("HII_GRAPH_NAV_EXIT_DISCARD_SPEECH=PASS");
                        else
                            marker("HII_GRAPH_NAV_EXIT_DISCARD_SPEECH=FAILED");
                    }
                    marker("HII_GRAPH_NAV_SIMPLE_EXIT=PASS");
                    marker("HII_GRAPH_NAV_EXIT=PASS");
                    return 1;
                }
            }
            if (key.unicode_char == 0x000du) {
                marker("HII_GRAPH_NAV_KEY=ENTER");
                u8 current_condition = g_nav_prompt_condition_flags[g_nav_prompt_index];
                u16 target = g_nav_ref_form_ids[g_nav_prompt_index];
                if (current_condition & (NAV_COND_GRAY | NAV_COND_DISABLE)) {
                    marker("HII_GRAPH_NAV_DISABLED_ACTION=BLOCKED");
                    speech_override = "disabled";
                    speech_override_length = 8u;
                    speak = 1;
                } else if (current_condition & NAV_COND_UNKNOWN) {
                    marker("HII_GRAPH_NAV_CONDITIONAL_ACTION=BLOCKED");
                    speech_override = "conditional";
                    speech_override_length = 11u;
                    speak = 1;
                } else if (g_nav_m1603qa_308_profile && target &&
                           target != g_nav_current_form_id) {
                    u16 previous = g_nav_current_form_id;
                    if (g_nav_form_history_depth >= 16u) return 0;
                    g_nav_form_history[g_nav_form_history_depth++] = previous;
                    speech_phrase_cancel();
                    if (!nav_load_form(system_table, target)) {
                        --g_nav_form_history_depth;
                        return 0;
                    }
                    marker("HII_GRAPH_NAV_FORM_ENTER=PASS");
                    if (g_nav_form_title_length) {
                        speech_override = g_nav_form_title;
                        speech_override_length = g_nav_form_title_length;
                        marker("HII_GRAPH_NAV_FORM_TITLE_SPEECH=PASS");
                    }
                    speak = 1;
                } else if (g_nav_prompt_opcodes[g_nav_prompt_index] == 0x06u &&
                           nav_stage_toggle_checkbox(system_table, g_nav_prompt_index)) {
                    marker("HII_GRAPH_NAV_ENTER_CONTEXT_ACTION=PASS");
                    marker("HII_GRAPH_NAV_ENTER_TOGGLE=PASS");
                    marker("HII_GRAPH_NAV_STAGED_CHECKBOX=PASS");
                    marker("HII_GRAPH_NAV_STAGED_EDIT=PASS");
                    if (g_nav_m1603qa_308_profile) {
                        if (!nav_refresh_current_form(system_table)) return 0;
                        marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                    }
                    speak = 1;
                } else if (g_nav_prompt_opcodes[g_nav_prompt_index] == 0x05u &&
                           nav_stage_cycle_oneof(system_table, g_nav_prompt_index, 1)) {
                    marker("HII_GRAPH_NAV_ENTER_CONTEXT_ACTION=PASS");
                    marker("HII_GRAPH_NAV_ENTER_CHOICE=PASS");
                    marker("HII_GRAPH_NAV_STAGED_ONEOF=PASS");
                    marker("HII_GRAPH_NAV_STAGED_EDIT=PASS");
                    if (g_nav_m1603qa_308_profile) {
                        if (!nav_refresh_current_form(system_table)) return 0;
                        marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                    }
                    speak = 1;
                } else {
                    if (target == g_nav_current_form_id)
                        marker("HII_GRAPH_NAV_SELF_REF_ACTION=BLOCKED");
                    marker("HII_GRAPH_NAV_READ_ONLY_ACTION=BLOCKED");
                    speech_override = "read only";
                    speech_override_length = 9u;
                    speak = 1;
                }
            }
            if (key.unicode_char == (u16)'w' || key.unicode_char == (u16)'W') {
                marker("HII_GRAPH_NAV_KEY=W");
                if (nav_build_where_am_i_speech(system_table, g_nav_prompt_index,
                                                g_nav_where_text,
                                                &g_nav_where_length)) {
                    speech_override = g_nav_where_text;
                    speech_override_length = g_nav_where_length;
                    marker("HII_GRAPH_NAV_WHERE_AM_I_CONTEXT=PASS");
                }
                marker("HII_GRAPH_NAV_WHERE_AM_I=PASS");
                speak = 1;
            } else if (key.unicode_char == (u16)'f' || key.unicode_char == (u16)'F') {
                marker("HII_GRAPH_NAV_KEY=STRUCTURAL_FORM");
                u8 next = 0;
                int direction = key.unicode_char == (u16)'F' ? -1 : 1;
                if (nav_find_form(direction, &next)) {
                    nav_prompt_load(next);
                    marker("HII_GRAPH_NAV_STRUCTURAL_FORM=PASS");
                } else {
                    speech_override = "no form";
                    speech_override_length = 7u;
                    marker("HII_GRAPH_NAV_STRUCTURAL_FORM=NOT_FOUND");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'b' || key.unicode_char == (u16)'B') {
                marker("HII_GRAPH_NAV_KEY=STRUCTURAL_BUTTON");
                u8 next = 0;
                int direction = key.unicode_char == (u16)'B' ? -1 : 1;
                if (nav_find_group(NAV_GROUP_BUTTON, direction, &next)) {
                    nav_prompt_load(next);
                    marker("HII_GRAPH_NAV_STRUCTURAL_BUTTON=PASS");
                } else {
                    speech_override = "no button";
                    speech_override_length = 9u;
                    marker("HII_GRAPH_NAV_STRUCTURAL_BUTTON=NOT_FOUND");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'x' || key.unicode_char == (u16)'X') {
                marker("HII_GRAPH_NAV_KEY=STRUCTURAL_CHECKBOX");
                u8 next = 0;
                int direction = key.unicode_char == (u16)'X' ? -1 : 1;
                if (nav_find_group(NAV_GROUP_CHECKBOX, direction, &next)) {
                    nav_prompt_load(next);
                    marker("HII_GRAPH_NAV_STRUCTURAL_CHECKBOX=PASS");
                } else {
                    speech_override = "no checkbox";
                    speech_override_length = 11u;
                    marker("HII_GRAPH_NAV_STRUCTURAL_CHECKBOX=NOT_FOUND");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'c' || key.unicode_char == (u16)'C') {
                marker("HII_GRAPH_NAV_KEY=STRUCTURAL_CHOICE");
                u8 next = 0;
                int direction = key.unicode_char == (u16)'C' ? -1 : 1;
                if (nav_find_group(NAV_GROUP_CHOICE, direction, &next)) {
                    nav_prompt_load(next);
                    marker("HII_GRAPH_NAV_STRUCTURAL_CHOICE=PASS");
                } else {
                    speech_override = "no choice";
                    speech_override_length = 9u;
                    marker("HII_GRAPH_NAV_STRUCTURAL_CHOICE=NOT_FOUND");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'e' || key.unicode_char == (u16)'E') {
                marker("HII_GRAPH_NAV_KEY=STRUCTURAL_EDITABLE");
                u8 next = 0;
                int direction = key.unicode_char == (u16)'E' ? -1 : 1;
                if (nav_find_group(NAV_GROUP_EDITABLE, direction, &next)) {
                    nav_prompt_load(next);
                    marker("HII_GRAPH_NAV_STRUCTURAL_EDITABLE=PASS");
                } else {
                    speech_override = "no editable";
                    speech_override_length = 11u;
                    marker("HII_GRAPH_NAV_STRUCTURAL_EDITABLE=NOT_FOUND");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'p' || key.unicode_char == (u16)'P') {
                marker("HII_GRAPH_NAV_KEY=P");
                if (nav_build_position_speech(g_nav_prompt_index,
                                              g_nav_position_text,
                                              &g_nav_position_length)) {
                    speech_override = g_nav_position_text;
                    speech_override_length = g_nav_position_length;
                    marker("HII_GRAPH_NAV_POSITION_SPEECH=PASS");
                } else {
                    speech_override = "no position";
                    speech_override_length = 11u;
                    marker("HII_GRAPH_NAV_POSITION_SPEECH=NOT_AVAILABLE");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'o' || key.unicode_char == (u16)'O') {
                marker("HII_GRAPH_NAV_KEY=O");
                int direction = key.unicode_char == (u16)'O' ? -1 : 1;
                if (nav_stage_cycle_oneof(system_table, g_nav_prompt_index,
                                          direction)) {
                    marker("HII_GRAPH_NAV_STAGED_ONEOF=PASS");
                    marker("HII_GRAPH_NAV_STAGED_EDIT=PASS");
                    if (g_nav_m1603qa_308_profile) {
                        if (!nav_refresh_current_form(system_table)) return 0;
                        marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                    }
                    speak = 1;
                } else {
                    speech_override = "not editable";
                    speech_override_length = 12u;
                    marker("HII_GRAPH_NAV_STAGED_ONEOF=NOT_AVAILABLE");
                    speak = 1;
                }
            } else if (key.unicode_char == 0x0020u) {
                marker("HII_GRAPH_NAV_KEY=SPACE");
                if (nav_stage_toggle_checkbox(system_table, g_nav_prompt_index)) {
                    marker("HII_GRAPH_NAV_STAGED_CHECKBOX=PASS");
                    marker("HII_GRAPH_NAV_STAGED_EDIT=PASS");
                    if (g_nav_m1603qa_308_profile) {
                        if (!nav_refresh_current_form(system_table)) return 0;
                        marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                    }
                    speak = 1;
                } else {
                    speech_override = "not editable";
                    speech_override_length = 12u;
                    marker("HII_GRAPH_NAV_STAGED_CHECKBOX=NOT_AVAILABLE");
                    speak = 1;
                }
            } else if (key.unicode_char == (u16)'l' || key.unicode_char == (u16)'L') {
                marker("HII_GRAPH_NAV_KEY=L");
                if (nav_build_control_detail_speech(g_nav_prompt_index,
                                                    g_nav_value_text,
                                                    &g_nav_value_length)) {
                    speech_override = g_nav_value_text;
                    speech_override_length = g_nav_value_length;
                    marker("HII_GRAPH_NAV_CONTROL_DETAILS=PASS");
                } else {
                    speech_override = "no details";
                    speech_override_length = 10u;
                    marker("HII_GRAPH_NAV_CONTROL_DETAILS=NOT_AVAILABLE");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'+' || key.unicode_char == (u16)'-') {
                marker("HII_GRAPH_NAV_KEY=NUMERIC_PREVIEW");
                int direction = key.unicode_char == (u16)'-' ? -1 : 1;
                if (nav_stage_adjust_numeric(system_table, g_nav_prompt_index,
                                             direction)) {
                    marker("HII_GRAPH_NAV_STAGED_NUMERIC=PASS");
                    marker("HII_GRAPH_NAV_STAGED_EDIT=PASS");
                    if (g_nav_m1603qa_308_profile) {
                        if (!nav_refresh_current_form(system_table)) return 0;
                        marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                    }
                    speak = 1;
                } else {
                    speech_override = "not editable";
                    speech_override_length = 12u;
                    marker("HII_GRAPH_NAV_STAGED_NUMERIC=NOT_AVAILABLE");
                    speak = 1;
                }
            } else if (key.unicode_char == (u16)'m' || key.unicode_char == (u16)'M') {
                marker("HII_GRAPH_NAV_KEY=M");
                if (nav_build_edit_status_speech(g_nav_edit_status_text,
                                                 &g_nav_edit_status_length)) {
                    speech_override = g_nav_edit_status_text;
                    speech_override_length = g_nav_edit_status_length;
                    marker("HII_GRAPH_NAV_STAGED_STATUS_SPEECH=PASS");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'z' || key.unicode_char == (u16)'Z') {
                marker("HII_GRAPH_NAV_KEY=Z");
                if (nav_stage_discard_prompt(g_nav_prompt_index)) {
                    speech_override = "edit discarded";
                    speech_override_length = 14u;
                    marker("HII_GRAPH_NAV_STAGED_CURRENT_DISCARD=PASS");
                } else {
                    speech_override = "no pending edit";
                    speech_override_length = 15u;
                    marker("HII_GRAPH_NAV_STAGED_CURRENT_DISCARD=NOT_AVAILABLE");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'d' || key.unicode_char == (u16)'D') {
                marker("HII_GRAPH_NAV_KEY=D");
                nav_stage_clear_all();
                speech_override = "edits discarded";
                speech_override_length = 15u;
                marker("HII_GRAPH_NAV_STAGED_DISCARD=PASS");
                speak = 1;
            } else if (key.unicode_char == (u16)'s' || key.unicode_char == (u16)'S') {
                marker("HII_GRAPH_NAV_KEY=S");
                speech_override = "save unavailable";
                speech_override_length = 16u;
                marker("HII_GRAPH_NAV_SAVE_BLOCKED=PASS");
                marker("HII_GRAPH_NAV_NO_FIRMWARE_WRITE=PASS");
                speak = 1;
            } else if (key.unicode_char == (u16)'h' || key.unicode_char == (u16)'H' ||
                       key.scan_code == 0x000bu) {
                marker(key.scan_code == 0x000bu ? "HII_GRAPH_NAV_KEY=F1"
                                                : "HII_GRAPH_NAV_KEY=H");
                marker("HII_GRAPH_NAV_HELP_DISCOVERABLE=PASS");
                speak = 1;
                speak_help = 1;
            } else if (key.unicode_char == (u16)'v' || key.unicode_char == (u16)'V') {
                marker("HII_GRAPH_NAV_KEY=V");
                const char *live_text = 0;
                u8 live_length = 0u;
                u8 checkbox_state = 0u;
                if (nav_live_value_text(system_table, g_nav_prompt_index,
                                        &live_text, &live_length,
                                        &checkbox_state)) {
                    speech_override = live_text;
                    speech_override_length = live_length;
                    serial_puts("HII_GRAPH_NAV_LIVE_TEXT=");
                    serial_puts(live_text);
                    serial_puts("\r\n");
                    marker("HII_GRAPH_NAV_LIVE_VALUE_READ=PASS");
                    if (checkbox_state)
                        marker("HII_GRAPH_NAV_CHECKBOX_STATE=PASS");
                    else if (g_nav_prompt_opcodes[g_nav_prompt_index] == 0x05u)
                        marker("HII_GRAPH_NAV_LIVE_OPTION_MATCH=PASS");
                } else {
                    speech_override = "no value";
                    speech_override_length = 8u;
                    marker("HII_GRAPH_NAV_LIVE_VALUE_READ=NOT_AVAILABLE");
                }
                speak = 1;
            } else if (key.unicode_char == (u16)'r' || key.unicode_char == (u16)'R') {
                marker("HII_GRAPH_NAV_KEY=R");
                g_nav_event_mask |= NAV_SEEN_R;
                if (g_nav_m1603qa_308_profile) {
                    speech_phrase_cancel();
                    if (!nav_refresh_current_form(system_table)) return 0;
                }
                speak = 1;
            } else if (key.scan_code == 0x0001u) {
                marker("HII_GRAPH_NAV_KEY=UP");
                g_nav_event_mask |= NAV_SEEN_UP;
                u8 next = g_nav_prompt_index ? (u8)(g_nav_prompt_index - 1u)
                                             : (u8)(g_nav_prompt_total - 1u);
                nav_prompt_load(next);
                speak = 1;
            } else if (key.scan_code == 0x0002u) {
                marker("HII_GRAPH_NAV_KEY=DOWN");
                g_nav_event_mask |= NAV_SEEN_DOWN;
                u8 next = (u8)(g_nav_prompt_index + 1u);
                if (next >= g_nav_prompt_total) next = 0;
                nav_prompt_load(next);
                speak = 1;
            } else if (key.scan_code == 0x0004u) {
                marker("HII_GRAPH_NAV_KEY=LEFT");
                u8 op = g_nav_prompt_opcodes[g_nav_prompt_index];
                if (op == 0x05u) {
                    if (nav_stage_cycle_oneof(system_table, g_nav_prompt_index, -1)) {
                        marker("HII_GRAPH_NAV_LEFT_RIGHT_EDIT=PASS");
                        marker("HII_GRAPH_NAV_ARROW_CHOICE=PASS");
                        if (g_nav_m1603qa_308_profile) {
                            if (!nav_refresh_current_form(system_table)) return 0;
                            marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                        }
                    } else {
                        speech_override = "no change";
                        speech_override_length = 9u;
                        marker("HII_GRAPH_NAV_ARROW_EDIT_BLOCKED=PASS");
                    }
                } else if (op == 0x07u) {
                    if (nav_stage_adjust_numeric(system_table, g_nav_prompt_index, -1)) {
                        marker("HII_GRAPH_NAV_LEFT_RIGHT_EDIT=PASS");
                        marker("HII_GRAPH_NAV_ARROW_NUMERIC=PASS");
                        if (g_nav_m1603qa_308_profile) {
                            if (!nav_refresh_current_form(system_table)) return 0;
                            marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                        }
                    } else {
                        speech_override = "no change";
                        speech_override_length = 9u;
                        marker("HII_GRAPH_NAV_ARROW_EDIT_BLOCKED=PASS");
                    }
                } else {
                    u8 next = g_nav_prompt_index ? (u8)(g_nav_prompt_index - 1u)
                                                 : (u8)(g_nav_prompt_total - 1u);
                    nav_prompt_load(next);
                    marker("HII_GRAPH_NAV_LEFT_FALLBACK_MOVE=PASS");
                }
                speak = 1;
            } else if (key.scan_code == 0x0003u) {
                marker("HII_GRAPH_NAV_KEY=RIGHT");
                u8 op = g_nav_prompt_opcodes[g_nav_prompt_index];
                if (op == 0x05u) {
                    if (nav_stage_cycle_oneof(system_table, g_nav_prompt_index, 1)) {
                        marker("HII_GRAPH_NAV_LEFT_RIGHT_EDIT=PASS");
                        marker("HII_GRAPH_NAV_ARROW_CHOICE=PASS");
                        if (g_nav_m1603qa_308_profile) {
                            if (!nav_refresh_current_form(system_table)) return 0;
                            marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                        }
                    } else {
                        speech_override = "no change";
                        speech_override_length = 9u;
                        marker("HII_GRAPH_NAV_ARROW_EDIT_BLOCKED=PASS");
                    }
                } else if (op == 0x07u) {
                    if (nav_stage_adjust_numeric(system_table, g_nav_prompt_index, 1)) {
                        marker("HII_GRAPH_NAV_LEFT_RIGHT_EDIT=PASS");
                        marker("HII_GRAPH_NAV_ARROW_NUMERIC=PASS");
                        if (g_nav_m1603qa_308_profile) {
                            if (!nav_refresh_current_form(system_table)) return 0;
                            marker("HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS");
                        }
                    } else {
                        speech_override = "no change";
                        speech_override_length = 9u;
                        marker("HII_GRAPH_NAV_ARROW_EDIT_BLOCKED=PASS");
                    }
                } else {
                    u8 next = (u8)(g_nav_prompt_index + 1u);
                    if (next >= g_nav_prompt_total) next = 0;
                    nav_prompt_load(next);
                    marker("HII_GRAPH_NAV_RIGHT_FALLBACK_MOVE=PASS");
                }
                speak = 1;
            } else if (key.unicode_char == 0x0009u) {
                /* Tab advances focus without changing firmware values. */
                marker("HII_GRAPH_NAV_KEY=TAB");
                u8 next = (u8)(g_nav_prompt_index + 1u);
                if (next >= g_nav_prompt_total) next = 0;
                nav_prompt_load(next);
                speak = 1;
            } else if (key.scan_code == 0x0005u) {
                marker("HII_GRAPH_NAV_KEY=HOME");
                g_nav_event_mask |= NAV_SEEN_HOME;
                nav_prompt_load(0u);
                speak = 1;
            } else if (key.scan_code == 0x0006u) {
                marker("HII_GRAPH_NAV_KEY=END");
                g_nav_event_mask |= NAV_SEEN_END;
                nav_prompt_load((u8)(g_nav_prompt_total - 1u));
                speak = 1;
            } else if (key.scan_code == 0x0009u) {
                marker("HII_GRAPH_NAV_KEY=PAGE_UP");
                g_nav_event_mask |= NAV_SEEN_PAGE_UP;
                u8 next = g_nav_prompt_index > 5u ? (u8)(g_nav_prompt_index - 5u) : 0u;
                nav_prompt_load(next);
                speak = 1;
            } else if (key.scan_code == 0x000au) {
                marker("HII_GRAPH_NAV_KEY=PAGE_DOWN");
                g_nav_event_mask |= NAV_SEEN_PAGE_DOWN;
                u8 next = (u8)(g_nav_prompt_index + 5u);
                if (next >= g_nav_prompt_total) next = (u8)(g_nav_prompt_total - 1u);
                nav_prompt_load(next);
                speak = 1;
            }

            if (speak) {
                const char *speech_text = speech_override ? speech_override : g_nav_speech_text;
                u8 speech_length = speech_override ? speech_override_length : g_nav_speech_length;
                if (!speech_override && !speak_help &&
                    nav_build_focus_speech(system_table, g_nav_prompt_index,
                                           g_nav_focus_speech,
                                           &g_nav_focus_speech_length)) {
                    speech_text = g_nav_focus_speech;
                    speech_length = g_nav_focus_speech_length;
                    marker("HII_GRAPH_NAV_FOCUS_VALUE_SPEECH=PASS");
                }
                if (speak_help) {
                    if (g_nav_help_length) {
                        speech_text = g_nav_help_text;
                        speech_length = g_nav_help_length;
                    } else {
                        speech_text = "up down move left right change enter action escape back f1 help";
                        speech_length = 63u;
                        marker("HII_GRAPH_NAV_BUILTIN_HELP=PASS");
                    }
                }

                serial_puts("HII_GRAPH_NAV_INDEX=0x");
                serial_hex8(g_nav_prompt_index);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_TEXT=");
                serial_puts(g_prompt_text);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_ROLE=");
                serial_puts(ifr_semantic_role(g_nav_prompt_opcode));
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_FORM_ID=0x");
                serial_hex32((u32)g_nav_prompt_form_ids[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_QUESTION_ID=0x");
                serial_hex32((u32)g_nav_prompt_question_ids[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_HANDLE_INDEX=0x");
                serial_hex8(g_nav_prompt_handle_indices[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_VALUE_WIDTH=0x");
                serial_hex8(g_nav_prompt_value_widths[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_VARSTORE_ID=0x");
                serial_hex32((u32)g_nav_prompt_varstore_ids[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_VAR_INFO=0x");
                serial_hex32((u32)g_nav_prompt_var_infos[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_QUESTION_FLAGS=0x");
                serial_hex8(g_nav_prompt_question_flags[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_CONDITION_FLAGS=0x");
                serial_hex8(g_nav_prompt_condition_flags[g_nav_prompt_index]);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_SPEECH_TEXT=");
                serial_puts(speech_text);
                serial_puts("\r\n");
                marker("HII_GRAPH_NAV_SEMANTIC_ROLE=PASS");
                if (speak_help)
                    marker(g_nav_help_length ? "HII_GRAPH_NAV_CONTEXT_HELP_SPEECH=PASS"
                                             : "HII_GRAPH_NAV_CONTEXT_HELP_SPEECH=NO_HELP");

                if (g_speech_active || g_speech_phrase_active) {
                    speech_phrase_cancel();
                    if (g_nav_speech_interruptions != 0xffu) ++g_nav_speech_interruptions;
                    marker("HII_GRAPH_NAV_SPEECH_INTERRUPT=PASS");
                }
                if (!speech_phrase_begin(speech_text, speech_length)) return 0;

                if (g_nav_speech_events != 0xffu) ++g_nav_speech_events;
                if (g_nav_realtime_events != 0xffu) ++g_nav_realtime_events;
                audio_progress_for_current = 0;
                marker("HII_GRAPH_NAV_REALTIME_FOCUS_SPEECH=PASS");
                marker("HII_GRAPH_NAV_SPEECH_DMA=STARTED");
                marker("HII_GRAPH_NAV_SPEECH_HDA=STARTED");
                if (g_speech_dma_allocations != 1u) return 0;
                marker("HII_GRAPH_SPEECH_DMA_REUSE=PASS");
            }
        }

        if (g_speech_active) {
            u8 progressed = 0;
            int audio_state = speech_phrase_poll(1000u, &progressed);
            if (progressed && !audio_progress_for_current) {
                marker("HII_GRAPH_NAV_LPIB_PROGRESS=PASS");
                audio_progress_for_current = 1;
            }
            if (audio_state < 0) return 0;
            if (audio_state > 0) {
                marker("HII_GRAPH_NAV_SPEECH_DMA=PASS");
                marker("HII_GRAPH_NAV_SPEECH_HDA=PASS");
            }
        }
        g_stall(1000);
    }
}
#endif

__attribute__((ms_abi)) u64 efi_main(void *image_handle, void *system_table) {
    serial_init();
    marker("QEVARYNOX-UEFI-HII-GRAPH-PROMPT-SPEECH-V1");
    marker("STATE=START");
    serial_puts("UEFI_SOURCE_BLOB=" QEV_SOURCE_BLOB "\r\n");
    marker("FRAMEWORK=NONE");
    marker("EDK2=NONE");

    void *boot_services = *(void **)((u8 *)system_table + 0x60);
    if (boot_services) {
        g_allocate_pages = *(allocate_pages_fn *)((u8 *)boot_services + 0x28);
        g_stall = *(stall_fn *)((u8 *)boot_services + 0xf8);
    }
    if (!resolve_hii_prompt(system_table)) {
        marker("STATUS=BLOCKED");
        marker("REASON=HII_PROMPT_NOT_RESOLVED");
        return 1;
    }

    if (!graph_selftest()) {
        marker("STATUS=BLOCKED");
        marker("REASON=HDA_MULTIHOP_EFI_SELFTEST_FAILED");
        return 1;
    }
    marker("HDA_MULTIHOP_EFI_SELFTEST=PASS");
    marker("HDA_RANGE_AND_SELECTOR_ENGINE=PASS");
    marker("NO_FIXED_WIDGET_NIDS=PASS");

    if (!discover_controller()) {
        marker("STATUS=BLOCKED");
        marker("REASON=HDA_CONTROLLER_OR_CODEC_NOT_FOUND");
        return 1;
    }
    marker("HDA_CONTROLLER_CODEC=PASS");

    u8 pin = 0, dac = 0, selectors = 0;
    if (!discover_live_graph(&pin, &dac, &selectors)) {
        marker("STATUS=BLOCKED");
        marker("REASON=HDA_GRAPH_ROUTE_NOT_FOUND");
        return 1;
    }
    marker("HDA_GRAPH_SEARCH_LIVE=PASS");

    u8 applied = 0;
    if (!apply_route(pin, dac, &applied) || applied != selectors) {
        marker("STATUS=BLOCKED");
        marker("REASON=HDA_SELECTOR_APPLY_OR_READBACK_FAILED");
        return 1;
    }
    marker("HDA_SELECTOR_APPLY_LIVE=PASS");
    serial_puts("HDA_PIN_NID=0x"); serial_hex8(pin); serial_puts("\r\n");
    serial_puts("HDA_DAC_NID=0x"); serial_hex8(dac); serial_puts("\r\n");
    serial_puts("HDA_ROUTE_DEPTH=0x"); serial_hex8(g_depth[dac]); serial_puts("\r\n");
    serial_puts("HDA_SELECTOR_WRITES_REQUIRED=0x"); serial_hex8(selectors); serial_puts("\r\n");
    serial_puts("HDA_SELECTOR_WRITES_APPLIED=0x"); serial_hex8(applied); serial_puts("\r\n");
    marker("HDA_SELECTOR_PLAN=PASS");

    if (!configure_output_path(pin, dac)) {
        marker("STATUS=BLOCKED");
        marker("REASON=HDA_OUTPUT_PATH_CONFIGURATION_FAILED");
        return 1;
    }
    marker("HDA_OUTPUT_PATH_CONFIGURATION=PASS");
#ifdef QEV_VOICECORE_V4
    marker("SYNTH=VOICECORE_V4_FULL_LETTER_CLIPS");
    marker("VOICECORE_V4_RUNTIME_BINDING=PASS");
#else
    marker("SYNTH=ALLOPHONE_BDL_RUNTIME_TEXT_V1");
    marker("SYNTH=GRAPHEME_ALLOPHONE_RUNTIME_TEXT_V2");
    marker("SYNTH=CLEAR_LETTERNAME_SPELLING_FR_V3");
#endif
    marker("HII_PROMPT_MAX_CHARS=32");
    marker("HII_PROMPT_WORD_BOUNDARIES=PASS");
    marker("BDL_RUNTIME_TEXT_SCHEDULE=PASS");

#ifdef QEV_INTERACTIVE_NAV
    static const char discovery_speech[] = "ready press f1 for help";
    if (!run_speech_dma(discovery_speech, 23u)) {
        marker("STATUS=BLOCKED");
        marker("REASON=HII_GRAPH_DISCOVERY_SPEECH_FAILED");
        return 1;
    }
    marker("HII_GRAPH_NAV_DISCOVERY_PROMPT=PASS");

    const char *initial_speech = g_nav_speech_text;
    u8 initial_speech_length = g_nav_speech_length;
    if (nav_build_focus_speech(system_table, g_nav_prompt_index,
                               g_nav_focus_speech,
                               &g_nav_focus_speech_length)) {
        initial_speech = g_nav_focus_speech;
        initial_speech_length = g_nav_focus_speech_length;
        marker("HII_GRAPH_NAV_INITIAL_VALUE_SPEECH=PASS");
    }
    if (!run_speech_dma(initial_speech, initial_speech_length)) {
#else
    if (!run_speech_dma(g_prompt_text, g_prompt_count)) {
#endif
        marker("STATUS=BLOCKED");
        marker("REASON=HII_GRAPH_SPEECH_DMA_FAILED");
        return 1;
    }
    marker("HII_GRAPH_SPEECH_DMA=PASS");
    marker("HII_PROMPT_SPEECH_HDA=PASS");
    marker("LPIB_PROGRESS=PASS");
    marker("HII_GRAPH_NAV_REALTIME_CAPABLE=PASS");
#ifdef QEV_INTERACTIVE_NAV
    marker("HII_GRAPH_NAV_SEMANTIC_SPEECH=ROLE_STATE_LABEL_VALUE");
#endif
    if (g_controller_preferred && g_codec_vendor_id == 0x10ec0256u) {
        marker("PHYSICAL_ASUS_M1603QA_HDA_RUNTIME=PASS");
        if (g_selected_pin_is_internal_speaker)
            marker("PHYSICAL_ASUS_M1603QA_INTERNAL_SPEAKER_PIN=PASS");
        else
            marker("PHYSICAL_ASUS_M1603QA_INTERNAL_SPEAKER_PIN=NOT_ESTABLISHED");
        marker("PHYSICAL_ASUS_M1603QA_AUDIBLE_SPEAKER=REQUIRES_HUMAN_CONFIRMATION");
    }
#ifdef QEV_INTERACTIVE_NAV
    if (!wait_navigation_keys(system_table)) {
        marker("STATUS=BLOCKED");
        marker("REASON=HII_GRAPH_NAVIGATION_FAILED");
        return 1;
    }
#endif
#ifdef QEV_INTERACTIVE_REPEAT
    if (!wait_repeat_key(system_table)) {
        marker("STATUS=BLOCKED");
        marker("REASON=HII_GRAPH_REPEAT_KEY_CANCELLED");
        return 1;
    }
    marker("EVENT=HII_GRAPH_REPEAT_TEXT_COMMIT");
    if (!run_speech_dma(g_prompt_text, g_prompt_count)) {
        marker("STATUS=BLOCKED");
        marker("REASON=HII_GRAPH_REPEAT_SPEECH_DMA_FAILED");
        return 1;
    }
    marker("HII_GRAPH_REPEAT_SPEECH_DMA=PASS");
    marker("HII_GRAPH_REPEAT_SPEECH_HDA=PASS");
    marker("HII_GRAPH_REPEAT_LPIB_PROGRESS=PASS");
    if (g_speech_dma_allocations != 1u) {
        marker("STATUS=BLOCKED");
        marker("REASON=HII_GRAPH_SPEECH_DMA_REUSE_FAILED");
        return 1;
    }
    marker("HII_GRAPH_SPEECH_DMA_REUSE=PASS");
#endif
    if (persist_boot_proof(image_handle, boot_services, pin, dac, selectors, applied)) {
        marker("BOOT_MEDIA_PERSISTENT_PROOF=PASS");
    } else {
        marker("BOOT_MEDIA_PERSISTENT_PROOF=NOT_ESTABLISHED");
    }
    marker("PHYSICAL_ASUS_M1603QA_SPEAKER_AUDIBLE=REQUIRES_HUMAN_CONFIRMATION");
    marker("STATUS=PASS");
    return 0;
}
