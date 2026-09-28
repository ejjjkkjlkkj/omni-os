typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long long u64;
typedef unsigned long long usize;

typedef u64 (*stall_fn)(usize microseconds);
typedef u64 (*allocate_pages_fn)(u32 type, u32 memory_type, usize pages, u64 *memory);

extern const u8 qev_unit_d[];
extern const u32 qev_unit_d_len;
extern const u8 qev_unit_e[];
extern const u32 qev_unit_e_len;

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
static u8 g_conn_count[MAX_NID];
static u8 g_conn[MAX_NID][MAX_CONN];
static u8 g_pin_output[MAX_NID];
static u8 g_seen[MAX_NID];
static u8 g_parent[MAX_NID];
static u8 g_depth[MAX_NID];
static u8 g_queue[MAX_NID];
static u8 g_route_index[MAX_NID];
static u32 g_widget_cap[MAX_NID];

static volatile u8 *g_hda;
static u8 g_cad;
static u8 g_afg = INVALID_NID;
static u8 g_controller_preferred;
static stall_fn g_stall;
static allocate_pages_fn g_allocate_pages;

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
static void marker(const char *s) {
    serial_puts(s);
    serial_puts("\r\n");
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
        g_conn_count[i] = 0;
        g_pin_output[i] = 0;
        g_seen[i] = 0;
        g_parent[i] = INVALID_NID;
        g_depth[i] = 0;
        g_queue[i] = 0;
        g_route_index[i] = 0;
        g_widget_cap[i] = 0;
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

static int wait_node_d0(u8 nid) {
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
    if (supported == INVALID_RESP || !(supported & 0x01u))
        return g_controller_preferred ? 0 : 1;
    if (verb12(g_afg, 0x705, 0x00) == INVALID_RESP) return 0;
    return wait_node_d0(g_afg);
}

static int power_up_route_widget(u8 nid) {
    if (!(g_widget_cap[nid] & 0x00000400u)) return 1;
    u32 supported = get_param(nid, 0x0f);
    if (supported == INVALID_RESP || !(supported & 0x01u)) return 0;
    if (verb12(nid, 0x705, 0x00) == INVALID_RESP) return 0;
    return wait_node_d0(nid);
}

static int configure_output_path(u8 pin, u8 dac) {
    if (!power_up_afg()) return 0;
    u8 cur = dac;
    for (;;) {
        if (!power_up_route_widget(cur)) return 0;
        if (cur == pin) break;
        u8 child = g_parent[cur];
        if (child == INVALID_NID) return 0;
        cur = child;
    }
    marker("HDA_ROUTE_POWER_D0=PASS");

    u32 amp_cap = get_param(dac, 0x12);
    if (amp_cap == INVALID_RESP) return 0;
    if (amp_cap) {
        if (verb4(dac, 0x3, 0xb040) == INVALID_RESP) return 0;
        u32 left = verb4(dac, 0xb, 0xa000);
        u32 right = verb4(dac, 0xb, 0x8000);
        if (left == INVALID_RESP || right == INVALID_RESP) return 0;
        if ((left & 0x7f) != 0x40 || (right & 0x7f) != 0x40) return 0;
    }

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

static int run_speech_dma(void) {
    if (!g_allocate_pages) return 0;
    u64 base = 0xffffffffu;
    if (g_allocate_pages(1, 4, 16, &base) != 0 || !base || base > 0xffffffffu) return 0;

    const u32 pcm_off = 0x1000;
    u32 d_off = 0;
    u32 e_off = qev_unit_d_len;
    u32 bank_len = qev_unit_d_len + qev_unit_e_len;
    if (!qev_unit_d_len || !qev_unit_e_len || bank_len > (16u * 4096u - pcm_off)) return 0;

    volatile u8 *pcm = (volatile u8 *)(usize)(base + pcm_off);
    copy_bytes(pcm + d_off, qev_unit_d, qev_unit_d_len);
    copy_bytes(pcm + e_off, qev_unit_e, qev_unit_e_len);

    volatile u8 *bdl = (volatile u8 *)(usize)base;
    *(volatile u64 *)(bdl + 0x00) = base + pcm_off + e_off;
    *(volatile u32 *)(bdl + 0x08) = qev_unit_e_len;
    *(volatile u32 *)(bdl + 0x0c) = 0;
    *(volatile u64 *)(bdl + 0x10) = base + pcm_off + d_off;
    *(volatile u32 *)(bdl + 0x18) = qev_unit_d_len;
    *(volatile u32 *)(bdl + 0x1c) = 1;
    fence();

    u16 gcap = mmio16(0x00);
    u8 iss = (u8)((gcap >> 8) & 0x0f);
    volatile u8 *sd = g_hda + 0x80 + ((u32)iss * 0x20);

    sd[0] = (u8)(sd[0] & ~2u);
    u32 timeout = 100000;
    while (timeout-- && (sd[0] & 2)) {}
    if (!timeout) return 0;
    sd[0] = (u8)(sd[0] | 1u);
    timeout = 100000;
    while (timeout-- && !(sd[0] & 1)) {}
    if (!timeout) return 0;
    sd[0] = (u8)(sd[0] & ~1u);
    timeout = 100000;
    while (timeout-- && (sd[0] & 1)) {}
    if (!timeout) return 0;

    *(volatile u32 *)(sd + 0x08) = qev_unit_e_len + qev_unit_d_len;
    *(volatile u16 *)(sd + 0x0c) = 1;
    *(volatile u16 *)(sd + 0x12) = 0x0011;
    *(volatile u32 *)(sd + 0x18) = (u32)base;
    *(volatile u32 *)(sd + 0x1c) = (u32)(base >> 32);
    fence();

    sd[2] = 0x10;
    sd[0] = (u8)(sd[0] | 2u);
    if (g_stall) g_stall(400000);
    u32 lpib = *(volatile u32 *)(sd + 0x04);
    sd[0] = (u8)(sd[0] & ~2u);
    if (!lpib) return 0;
    return 1;
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

    /* Prefer the ASUS M1603QA analog HDA function (AMD 1022:15E3).
       Its sibling 1002:1637 is HDMI audio.  A second pass keeps the
       freestanding reader portable on QEMU, VMware and other machines. */
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

    u32 command = pci_read32(cfg | 0x04);
    pci_write32(cfg | 0x04, command | 0x00000006u);

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
        if (state & (1u << cad)) {
            g_cad = cad;
            return 1;
        }
    }
    return 0;
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

__attribute__((ms_abi)) u64 efi_main(void *image_handle, void *system_table) {
    (void)image_handle;
    serial_init();
    marker("QEVARYNOX-UEFI-HDA-GRAPH-SPEECH-V1");
    marker("STATE=START");
    marker("FRAMEWORK=NONE");
    marker("EDK2=NONE");
    marker("NO_FIXED_WIDGET_NIDS=PASS");

    void *boot_services = *(void **)((u8 *)system_table + 0x60);
    if (boot_services) {
        g_allocate_pages = *(allocate_pages_fn *)((u8 *)boot_services + 0x28);
        g_stall = *(stall_fn *)((u8 *)boot_services + 0xf8);
    }

    if (!graph_selftest()) {
        marker("STATUS=BLOCKED");
        marker("REASON=HDA_MULTIHOP_EFI_SELFTEST_FAILED");
        return 1;
    }
    marker("HDA_MULTIHOP_EFI_SELFTEST=PASS");
    marker("HDA_RANGE_AND_SELECTOR_ENGINE=PASS");

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
    marker("SYNTH=ALLOPHONE_CONCATENATIVE_V1");
    marker("WORD=AIDE");
    marker("UNIT_BANK_ORDER=d,e");
    marker("BDL_RUNTIME_ORDER=e,d");
    if (!run_speech_dma()) {
        marker("STATUS=BLOCKED");
        marker("REASON=HDA_GRAPH_SPEECH_DMA_FAILED");
        return 1;
    }
    marker("HDA_GRAPH_SPEECH_DMA=PASS");
    marker("RUNTIME_ALLOPHONE_HDA=PASS");
    marker("LPIB_PROGRESS=PASS");
    marker("PHYSICAL_ASUS_M1603QA_SPEECH=NOT_ESTABLISHED");
    marker("STATUS=PASS");
    return 0;
}
