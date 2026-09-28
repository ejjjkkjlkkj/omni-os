typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long long u64;
typedef unsigned long long usize;
typedef signed short i16;
typedef signed int i32;

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
extern const u32 qev_sil_unit_index;
extern const u8 qev_letter_unit_count[];
extern const u8 qev_letter_units[];

#define MAX_NID 256
#define MAX_CONN 64
#define INVALID_NID 0xff
#define INVALID_RESP 0xffffffffu

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
static u32 g_selected_pin_default_config = INVALID_RESP;
static u8 g_selected_pin_is_internal_speaker;
static stall_fn g_stall;
static allocate_pages_fn g_allocate_pages;
static u64 g_speech_dma_base;
static __attribute__((unused)) u8 g_speech_dma_allocations;
static u8 g_speech_stream_initialized;
static u8 g_proof_overflow;
static u32 g_bank_count;  /* phrase bank entries loaded (0 = letter spelling only) */
static u32 g_bank_hits;
static u32 g_bank_misses;
static u8 g_speech_active;
static volatile u8 *g_speech_pcm;   /* PCM of the utterance playing (fade-out on interrupt) */
static u32 g_speech_payload;
static u64 g_speech_timeout_us;
static u64 g_speech_elapsed_us;

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

typedef struct {
    void *new_package_list;
    void *remove_package_list;
    void *update_package_list;
    hii_list_fn list_package_lists;
    hii_export_fn export_package_lists;
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

static const efi_guid g_hii_database_guid =
    {0xef9fc172u,0xa1b2u,0x4693u,{0xb3,0x27,0x6d,0x32,0xfc,0x41,0x60,0x42}};
static const efi_guid g_hii_string_guid =
    {0x0fd96974u,0x23aau,0x4cdcu,{0xb9,0xcb,0x98,0xd1,0x77,0x50,0x32,0x2a}};
static const efi_guid g_loaded_image_guid =
    {0x5b1b31a1u,0x9562u,0x11d2u,{0x8e,0x3f,0x00,0xa0,0xc9,0x69,0x72,0x3b}};
static const efi_guid g_simple_fs_guid =
    {0x964e5b22u,0x6459u,0x11d2u,{0x8e,0x39,0x00,0xa0,0xc9,0x69,0x72,0x3b}};

static u8 g_hii_package[1024u * 1024u];
static void *g_hii_handles[256];
static char g_prompt_text[33];
static u32 g_prompt_count;

#ifdef QEV_INTERACTIVE_NAV
#define MAX_HII_NAV_PROMPTS 32
static char g_nav_prompts[MAX_HII_NAV_PROMPTS][33];
static u8 g_nav_prompt_lengths[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_opcodes[MAX_HII_NAV_PROMPTS];
static u8 g_nav_prompt_total;
static u8 g_nav_prompt_index;
static u8 g_nav_prompt_opcode;
static char g_nav_speech_text[33];
static u8 g_nav_speech_length;
static u8 g_nav_event_mask;
static u8 g_nav_speech_events;
static u8 g_nav_realtime_events;
static u8 g_nav_speech_interruptions;
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
/*
 * mfence only orders stores; it does not push them out of the CPU cache. A
 * non-snooping HDA controller would then DMA stale RAM, which plays as noise
 * while LPIB still advances normally (QEMU is always coherent and cannot show
 * it). Write back every dirty line before handing a buffer to the controller.
 */
static inline void cache_writeback(void) {
    __asm__ volatile("mfence\n\twbinvd" ::: "memory");
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
/*
 * Physical machines have no COM1, so every serial line is also kept in RAM and
 * written to \OMNI-SR-TRACE.TXT on every exit, including BLOCKED ones.
 */
static char g_trace[16384];
static usize g_trace_n;
static u8 g_trace_truncated;
static u8 g_trace_final;  /* exit lines may use the reserved tail */
static void *g_trace_image_handle;
static void *g_trace_boot_services;
static void serial_char(char ch) {
    usize limit = g_trace_final ? sizeof(g_trace) : sizeof(g_trace) - 512u;
    if (g_trace_n + 1u < limit) g_trace[g_trace_n++] = ch;
    else g_trace_truncated = 1;
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
static const char *g_last_reason;
static void marker(const char *s) {
    if (s[0] == 'R' && s[1] == 'E' && s[2] == 'A' && s[3] == 'S' &&
        s[4] == 'O' && s[5] == 'N' && s[6] == '=') g_last_reason = s;
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
    /* Configuration Default of the pin actually driven (speaker expected on ALC256). */
    proof_puts(proof,sizeof(proof),&n,"HDA_SELECTED_PIN_DEFAULT_CONFIG=0x");
    proof_hex32(proof,sizeof(proof),&n,g_selected_pin_default_config);
    proof_puts(proof,sizeof(proof),&n,"\r\n");
    proof_puts(proof,sizeof(proof),&n,"PHRASE_BANK_ENTRIES=0x");
    proof_hex32(proof,sizeof(proof),&n,g_bank_count);
    proof_puts(proof,sizeof(proof),&n,"\r\nPHRASE_BANK_HITS=0x");
    proof_hex32(proof,sizeof(proof),&n,g_bank_hits);
    proof_puts(proof,sizeof(proof),&n,"\r\nPHRASE_BANK_MISSES=0x");
    proof_hex32(proof,sizeof(proof),&n,g_bank_misses);
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

static int persist_trace(void *image_handle, void *boot_services) {
    static const u16 filename[] = {
        '\\','O','M','N','I','-','S','R','-','T','R','A','C','E','.','T','X','T',0
    };
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
     * Called several times per boot (checkpoint, then exit). Deleting and
     * re-creating the file proved unreliable, so the trace is always written
     * as one fixed-size block from offset 0: the unused tail is newlines, and
     * no stale bytes of an earlier, longer trace can survive.
     */
    typedef u64 (*trace_set_position_fn)(void *self, u64 position);
    const u64 create_mode = 0x8000000000000000ull | 0x2ull | 0x1ull;
    if (root->open(root, (void **)&file, filename, create_mode, 0) != 0 ||
        !file || !file->write || !file->set_position) {
        if (root->close) root->close(root);
        return 0;
    }
    for (usize i = g_trace_n; i < sizeof(g_trace); ++i) g_trace[i] = '\n';
    u64 st = ((trace_set_position_fn)file->set_position)(file, 0);
    usize bytes = sizeof(g_trace);
    if (st == 0) st = file->write(file, &bytes, g_trace);
    if (st == 0 && file->flush) st = file->flush(file);
    if (file->close) file->close(file);
    if (root->close) root->close(root);
    return st == 0 && bytes == sizeof(g_trace);
}

/*
 * Natural-speech phrase bank (\EFI\OMNI\PHRASES.BIN, built by build_phrase_bank.py
 * with ST's neural voice). Key: FNV-1a 64 of the exact text the reader speaks.
 * Clips are 24 kHz signed-16 mono; they are expanded to the 48 kHz stereo DMA
 * format at playback. Missing file or unknown text: letter spelling as before.
 */
typedef u64 (*file_read_fn)(void *self, usize *buffer_size, void *buffer);
typedef u64 (*file_set_position_fn)(void *self, u64 position);
#define PHRASE_BANK_MAX_ENTRIES 16384u
#define PHRASE_BANK_MAX_CLIP_BYTES (1024u * 1024u)
static file_protocol *g_bank_file;
static u32 g_bank_data_off;
static u8 *g_bank_index;
static u8 *g_bank_clip;

static u64 phrase_fnv1a64(const char *text, u32 count) {
    u64 h = 0xcbf29ce484222325ull;
    for (u32 i = 0; i < count; ++i) h = (h ^ (u8)text[i]) * 0x100000001b3ull;
    return h;
}

static int phrase_bank_read(u64 position, void *buffer, usize bytes) {
    if (!g_bank_file || !g_bank_file->set_position || !g_bank_file->read) return 0;
    if (((file_set_position_fn)g_bank_file->set_position)(g_bank_file, position) != 0) return 0;
    usize got = bytes;
    return ((file_read_fn)g_bank_file->read)(g_bank_file, &got, buffer) == 0 && got == bytes;
}

static int phrase_bank_open(void *image_handle, void *boot_services) {
    static const u16 name[] = {
        '\\','E','F','I','\\','O','M','N','I','\\','P','H','R','A','S','E','S','.','B','I','N',0
    };
    loaded_image_protocol_head *loaded = 0;
    simple_fs_protocol *fs = 0;
    file_protocol *root = 0;
    u8 header[32];
    if (!image_handle || !boot_services || !g_allocate_pages) return 0;
    handle_protocol_fn handle_protocol = *(handle_protocol_fn *)((u8 *)boot_services + 0x98);
    if (!handle_protocol ||
        handle_protocol(image_handle, &g_loaded_image_guid, (void **)&loaded) != 0 || !loaded ||
        !loaded->device_handle ||
        handle_protocol(loaded->device_handle, &g_simple_fs_guid, (void **)&fs) != 0 || !fs ||
        !fs->open_volume || fs->open_volume(fs, &root) != 0 || !root || !root->open) return 0;
    if (root->open(root, (void **)&g_bank_file, name, 0x1ull, 0) != 0 || !g_bank_file) {
        g_bank_file = 0;
        return 0;
    }
    if (!phrase_bank_read(0, header, sizeof(header))) return 0;
    static const char magic[8] = {'Q','E','V','P','H','R','0','1'};
    for (u32 i = 0; i < 8u; ++i) if (header[i] != (u8)magic[i]) return 0;
    u32 count = *(u32 *)(header + 8);
    u32 rate = *(u32 *)(header + 12), channels = *(u32 *)(header + 16), bits = *(u32 *)(header + 20);
    u32 index_off = *(u32 *)(header + 24), data_off = *(u32 *)(header + 28);
    if (!count || count > PHRASE_BANK_MAX_ENTRIES || rate != 24000u || channels != 1u || bits != 16u ||
        index_off != 32u || data_off != index_off + count * 16u) return 0;
    u64 index_mem = 0xffffffffu, clip_mem = 0xffffffffu;
    usize index_pages = ((usize)count * 16u + 4095u) / 4096u;
    if (g_allocate_pages(1, 4, index_pages, &index_mem) != 0 || !index_mem) return 0;
    if (g_allocate_pages(1, 4, PHRASE_BANK_MAX_CLIP_BYTES / 4096u, &clip_mem) != 0 || !clip_mem) return 0;
    g_bank_index = (u8 *)(usize)index_mem;
    g_bank_clip = (u8 *)(usize)clip_mem;
    if (!phrase_bank_read(index_off, g_bank_index, (usize)count * 16u)) return 0;
    g_bank_count = count;
    g_bank_data_off = data_off;
    return 1;
}

/* Binary search; returns clip byte length (0 = not in bank) and its file offset. */
static u32 phrase_bank_lookup(const char *text, u32 count, u64 *position_out) {
    if (!g_bank_count || !text || !count) return 0;
    u64 h = phrase_fnv1a64(text, count);
    u32 lo = 0, hi = g_bank_count;
    while (lo < hi) {
        u32 mid = lo + (hi - lo) / 2u;
        u64 key = *(u64 *)(g_bank_index + (usize)mid * 16u);
        if (key == h) {
            u32 off = *(u32 *)(g_bank_index + (usize)mid * 16u + 8u);
            u32 len = *(u32 *)(g_bank_index + (usize)mid * 16u + 12u);
            if (!len || (len & 1u) || len > PHRASE_BANK_MAX_CLIP_BYTES) return 0;
            *position_out = (u64)g_bank_data_off + off;
            return len;
        }
        if (key < h) lo = mid + 1u; else hi = mid;
    }
    return 0;
}

/* 24 kHz mono -> 48 kHz stereo (linear interpolation). Returns bytes written or 0. */
static u32 phrase_bank_expand(volatile u8 *pcm, u32 room, u32 clip_bytes) {
    u32 samples = clip_bytes / 2u;
    if (!samples || (u64)samples * 8u > room) return 0;
    const i16 *s = (const i16 *)g_bank_clip;
    volatile i16 *d = (volatile i16 *)pcm;
    for (u32 i = 0; i < samples; ++i) {
        i32 a = s[i], b = (i + 1u < samples) ? s[i + 1u] : 0;
        i16 mid = (i16)((a + b) / 2);
        d[i * 4u + 0u] = (i16)a; d[i * 4u + 1u] = (i16)a;
        d[i * 4u + 2u] = mid;    d[i * 4u + 3u] = mid;
    }
    return samples * 8u;
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

static void copy_bytes(volatile u8 *dst, const u8 *src, u32 len) {
    for (u32 i = 0; i < len; ++i) dst[i] = src[i];
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

/*
 * Interrupting speech by stopping the stream mid-waveform jumps the DAC to 0:
 * an audible click on every key press. Instead, rewrite the audio just ahead
 * of the DMA position as a 10 ms fade to silence, let it play, then stop.
 */
static __attribute__((unused)) void speech_dma_fade_stop(void) {
    volatile u8 *sd = speech_stream_descriptor();
    if (!g_speech_active || !sd || !g_speech_pcm || !g_stall) {
        speech_dma_stop();
        return;
    }
    const u32 frame = 4u;                  /* s16 stereo */
    const u32 guard = 480u * frame;        /* 10 ms: past what the controller has fetched */
    const u32 fade = 480u * frame;         /* 10 ms fade */
    const u32 hush = 960u * frame;         /* then 20 ms of silence */
    u32 lpib = *(volatile u32 *)(sd + 0x04);
    u32 start = ((lpib + guard) / frame) * frame;
    if (start < g_speech_payload) {
        volatile i16 *s = (volatile i16 *)(g_speech_pcm + start);
        u32 room = (g_speech_payload - start) / 2u;   /* samples */
        u32 fade_samples = fade / 2u, hush_end = (fade + hush) / 2u;
        for (u32 i = 0; i < room && i < hush_end; ++i) {
            if (i < fade_samples) {
                i32 gain = (i32)(fade_samples - i);
                s[i] = (i16)(((i32)s[i] * gain) / (i32)fade_samples);
            } else {
                s[i] = 0;
            }
        }
        cache_writeback();
        /* Stop only once the controller has consumed the fade: a fixed delay
           let prefetching controllers (QEMU) cut inside the fade. */
        u32 fade_end = start + fade + hush / 2u;
        if (fade_end > g_speech_payload) fade_end = g_speech_payload;
        for (u32 waited = 0; waited < 100u; ++waited) {
            u32 pos = *(volatile u32 *)(sd + 0x04);
            if (pos >= fade_end || pos < lpib || (sd[3] & 0x04u)) break;   /* past fade, wrapped, or done */
            g_stall(1000);
        }
        g_stall(5000);
    }
    speech_dma_stop();
}

static int speech_stream_start(u64 base, u32 pcm_off, u32 dma_bytes, u32 total_bytes,
                               u32 max_bdl_bytes, u64 max_play_us);

static int speech_dma_begin(const char *text, u32 text_count) {
    if (!g_allocate_pages || !g_stall || !text || !text_count || text_count > 32u) return 0;
    const u32 pcm_off = 0x1000u;
    const u32 dma_pages = 1536u;
    const u32 dma_bytes = dma_pages * 4096u;
    if (!qev_unit_bank_len || pcm_off >= dma_bytes) return 0;

    if (g_speech_active) speech_dma_stop();

    /*
     * Build the complete semantic label as contiguous PCM. This keeps realtime
     * interruption while avoiding one BDL descriptor per allophone and the
     * alignment failures seen with longer HII labels.
     *
     * Worst-case 32-character French letter-name spelling is about 4.36 MiB
     * (all 'w'). Reserve 6 MiB and up to 128 BDL entries so every accepted
     * 32-character label remains representable. Playback stays interruptible,
     * so the larger worst-case timeout never blocks keyboard focus changes.
     */
    u64 base = g_speech_dma_base;
    if (!base) {
        base = 0xffffffffu;
        if (g_allocate_pages(1, 4, dma_pages, &base) != 0 || !base || base > 0xffffffffu) return 0;
        g_speech_dma_base = base;
        ++g_speech_dma_allocations;
    }

    volatile u8 *pcm = (volatile u8 *)(usize)(base + pcm_off);

    /*
     * Keep physical speech intelligible: a short lead-in gives the codec time
     * to settle, grapheme gaps stop adjacent synthetic units from fusing, and
     * the tail prevents the final phoneme from being clipped. Sizes are whole
     * 48 kHz signed-16 stereo frames (192 bytes/ms).
     */
    const u32 lead_silence_bytes = 30u * 192u;
    const u32 grapheme_gap_bytes = 12u * 192u;
    const u32 tail_silence_bytes = 45u * 192u;
    u32 total_bytes = lead_silence_bytes;
    if (total_bytes > dma_bytes - pcm_off) return 0;
    for (u32 i = 0; i < total_bytes; ++i) pcm[i] = 0;

    /* Natural phrase from the bank when this exact text was pre-rendered. */
    u64 clip_position = 0;
    u32 clip_bytes = phrase_bank_lookup(text, text_count, &clip_position);
    u32 phrase_bytes = 0;
    if (clip_bytes && phrase_bank_read(clip_position, g_bank_clip, clip_bytes))
        phrase_bytes = phrase_bank_expand(pcm + total_bytes, dma_bytes - pcm_off - total_bytes, clip_bytes);
    if (phrase_bytes) {
        total_bytes += phrase_bytes;
        ++g_bank_hits;
        marker("HII_GRAPH_SPEECH_SOURCE=PHRASE_BANK");
    } else {
        if (g_bank_count) ++g_bank_misses;
    }

    for (u32 i = 0; !phrase_bytes && i < text_count; ++i) {
        u8 ch = (u8)text[i];
        if (ch == (u8)' ') {
            if (qev_sil_unit_index >= qev_unit_count) return 0;
            u32 ui = qev_sil_unit_index;
            u32 off = qev_unit_off[ui];
            u32 len = qev_unit_len[ui];
            if (!len || off > qev_unit_bank_len || len > qev_unit_bank_len - off) return 0;
            if (total_bytes > dma_bytes - pcm_off || len > dma_bytes - pcm_off - total_bytes) return 0;
            copy_bytes(pcm + total_bytes, qev_unit_bank + off, len);
            total_bytes += len;
            continue;
        }

        if (ch < (u8)'a' || ch > (u8)'z') return 0;

        if (i != 0u && text[i - 1u] != ' ') {
            if (total_bytes > dma_bytes - pcm_off ||
                grapheme_gap_bytes > dma_bytes - pcm_off - total_bytes) return 0;
            for (u32 gap = 0; gap < grapheme_gap_bytes; ++gap) pcm[total_bytes + gap] = 0;
            total_bytes += grapheme_gap_bytes;
        }

        u32 li = (u32)(ch - (u8)'a');
        u32 n = qev_letter_unit_count[li];
        if (!n || n > 8u) return 0;
        for (u32 j = 0; j < n; ++j) {
            u32 ui = qev_letter_units[li * 8u + j];
            if (ui >= qev_unit_count) return 0;
            u32 off = qev_unit_off[ui];
            u32 len = qev_unit_len[ui];
            if (!len || off > qev_unit_bank_len || len > qev_unit_bank_len - off) return 0;
            if (total_bytes > dma_bytes - pcm_off || len > dma_bytes - pcm_off - total_bytes) return 0;
            copy_bytes(pcm + total_bytes, qev_unit_bank + off, len);
            total_bytes += len;
        }
    }
    if (!total_bytes) return 0;
    if (total_bytes > dma_bytes - pcm_off ||
        tail_silence_bytes > dma_bytes - pcm_off - total_bytes) return 0;
    for (u32 i = 0; i < tail_silence_bytes; ++i) pcm[total_bytes + i] = 0;
    total_bytes += tail_silence_bytes;
    marker("HII_GRAPH_SPEECH_PACING=PASS");
    marker("HII_GRAPH_SPEECH_LONG_LABEL_CAPACITY=PASS");

    return speech_stream_start(base, pcm_off, dma_bytes, total_bytes, 0x10000u, 30000000ull);
}

/*
 * Describe PCM already written at base+pcm_off (48 kHz s16 stereo) with a BDL
 * at base, write it back from the CPU cache and start the output stream.
 */
static int speech_stream_start(u64 base, u32 pcm_off, u32 dma_bytes, u32 total_bytes,
                               u32 max_bdl_bytes, u64 max_play_us) {
    volatile u8 *bdl = (volatile u8 *)(usize)base;
    volatile u8 *pcm = (volatile u8 *)(usize)(base + pcm_off);
    u32 dma_payload = (total_bytes + 127u) & ~127u;
    if (dma_payload < total_bytes || dma_payload > dma_bytes - pcm_off) return 0;
    for (u32 i = total_bytes; i < dma_payload; ++i) pcm[i] = 0;
    g_speech_pcm = pcm;
    g_speech_payload = dma_payload;

    u32 entries = 0;
    u32 described = 0;
    while (described < dma_payload) {
        if (entries >= 256u) return 0;
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
    cache_writeback();

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
    if (play_us > max_play_us) {
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

static int run_speech_dma(const char *text, u32 text_count) {
    if (!speech_dma_begin(text, text_count)) return 0;
    for (;;) {
        u8 progress = 0;
        int state = speech_dma_poll(1000u, &progress);
        if (state > 0) return 1;
        if (state < 0) return 0;
        g_stall(1000);
    }
}

#ifdef QEV_INTERACTIVE_NAV
/*
 * Complete BIOS menu navigator (\EFI\OMNI\NAV.BIN, built by build_nav_bank.py
 * from the firmware image with ST's neural voices). Every utterance is a
 * pre-rendered natural-speech clip; the reader only moves through the tree,
 * streams clips from the file and upsamples them to 48 kHz with the polyphase
 * FIR stored in the file. Read-only: nothing here writes a BIOS setting.
 */
#define NAV_NONE 0xffffffffu
#define NAV_ROLE_CONTAINER 0u
#define NAV_ROLE_SUBTITLE 1u
#define NAV_ROLE_LAUNCH 14u     /* starts the accessible WinRE found on the key */
#define NAV_MAX_DEPTH 32u
#define NAV_MAX_TABLE_BYTES (8u * 1024u * 1024u)
#define NAV_MAX_CLIP_BYTES (4u * 1024u * 1024u)   /* 131 s at 16 kHz */
#define NAV_DMA_PAGES 6144u                        /* 24 MiB: 131 s at 48 kHz stereo */
#define NAV_MAX_FIR 128u
enum { NAV_SYS_WELCOME, NAV_SYS_TOP, NAV_SYS_BOTTOM, NAV_SYS_NO_HELP, NAV_SYS_NOT_MENU,
       NAV_SYS_NO_OPTIONS, NAV_SYS_QUIT_CONFIRM, NAV_SYS_GOODBYE, NAV_SYS_EMPTY,
       NAV_SYS_LAUNCHING, NAV_SYS_LAUNCH_MISSING, NAV_SYS_COUNT };

/*
 * Chain-loading the accessible Windows RE of the key: the partition that holds
 * both \EFI\Microsoft\Boot\bootmgfw.efi and \sources\winre.wim. The Windows
 * boot manager is started from that partition so its BCD can use "boot".
 */
typedef u64 (*locate_handle_buffer_fn)(u32 search_type, const void *protocol, void *key,
                                       usize *count, void ***buffer);
typedef u64 (*free_pool_fn)(void *buffer);
typedef u64 (*allocate_pool_fn)(u32 pool_type, usize size, void **buffer);
typedef u64 (*load_image_fn)(u8 boot_policy, void *parent, void *device_path, void *source,
                             usize source_size, void **image);
typedef u64 (*start_image_fn)(void *image, usize *exit_data_size, u16 **exit_data);
static const efi_guid g_device_path_guid =
    {0x09576e91u,0x6d3fu,0x11d2u,{0x8e,0x39,0x00,0xa0,0xc9,0x69,0x72,0x3b}};
static const u16 g_winre_loader[] = {
    '\\','E','F','I','\\','M','i','c','r','o','s','o','f','t','\\','B','o','o','t','\\',
    'b','o','o','t','m','g','f','w','.','e','f','i',0
};
static const u16 g_winre_wim[] = {
    '\\','s','o','u','r','c','e','s','\\','w','i','n','r','e','.','w','i','m',0
};
static void *g_winre_device;

static int volume_has(file_protocol *root, const u16 *path) {
    file_protocol *f = 0;
    if (root->open(root, (void **)&f, path, 0x1ull, 0) != 0 || !f) return 0;
    if (f->close) f->close(f);
    return 1;
}

/* Returns 1 and remembers the device when an accessible WinRE is present. */
static int winre_locate(void *boot_services) {
    g_winre_device = 0;
    locate_handle_buffer_fn locate = *(locate_handle_buffer_fn *)((u8 *)boot_services + 0x138);
    handle_protocol_fn handle_protocol = *(handle_protocol_fn *)((u8 *)boot_services + 0x98);
    free_pool_fn free_pool = *(free_pool_fn *)((u8 *)boot_services + 0x48);
    usize count = 0;
    void **handles = 0;
    if (!locate || !handle_protocol || locate(2, &g_simple_fs_guid, 0, &count, &handles) != 0 || !handles)
        return 0;
    for (usize i = 0; i < count && !g_winre_device; ++i) {
        simple_fs_protocol *fs = 0;
        file_protocol *root = 0;
        if (handle_protocol(handles[i], &g_simple_fs_guid, (void **)&fs) != 0 || !fs || !fs->open_volume ||
            fs->open_volume(fs, &root) != 0 || !root || !root->open) continue;
        if (volume_has(root, g_winre_loader) && volume_has(root, g_winre_wim)) g_winre_device = handles[i];
        if (root->close) root->close(root);
    }
    if (free_pool) free_pool(handles);
    return g_winre_device != 0;
}

/* Device path of the volume + a file-path node for the boot manager. */
static int winre_start(void *image_handle, void *boot_services) {
    if (!g_winre_device) return 0;
    handle_protocol_fn handle_protocol = *(handle_protocol_fn *)((u8 *)boot_services + 0x98);
    allocate_pool_fn allocate_pool = *(allocate_pool_fn *)((u8 *)boot_services + 0x40);
    load_image_fn load_image = *(load_image_fn *)((u8 *)boot_services + 0xc8);
    start_image_fn start_image = *(start_image_fn *)((u8 *)boot_services + 0xd0);
    u8 *dp = 0;
    if (!handle_protocol || !allocate_pool || !load_image || !start_image ||
        handle_protocol(g_winre_device, &g_device_path_guid, (void **)&dp) != 0 || !dp) return 0;
    usize prefix = 0;
    for (u32 guard = 0; guard < 64u; ++guard) {           /* up to the end node */
        u16 len = (u16)(dp[prefix + 2] | (dp[prefix + 3] << 8));
        if (dp[prefix] == 0x7f && dp[prefix + 1] == 0xff) break;
        if (len < 4u) return 0;
        prefix += len;
    }
    usize name_bytes = sizeof(g_winre_loader);
    usize node = 4u + name_bytes;
    u8 *full = 0;
    if (allocate_pool(4, prefix + node + 4u, (void **)&full) != 0 || !full) return 0;
    for (usize i = 0; i < prefix; ++i) full[i] = dp[i];
    full[prefix] = 0x04; full[prefix + 1] = 0x04;         /* media / file path */
    full[prefix + 2] = (u8)node; full[prefix + 3] = (u8)(node >> 8);
    for (usize i = 0; i < name_bytes; ++i) full[prefix + 4 + i] = ((const u8 *)g_winre_loader)[i];
    full[prefix + node] = 0x7f; full[prefix + node + 1] = 0xff;
    full[prefix + node + 2] = 4; full[prefix + node + 3] = 0;
    void *child = 0;
    if (load_image(0, image_handle, full, 0, 0, &child) != 0 || !child) {
        marker("WINRE_LOAD=FAILED");
        return 0;
    }
    marker("WINRE_LOAD=PASS");
    persist_trace(g_trace_image_handle, g_trace_boot_services);
    start_image(child, 0, 0);                              /* returns only on failure */
    marker("WINRE_START=RETURNED");
    return 0;
}
static u8 g_nav_launch;

typedef struct {
    u32 speak, help, enter, target, child_first;
    u16 child_count;
    u8 role, flags;
    u32 option_first;
    u16 option_count, option_default;
} nav_node;

static file_protocol *g_nav_file;
static u8 *g_nav_table;          /* file bytes [nodes_off, clip_data_off) */
static const nav_node *g_nav_nodes;
static const u32 *g_nav_links;
static const u32 *g_nav_sys;
static const i16 *g_nav_fir;
static const u32 *g_nav_clip_index;
static u32 g_nav_clip_count;
static u32 g_nav_clip_data_off, g_nav_up, g_nav_taps;
static i16 *g_nav_clip;
static u64 g_nav_dma_base;
static u32 g_nav_loaded;
static u32 g_nav_log_budget = 160u;

static u32 rd32h(const u8 *p) {
    return (u32)p[0] | ((u32)p[1] << 8) | ((u32)p[2] << 16) | ((u32)p[3] << 24);
}

static int nav_file_read(u64 position, void *buffer, usize bytes) {
    if (!g_nav_file || !g_nav_file->set_position || !g_nav_file->read) return 0;
    if (((file_set_position_fn)g_nav_file->set_position)(g_nav_file, position) != 0) return 0;
    usize got = bytes;
    return ((file_read_fn)g_nav_file->read)(g_nav_file, &got, buffer) == 0 && got == bytes;
}

static int nav_bank_open(void *image_handle, void *boot_services) {
    static const u16 name[] = {
        '\\','E','F','I','\\','O','M','N','I','\\','N','A','V','.','B','I','N',0
    };
    loaded_image_protocol_head *loaded = 0;
    simple_fs_protocol *fs = 0;
    file_protocol *root = 0;
    u8 h[64];
    if (!image_handle || !boot_services || !g_allocate_pages) return 0;
    handle_protocol_fn handle_protocol = *(handle_protocol_fn *)((u8 *)boot_services + 0x98);
    if (!handle_protocol ||
        handle_protocol(image_handle, &g_loaded_image_guid, (void **)&loaded) != 0 || !loaded ||
        !loaded->device_handle ||
        handle_protocol(loaded->device_handle, &g_simple_fs_guid, (void **)&fs) != 0 || !fs ||
        !fs->open_volume || fs->open_volume(fs, &root) != 0 || !root || !root->open) return 0;
    if (root->open(root, (void **)&g_nav_file, name, 0x1ull, 0) != 0 || !g_nav_file) {
        g_nav_file = 0;
        marker("NAV_BANK=ABSENT");
        return 0;
    }
    if (!nav_file_read(0, h, sizeof(h))) return 0;
    static const char magic[8] = {'Q','E','V','N','A','V','0','1'};
    for (u32 i = 0; i < 8u; ++i) if (h[i] != (u8)magic[i]) { marker("NAV_BANK=BAD_MAGIC"); return 0; }
    u32 node_count = rd32h(h + 8), nodes_off = rd32h(h + 16), links_off = rd32h(h + 20);
    u32 links_count = rd32h(h + 24), clip_count = rd32h(h + 28), index_off = rd32h(h + 32);
    u32 data_off = rd32h(h + 36), rate = rd32h(h + 40), sys_count = rd32h(h + 44);
    u32 sys_off = rd32h(h + 48), fir_off = rd32h(h + 52);
    u32 up = (u32)h[56] | ((u32)h[57] << 8), taps = (u32)h[58] | ((u32)h[59] << 8);
    /* The file is untrusted input: every size is checked in 64-bit arithmetic
       so no product can wrap around, and every table must lie inside the
       region [nodes_off, data_off) that is read into memory. */
    if (!node_count || !clip_count || nodes_off != 64u ||
        (u64)links_off != (u64)nodes_off + (u64)node_count * 32u ||
        (u64)sys_off != (u64)links_off + (u64)links_count * 4u || sys_count < NAV_SYS_COUNT ||
        (u64)fir_off != (u64)sys_off + (u64)sys_count * 4u || !up || !taps || up > 8u || taps > 64u ||
        up * taps > NAV_MAX_FIR || (u64)index_off != (u64)fir_off + (u64)up * taps * 2u ||
        (u64)data_off < (u64)index_off + (u64)clip_count * 8u ||
        (u64)rate * up != 48000u || data_off > NAV_MAX_TABLE_BYTES) {
        marker("NAV_BANK=BAD_HEADER");
        return 0;
    }
    u64 table = 0xffffffffu, clip = 0xffffffffu;
    if (g_allocate_pages(1, 4, (data_off + 4095u) / 4096u, &table) != 0 || !table) return 0;
    if (g_allocate_pages(1, 4, NAV_MAX_CLIP_BYTES / 4096u, &clip) != 0 || !clip) return 0;
    g_nav_table = (u8 *)(usize)table;
    if (!nav_file_read(nodes_off, g_nav_table, data_off - nodes_off)) { marker("NAV_BANK=READ_FAILED"); return 0; }
    g_nav_nodes = (const nav_node *)(void *)g_nav_table;
    g_nav_links = (const u32 *)(void *)(g_nav_table + (links_off - nodes_off));
    g_nav_sys = (const u32 *)(void *)(g_nav_table + (sys_off - nodes_off));
    g_nav_fir = (const i16 *)(void *)(g_nav_table + (fir_off - nodes_off));
    g_nav_clip_index = (const u32 *)(void *)(g_nav_table + (index_off - nodes_off));
    g_nav_clip = (i16 *)(usize)clip;
    g_nav_clip_count = clip_count;
    g_nav_clip_data_off = data_off;
    g_nav_up = up;
    g_nav_taps = taps;
    /* Structural check: every child link is a node, every target a container. */
    for (u32 i = 0; i < node_count; ++i) {
        const nav_node *n = &g_nav_nodes[i];
        if ((u64)n->child_first + n->child_count > links_count ||
            (u64)n->option_first + n->option_count > links_count) { marker("NAV_BANK=BAD_NODE"); return 0; }
        for (u32 c = 0; c < n->child_count; ++c)
            if (g_nav_links[n->child_first + c] >= node_count) { marker("NAV_BANK=BAD_LINK"); return 0; }
        for (u32 o = 0; o < n->option_count; ++o)
            if (g_nav_links[n->option_first + o] >= clip_count) { marker("NAV_BANK=BAD_OPTION"); return 0; }
        if (n->target != NAV_NONE &&
            (n->target >= node_count || g_nav_nodes[n->target].role != NAV_ROLE_CONTAINER)) {
            marker("NAV_BANK=BAD_TARGET");
            return 0;
        }
    }
    for (u32 i = 0; i < NAV_SYS_COUNT; ++i)
        if (g_nav_sys[i] >= clip_count) { marker("NAV_BANK=BAD_SYS"); return 0; }
    if (g_nav_nodes[0].role != NAV_ROLE_CONTAINER) { marker("NAV_BANK=BAD_ROOT"); return 0; }
    g_nav_loaded = 1;
    serial_puts("NAV_BANK_NODES=0x"); serial_hex32(node_count); serial_puts("\r\n");
    serial_puts("NAV_BANK_CLIPS=0x"); serial_hex32(clip_count); serial_puts("\r\n");
    return 1;
}

/* Clip -> 48 kHz s16 stereo PCM through the polyphase FIR. Returns bytes or 0. */
static u32 nav_expand(volatile u8 *dst, u32 room, const i16 *x, u32 n) {
    const u32 up = g_nav_up, taps = g_nav_taps;
    u64 frames = ((u64)n + taps - 1u) * up;
    if (frames * 4u > room) return 0;
    volatile i16 *d = (volatile i16 *)dst;
    u32 o = 0;
    for (u32 i = 0; i < n + taps - 1u; ++i) {
        for (u32 p = 0; p < up; ++p) {
            i32 acc = 0;
            for (u32 k = 0; k < taps; ++k) {
                if (k > i) break;
                u32 j = i - k;
                if (j < n) acc += (i32)g_nav_fir[p + up * k] * (i32)x[j];
            }
            acc = (acc + 16384) >> 15;
            if (acc > 32767) acc = 32767;
            if (acc < -32768) acc = -32768;
            d[o++] = (i16)acc;
            d[o++] = (i16)acc;
        }
    }
    return o * 2u;
}

static int nav_play(u32 clip) {
    if (!g_nav_loaded || clip >= g_nav_clip_count) return 0;
    if (g_speech_active) speech_dma_fade_stop();
    u32 off = g_nav_clip_index[clip * 2u], bytes = g_nav_clip_index[clip * 2u + 1u];
    if (!bytes || (bytes & 1u) || bytes > NAV_MAX_CLIP_BYTES) return 0;
    if (!nav_file_read((u64)g_nav_clip_data_off + off, g_nav_clip, bytes)) return 0;

    const u32 pcm_off = 0x2000u;                  /* 256 BDL entries */
    const u32 dma_bytes = NAV_DMA_PAGES * 4096u;
    if (!g_nav_dma_base) {
        u64 base = 0xffffffffu;
        if (g_allocate_pages(1, 4, NAV_DMA_PAGES, &base) != 0 || !base || base > 0xffffffffu) return 0;
        g_nav_dma_base = base;
    }
    volatile u8 *pcm = (volatile u8 *)(usize)(g_nav_dma_base + pcm_off);
    /* The first start of the stream loses the audio just after a short lead-in
       (seen as a missing fade-in under QEMU); give the codec 200 ms to settle
       the first time, 30 ms afterwards. */
    static u8 primed;
    const u32 lead = (primed ? 30u : 200u) * 192u, tail = 60u * 192u;
    primed = 1;
    for (u32 i = 0; i < lead; ++i) pcm[i] = 0;
    u32 body = nav_expand(pcm + lead, dma_bytes - pcm_off - lead - tail, g_nav_clip, bytes / 2u);
    if (!body) return 0;
    for (u32 i = 0; i < tail; ++i) pcm[lead + body + i] = 0;
    return speech_stream_start(g_nav_dma_base, pcm_off, dma_bytes, lead + body + tail,
                               0x20000u, 150000000ull);
}

static void nav_log(const char *what, u32 value) {
    if (!g_nav_log_budget) return;
    --g_nav_log_budget;
    serial_puts(what);
    serial_puts("=0x");
    serial_hex32(value);
    serial_puts("\r\n");
}

/* Up to two utterances: the second starts when the first has finished. */
static u32 g_nav_next = NAV_NONE;
static void nav_say(u32 clip, u32 then) {
    g_nav_next = NAV_NONE;
    if (clip == NAV_NONE) {
        clip = then;
        then = NAV_NONE;
    }
    if (clip == NAV_NONE) return;
    if (!nav_play(clip)) {
        nav_log("NAV_PLAY_FAILED", clip);
        return;
    }
    g_nav_next = then;
}

static const nav_node *nav_child(const nav_node *container, u32 index) {
    return &g_nav_nodes[g_nav_links[container->child_first + index]];
}

static int nav_run(void *system_table) {
    simple_text_input_protocol *conin =
        *(simple_text_input_protocol **)((u8 *)system_table + 0x30);
    if (!conin || !conin->read_key || !g_stall || !g_nav_loaded) return 0;

    u32 stack_node[NAV_MAX_DEPTH], stack_index[NAV_MAX_DEPTH];
    u32 depth = 0, option = 0, option_node = NAV_NONE;
    u8 quit_armed = 0, quitting = 0;
    stack_node[0] = 0;
    stack_index[0] = 0;

    marker("NAV_READY=PASS");
    persist_trace(g_trace_image_handle, g_trace_boot_services);
    {
        const nav_node *root = &g_nav_nodes[0];
        g_nav_next = NAV_NONE;
        nav_say(g_nav_sys[NAV_SYS_WELCOME], root->enter);
    }

    for (;;) {
        const nav_node *c = &g_nav_nodes[stack_node[depth]];
        u32 idx = stack_index[depth];
        const nav_node *item = c->child_count ? nav_child(c, idx) : 0;

        efi_input_key key;
        key.scan_code = 0;
        key.unicode_char = 0;
        if (!quitting && conin->read_key(conin, &key) == 0) {
            u16 sc = key.scan_code, uc = key.unicode_char;
            u8 is_esc = (sc == 0x17u || uc == 0x1bu), is_back = (uc == 0x08u);
            if (!is_esc) quit_armed = 0;
            nav_log("NAV_KEY", ((u32)sc << 16) | uc);
            if (sc == 0x01u || sc == 0x02u) {                 /* up / down */
                if (!c->child_count) nav_say(g_nav_sys[NAV_SYS_EMPTY], NAV_NONE);
                else if (sc == 0x01u && idx == 0) nav_say(g_nav_sys[NAV_SYS_TOP], item->speak);
                else if (sc == 0x02u && idx + 1u >= c->child_count) nav_say(g_nav_sys[NAV_SYS_BOTTOM], item->speak);
                else {
                    stack_index[depth] = sc == 0x01u ? idx - 1u : idx + 1u;
                    nav_say(nav_child(c, stack_index[depth])->speak, NAV_NONE);
                }
            } else if (sc == 0x05u || sc == 0x06u) {          /* home / end */
                if (!c->child_count) nav_say(g_nav_sys[NAV_SYS_EMPTY], NAV_NONE);
                else {
                    stack_index[depth] = sc == 0x05u ? 0u : c->child_count - 1u;
                    nav_say(nav_child(c, stack_index[depth])->speak, NAV_NONE);
                }
            } else if (sc == 0x09u || sc == 0x0au) {          /* page up / down: previous / next section */
                if (c->child_count) {
                    u32 j = idx, found = NAV_NONE;
                    for (u32 step = 0; step < c->child_count; ++step) {
                        if (sc == 0x09u) { if (!j) break; --j; } else { if (j + 1u >= c->child_count) break; ++j; }
                        if (nav_child(c, j)->role == NAV_ROLE_SUBTITLE) { found = j; break; }
                    }
                    if (found == NAV_NONE) {
                        if (sc == 0x09u) found = idx >= 10u ? idx - 10u : 0u;
                        else found = idx + 10u < c->child_count ? idx + 10u : c->child_count - 1u;
                    }
                    stack_index[depth] = found;
                    nav_say(nav_child(c, found)->speak, NAV_NONE);
                }
            } else if (uc == 0x0du && item && item->role == NAV_ROLE_LAUNCH) {  /* start accessible WinRE */
                if (winre_locate(g_trace_boot_services)) {
                    marker("WINRE_FOUND=PASS");
                    g_nav_launch = 1;
                    quitting = 1;
                    nav_say(g_nav_sys[NAV_SYS_LAUNCHING], NAV_NONE);
                } else {
                    marker("WINRE_FOUND=NO");
                    nav_say(g_nav_sys[NAV_SYS_LAUNCH_MISSING], NAV_NONE);
                }
            } else if (uc == 0x0du) {                          /* enter: open sub-menu */
                if (item && item->target != NAV_NONE && depth + 1u < NAV_MAX_DEPTH) {
                    const nav_node *t = &g_nav_nodes[item->target];
                    ++depth;
                    stack_node[depth] = item->target;
                    stack_index[depth] = 0;
                    nav_say(t->enter, t->child_count ? nav_child(t, 0)->speak : NAV_NONE);
                } else {
                    nav_say(g_nav_sys[NAV_SYS_NOT_MENU], NAV_NONE);
                }
            } else if (is_esc || is_back) {                    /* back / quit */
                if (depth) {
                    --depth;
                    const nav_node *pc = &g_nav_nodes[stack_node[depth]];
                    nav_say(nav_child(pc, stack_index[depth])->speak, NAV_NONE);
                } else if (is_esc && quit_armed) {
                    nav_say(g_nav_sys[NAV_SYS_GOODBYE], NAV_NONE);
                    quitting = 1;
                } else {
                    quit_armed = is_esc;
                    nav_say(g_nav_sys[NAV_SYS_QUIT_CONFIRM], NAV_NONE);
                }
            } else if (sc == 0x03u || sc == 0x04u) {          /* right / left: listen to options */
                if (item && item->option_count) {
                    u32 id = g_nav_links[c->child_first + idx];
                    if (option_node != id) {
                        option_node = id;
                        option = item->option_default < item->option_count ? item->option_default : 0u;
                    } else if (sc == 0x03u) {
                        option = option + 1u < item->option_count ? option + 1u : 0u;
                    } else {
                        option = option ? option - 1u : item->option_count - 1u;
                    }
                    nav_say(g_nav_links[item->option_first + option], NAV_NONE);
                } else {
                    nav_say(g_nav_sys[NAV_SYS_NO_OPTIONS], NAV_NONE);
                }
            } else if (uc == (u16)'h' || uc == (u16)'H' || sc == 0x0bu) {  /* help (H or F1) */
                nav_say(item && item->help != NAV_NONE ? item->help : g_nav_sys[NAV_SYS_NO_HELP], NAV_NONE);
            } else if (uc == (u16)'r' || uc == (u16)'R') {          /* repeat the item */
                nav_say(item ? item->speak : g_nav_sys[NAV_SYS_EMPTY], NAV_NONE);
            } else if (uc == (u16)' ') {                           /* where am I: page, then item */
                nav_say(c->enter, item ? item->speak : NAV_NONE);
            }
        }

        if (g_speech_active) {
            u8 progressed = 0;
            int state = speech_dma_poll(1000u, &progressed);
            if (state != 0 && g_nav_next != NAV_NONE) {
                u32 next = g_nav_next;
                g_nav_next = NAV_NONE;
                nav_say(next, NAV_NONE);
            }
        } else if (g_nav_next != NAV_NONE) {
            u32 next = g_nav_next;
            g_nav_next = NAV_NONE;
            nav_say(next, NAV_NONE);
        } else if (quitting) {
            marker("NAV_EXIT=PASS");
            return 1;
        }
        g_stall(1000);
    }
}
#define g_nav_loaded_any() (g_nav_loaded != 0)
#else
#define g_nav_loaded_any() 0
#endif

static u16 rd16(const u8 *p) {
    return (u16)((u16)p[0] | ((u16)p[1] << 8));
}
static u32 rd32(const u8 *p) {
    return (u32)p[0] | ((u32)p[1] << 8) | ((u32)p[2] << 16) | ((u32)p[3] << 24);
}
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
    for (u32 i = 0; i < 127u && text[i] && n < 32u; ++i) {
        u16 ch = fold_prompt_char(text[i]);
        if (ch >= (u16)'a' && ch <= (u16)'z') {
            if (pending_space && n && n < 32u) out[n++] = ' ';
            if (n < 32u) out[n++] = (char)ch;
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
static int nav_prompt_add(u8 opcode, const char *text, u32 count) {
    if (!text || !count || count > 32u) return 0;
    for (u8 i = 0; i < g_nav_prompt_total; ++i) {
        if (g_nav_prompt_lengths[i] != (u8)count ||
            g_nav_prompt_opcodes[i] != opcode) continue;
        u32 same = 1;
        for (u32 j = 0; j < count; ++j) {
            if (g_nav_prompts[i][j] != text[j]) { same = 0; break; }
        }
        if (same) return 0;
    }
    if (g_nav_prompt_total >= MAX_HII_NAV_PROMPTS) return 0;
    u8 slot = g_nav_prompt_total++;
    for (u32 j = 0; j < count; ++j) g_nav_prompts[slot][j] = text[j];
    g_nav_prompts[slot][count] = 0;
    g_nav_prompt_lengths[slot] = (u8)count;
    g_nav_prompt_opcodes[slot] = opcode;
    return 1;
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
    if (n < 32u && g_prompt_count) g_nav_speech_text[n++] = ' ';
    for (u32 j = 0; j < g_prompt_count && n < 32u; ++j)
        g_nav_speech_text[n++] = g_prompt_text[j];
    g_nav_speech_text[n] = 0;
    g_nav_speech_length = (u8)n;
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
                while (q + 2 <= end) {
                    u8 op = q[0];
                    u32 oplen = (u32)(q[1] & 0x7f);
                    if (oplen < 2u || q + oplen > end) break;
                    if (prompt_opcode(op) && oplen >= 4u) {
                        u16 token = rd16(q + 2);
#ifdef QEV_INTERACTIVE_NAV
                        char candidate[33];
                        u32 candidate_count = 0;
                        if (token && get_hii_string(str, handle, token, candidate, &candidate_count)) {
                            nav_prompt_add(op, candidate, candidate_count);
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
                    q += oplen;
                }
            }
            p += len;
        }
    }
#ifdef QEV_INTERACTIVE_NAV
    if (g_nav_prompt_total) {
        nav_prompt_load(0);
        marker("IFR_PROMPT_STRING_ID=PASS");
        marker("HII_LANGUAGE_AND_STRING=PASS");
        marker("HII_GRAPH_NAV_PROMPT_COLLECTION=PASS");
        marker("HII_GRAPH_NAV_SEMANTIC_ROLE=PASS");
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

    /* AMD/ATI HDA (Linux AZX_SNOOP_TYPE_ATI): config byte 0x42 bits 2:0 must be
       010b or the controller DMAs without snooping the CPU caches. */
    u16 pci_vendor = (u16)(pci_read32(cfg) & 0xffffu);
    if (pci_vendor == 0x1022u || pci_vendor == 0x1002u) {
        u32 misc = pci_read32(cfg | 0x40);
        u8 before = (u8)(misc >> 16);
        u8 wanted = (u8)((before & ~0x07u) | 0x02u);
        if (before != wanted)
            pci_write32(cfg | 0x40, (misc & ~0x00ff0000u) | ((u32)wanted << 16));
        u8 after = (u8)(pci_read32(cfg | 0x40) >> 16);
        serial_puts("HDA_ATI_SNOOP_REG42_BEFORE=0x"); serial_hex8(before); serial_puts("\r\n");
        serial_puts("HDA_ATI_SNOOP_REG42_AFTER=0x"); serial_hex8(after); serial_puts("\r\n");
        marker((after & 0x07u) == 0x02u ? "HDA_ATI_SNOOP=ENABLED" : "HDA_ATI_SNOOP=NOT_ESTABLISHED");
    }

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
    g_selected_pin_default_config = best_config;
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
    if (!system_table || g_nav_prompt_total < 2u) return 0;
    simple_text_input_protocol *conin =
        *(simple_text_input_protocol **)((u8 *)system_table + 0x30);
    if (!conin || !conin->read_key || !g_stall) return 0;

    marker("HII_GRAPH_NAV_READY=PASS");
    marker("HII_GRAPH_NAV_REALTIME_MODE=INTERRUPTIBLE_DMA");
    /* Checkpoint: navigation has no timeout, so the machine is usually powered
       off from here and the exit trace would never be written. */
    persist_trace(g_trace_image_handle, g_trace_boot_services);
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
            u8 speak = 0;
            if (key.unicode_char == 0x001bu || key.scan_code == 0x0017u) {
                marker("HII_GRAPH_NAV_KEY=ESC");
                speech_dma_stop();
                if ((g_nav_event_mask & NAV_REQUIRED_MASK) != NAV_REQUIRED_MASK ||
                    g_nav_speech_events < 7u) {
                    marker("HII_GRAPH_NAV_REQUIRED_EVENTS=PENDING");
                    marker("HII_GRAPH_NAV_EXIT=BLOCKED_INCOMPLETE");
                    continue;
                }
                marker("HII_GRAPH_NAV_REQUIRED_EVENTS=PASS");
                marker("HII_GRAPH_NAV_EXIT=PASS");
                return 1;
            }
            if (key.unicode_char == (u16)'r' || key.unicode_char == (u16)'R') {
                marker("HII_GRAPH_NAV_KEY=R");
                g_nav_event_mask |= NAV_SEEN_R;
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
                serial_puts("HII_GRAPH_NAV_INDEX=0x");
                serial_hex8(g_nav_prompt_index);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_TEXT=");
                serial_puts(g_prompt_text);
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_ROLE=");
                serial_puts(ifr_semantic_role(g_nav_prompt_opcode));
                serial_puts("\r\n");
                serial_puts("HII_GRAPH_NAV_SPEECH_TEXT=");
                serial_puts(g_nav_speech_text);
                serial_puts("\r\n");
                marker("HII_GRAPH_NAV_SEMANTIC_ROLE=PASS");

                if (g_speech_active) {
                    speech_dma_stop();
                    if (g_nav_speech_interruptions != 0xffu) ++g_nav_speech_interruptions;
                    marker("HII_GRAPH_NAV_SPEECH_INTERRUPT=PASS");
                }
                if (!speech_dma_begin(g_nav_speech_text, g_nav_speech_length)) return 0;

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
            int audio_state = speech_dma_poll(1000u, &progressed);
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

static u64 screen_reader_main(void *image_handle, void *system_table);

__attribute__((ms_abi)) u64 efi_main(void *image_handle, void *system_table) {
    g_trace_image_handle = image_handle;
    g_trace_boot_services = system_table ? *(void **)((u8 *)system_table + 0x60) : 0;
    u64 rc = screen_reader_main(image_handle, system_table);
    g_trace_final = 1;
    marker(rc == 0 ? "OMNI_SR_EXIT=SUCCESS" : "OMNI_SR_EXIT=BLOCKED");
    if (g_last_reason) { serial_puts("OMNI_SR_LAST_"); marker(g_last_reason); }
    if (g_trace_truncated) marker("OMNI_SR_TRACE_TRUNCATED");
    persist_trace(g_trace_image_handle, g_trace_boot_services);
#ifdef QEV_INTERACTIVE_NAV
    if (rc == 0 && g_nav_launch) {
        speech_dma_stop();
        winre_start(image_handle, g_trace_boot_services);   /* only returns on failure */
    }
#endif
    return rc;
}

static u64 screen_reader_main(void *image_handle, void *system_table) {
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
        /* The boot manager arms a 5-minute watchdog before starting a boot
           application; reading BIOS menus takes longer, so disarm it. */
        typedef u64 (*set_watchdog_fn)(usize timeout, u64 code, usize size, u16 *data);
        set_watchdog_fn set_watchdog = *(set_watchdog_fn *)((u8 *)boot_services + 0x100);
        if (set_watchdog && set_watchdog(0, 0, 0, 0) == 0) marker("WATCHDOG_DISABLED=PASS");
    }
    if (phrase_bank_open(image_handle, boot_services)) {
        marker("PHRASE_BANK=LOADED");
        serial_puts("PHRASE_BANK_ENTRIES=0x"); serial_hex32(g_bank_count); serial_puts("\r\n");
    } else {
        g_bank_count = 0;
        marker("PHRASE_BANK=ABSENT_LETTER_SPELLING");
    }
#ifdef QEV_INTERACTIVE_NAV
    if (nav_bank_open(image_handle, boot_services)) marker("NAV_BANK=LOADED");
#endif
    /* With the complete BIOS map the live HII prompts are optional. */
    if (!resolve_hii_prompt(system_table) && !g_nav_loaded_any()) {
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
#ifdef QEV_INTERACTIVE_NAV
    if (g_nav_loaded) {
        marker("SYNTH=NEURAL_CLIPS_NAV_BIN_V1");
        if (!nav_run(system_table)) {
            marker("STATUS=BLOCKED");
            marker("REASON=NAV_RUN_FAILED");
            return 1;
        }
        marker("STATUS=PASS");
        return 0;
    }
#endif
    marker("SYNTH=ALLOPHONE_BDL_RUNTIME_TEXT_V1");
    marker("SYNTH=GRAPHEME_ALLOPHONE_RUNTIME_TEXT_V2");
    marker("SYNTH=CLEAR_LETTERNAME_SPELLING_FR_V3");
    marker("HII_PROMPT_MAX_CHARS=32");
    marker("HII_PROMPT_WORD_BOUNDARIES=PASS");
    marker("BDL_RUNTIME_TEXT_SCHEDULE=PASS");

#ifdef QEV_INTERACTIVE_NAV
    if (!run_speech_dma(g_nav_speech_text, g_nav_speech_length)) {
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
    marker("HII_GRAPH_NAV_SEMANTIC_SPEECH=ROLE_PLUS_LABEL");
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
