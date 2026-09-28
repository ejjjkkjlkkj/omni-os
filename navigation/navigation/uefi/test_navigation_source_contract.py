#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path

SRC = Path(__file__).with_name("hii_graph_prompt_speech_uefi.c")
text = SRC.read_text(encoding="utf-8")

required = (
    'HII_GRAPH_NAV_REALTIME_MODE=INTERRUPTIBLE_DMA',
    'HII_GRAPH_SPEECH_QUEUE=INTERRUPTIBLE_64',
    'HII_GRAPH_SPEECH_WORD_BOUNDARY_CHUNKING=PASS',
    'HII_GRAPH_SPEECH_HYBRID_WORD_MODE=PASS',
    'HII_GRAPH_SPEECH_WORD_PRONUNCIATION=PASS',
    'HII_GRAPH_SPEECH_WORD_FALLBACK=LETTER_NAMES',
    'HII_GRAPH_SPEECH_UNKNOWN_WORD_FALLBACK=PASS',
    'speech_lookup_word',
    'qev_word_count',
    'qev_word_name_stride',
    'qev_word_unit_stride',
    'qev_word_name_len',
    'qev_word_names',
    'qev_word_unit_count',
    'qev_word_units',
    'phoneme_gap_bytes = 0u',
    'grapheme_gap_bytes = 18u * 192u',
    'HII_GRAPH_SPEECH_CONTINUOUS_PHONEMES=PASS',
    'HII_GRAPH_SPEECH_WHOLE_CLIP_VOICECORE=PASS',
    'HII_GRAPH_SPEECH_WHOLE_PHRASE_CLIP=PASS',
    'HII_GRAPH_SPEECH_PHRASE_FIRST_MODE=PASS',
    'speech_lookup_phrase',
    'qev_phrase_count',
    'qev_phrase_name_stride',
    'qev_phrase_name_len',
    'qev_phrase_names',
    'qev_phrase_unit_index',
    'append_unit_pcm',
    'qev_unit_source_rate',
    'HII_GRAPH_SPEECH_MULTI_CHUNK=PASS',
    'HII_GRAPH_SPEECH_CHUNK_START=PASS',
    'HII_GRAPH_SPEECH_CHUNK_CONTINUE=PASS',
    'HII_GRAPH_SPEECH_PHRASE_COMPLETE=PASS',
    'QEV_NAV_TEXT_MAX 64u',
    'QEV_SPEECH_CHUNK_MAX 32u',
    'speech_phrase_begin',
    'speech_phrase_poll',
    'speech_phrase_cancel',
    'speech_phrase_start_next',
    'g_speech_phrase',
    'g_speech_chunk',
    'HII_GRAPH_NAV_DIRECTIONAL_MODEL=BLIND_SIMPLE',
    'HII_GRAPH_NAV_TAB_FORWARD=PASS',
    'HII_GRAPH_NAV_CONTEXT_HELP=PASS',
    'HII_GRAPH_NAV_CONTEXT_HELP_SPEECH=PASS',
    'HII_GRAPH_NAV_DISCOVERY_PROMPT=PASS',
    'HII_GRAPH_NAV_HELP_DISCOVERABLE=PASS',
    'HII_GRAPH_NAV_BUILTIN_HELP=PASS',
    'HII_GRAPH_NAV_SIMPLE_EXIT=PASS',
    'HII_GRAPH_NAV_EXIT_DISCARD_SPEECH=PASS',
    'preview edits discarded',
    'HII_GRAPH_NAV_REQUIRED_EVENTS=PENDING_NONBLOCKING',
    'HII_GRAPH_NAV_ENTER_CONTEXT_ACTION=PASS',
    'HII_GRAPH_NAV_ENTER_TOGGLE=PASS',
    'HII_GRAPH_NAV_ENTER_CHOICE=PASS',
    'ready press f1 for help',
    'up down move left right change enter action escape back f1 help',
    'HII_GRAPH_NAV_LEFT_RIGHT_EDIT=PASS',
    'HII_GRAPH_NAV_ARROW_CHOICE=PASS',
    'HII_GRAPH_NAV_ARROW_NUMERIC=PASS',
    'HII_GRAPH_NAV_ARROW_EDIT_BLOCKED=PASS',
    'no change',
    'HII_GRAPH_NAV_LEFT_FALLBACK_MOVE=PASS',
    'HII_GRAPH_NAV_RIGHT_FALLBACK_MOVE=PASS',
    'HII_GRAPH_NAV_SPEECH_INTERRUPT=PASS',
    'HII_GRAPH_NAV_SPEECH_INTERRUPT_EARLY=PASS',
    'HII_GRAPH_NAV_STRUCTURAL_KEYS=PASS',
    'HII_GRAPH_NAV_FORM_KEYS=PASS',
    'HII_GRAPH_NAV_FORM_ID_TRACKING=PASS',
    'HII_GRAPH_NAV_QUESTION_ID_TRACKING=PASS',
    'HII_GRAPH_NAV_VARSTORE_METADATA=PASS',
    'HII_GRAPH_NAV_VARSTORE_CATALOG=PASS',
    'HII_GRAPH_NAV_ONEOF_OPTIONS=PASS',
    'HII_GRAPH_NAV_LIVE_OPTION_MATCH=PASS',
    'HII_GRAPH_NAV_LIVE_VALUE_KEY=PASS',
    'HII_GRAPH_NAV_LIVE_VALUE_READ=PASS',
    'HII_GRAPH_NAV_FOCUS_VALUE_SPEECH=PASS',
    'HII_GRAPH_NAV_INITIAL_VALUE_SPEECH=PASS',
    'HII_GRAPH_NAV_SEMANTIC_SPEECH=ROLE_STATE_LABEL_VALUE',
    'HII_GRAPH_NAV_SEMANTIC_CORE=PASS',
    'HII_GRAPH_NAV_VALUE_OPTIONAL_SPEECH=PASS',
    'HII_GRAPH_NAV_PASSWORD_VALUE_READ_AVOIDED=PASS',
    'HII_GRAPH_NAV_SEMANTIC_TRUNCATION=PASS',
    'g_nav_focus_speech[QEV_NAV_TEXT_MAX + 1u]',
    'node.label = g_nav_prompt_lengths[prompt_index]',
    'node.value = (have_value && value_text && value_length) ? value_text : 0;',
    'nav_semantic_role_for_opcode',
    'nav_semantic_state_bits',
    'qev_semantic_focus_utterance',
    '#include "semantic_core.h"',
    'nav_build_focus_speech',
    'g_nav_focus_speech',
    'HII_GRAPH_NAV_CONDITION_TRACKING=PASS',
    'HII_GRAPH_NAV_CONDITION_EVALUATOR=PASS',
    'HII_GRAPH_NAV_SUPPRESS_RUNTIME=PASS',
    'HII_GRAPH_NAV_GRAY_RUNTIME=PASS',
    'HII_GRAPH_NAV_DISABLE_RUNTIME=PASS',
    'HII_GRAPH_NAV_DISABLED_PREVIEW=BLOCKED',
    '(NAV_COND_GRAY | NAV_COND_DISABLE | NAV_COND_UNKNOWN)',
    'HII_GRAPH_NAV_CONDITION_UNKNOWN_SAFE=PASS',
    'HII_GRAPH_NAV_QUESTION_CATALOG=PASS',
    'nav_eval_condition_expression',
    'nav_read_question_value',
    'nav_question_add',
    'MAX_HII_QUESTIONS 768',
    'NAV_COND_UNKNOWN',
    'HII_GRAPH_NAV_FORM_ID=0x',
    'HII_GRAPH_NAV_QUESTION_ID=0x',
    'HII_GRAPH_NAV_VARSTORE_ID=0x',
    'HII_GRAPH_NAV_VAR_INFO=0x',
    'HII_GRAPH_NAV_QUESTION_FLAGS=0x',
    'HII_GRAPH_NAV_CONDITION_FLAGS=0x',
    'HII_GRAPH_SPEECH_DIGITS=PASS',
    'HII_GRAPH_NAV_WHERE_AM_I_KEY=PASS',
    'HII_GRAPH_NAV_WHERE_AM_I_64=PASS',
    'g_nav_where_text[QEV_NAV_TEXT_MAX + 1u]',
    'HII_GRAPH_NAV_POSITION_KEY=PASS',
    'HII_GRAPH_NAV_POSITION_SPEECH=PASS',
    'HII_GRAPH_NAV_STAGED_EDIT_MODE=PASS',
    'HII_GRAPH_NAV_STAGED_NUMERIC_MODE=PASS',
    'HII_GRAPH_NAV_STAGED_NUMERIC=PASS',
    'HII_GRAPH_NAV_SPECIALIZED_METADATA=PASS',
    'HII_GRAPH_NAV_QUESTION_FLAGS_SEMANTICS=PASS',
    'HII_GRAPH_NAV_CALLBACK_AWARE=PASS',
    'HII_GRAPH_NAV_READ_ONLY_PREVIEW=BLOCKED',
    'NAV_Q_READ_ONLY',
    'NAV_Q_CALLBACK',
    'NAV_Q_RESET_REQUIRED',
    'NAV_Q_RECONNECT_REQUIRED',
    'NAV_Q_OPTIONS_ONLY',
    'nav_append_question_flags',
    'HII_GRAPH_NAV_PASSWORD_PRIVACY=PASS',
    'HII_GRAPH_NAV_PASSWORD_REDACTION=PASS',
    'HII_GRAPH_NAV_LIVE_PASSWORD_REDACTION=PASS',
    'HII_GRAPH_NAV_STAGED_CONDITION_EVAL=PASS',
    'HII_GRAPH_NAV_STAGED_CONDITION_VALUE=PASS',
    'HII_GRAPH_NAV_STAGED_DEPENDENCY_REFRESH=PASS',
    'HII_GRAPH_NAV_CONTROL_DETAILS=PASS',
    'nav_prompt_metadata_set',
    'nav_build_control_detail_speech',
    'nav_stage_adjust_numeric',
    'g_nav_prompt_min_value',
    'g_nav_prompt_max_value',
    'g_nav_prompt_step_value',
    'g_nav_prompt_min_size',
    'g_nav_prompt_max_size',
    'g_nav_prompt_max_containers',
    "key.unicode_char == (u16)'l'",
    "key.unicode_char == (u16)'+'",
    "key.unicode_char == (u16)'-'",
    'HII_GRAPH_NAV_STAGED_EDIT=PASS',
    'HII_GRAPH_NAV_STAGED_ONEOF=PASS',
    'HII_GRAPH_NAV_STAGED_CHECKBOX=PASS',
    'HII_GRAPH_NAV_STAGED_DISCARD=PASS',
    'HII_GRAPH_NAV_STAGED_STATUS_SPEECH=PASS',
    'HII_GRAPH_NAV_STAGED_CURRENT_DISCARD=PASS',
    'nav_stage_count',
    'nav_stage_discard_prompt',
    'nav_build_edit_status_speech',
    "key.unicode_char == (u16)'m'",
    "key.unicode_char == (u16)'z'",
    'HII_GRAPH_NAV_SAVE_BLOCKED=PASS',
    'HII_GRAPH_NAV_NO_FIRMWARE_WRITE=PASS',
    'HII_GRAPH_NAV_CONFIG_DRIVER_HANDLE=PASS',
    'HII_GRAPH_NAV_CONFIG_ACCESS_DISCOVERY=PASS',
    'HII_GRAPH_NAV_CONFIG_ROUTING_DISCOVERY=PASS',
    'HII_GRAPH_NAV_COMMIT_PATH_DISCOVERY=PASS',
    'HII_GRAPH_NAV_ROUTE_CONFIG_NOT_INVOKED=PASS',
    'nav_discover_config_commit_path',
    'g_hii_config_access_guid',
    'g_hii_config_routing_guid',
    'get_package_list_handle',
    'nav_stage_cycle_oneof',
    'nav_stage_toggle_checkbox',
    'nav_stage_clear_all',
    'nav_effective_value_text',
    'MAX_HII_STAGED_VALUES 64',
    "key.unicode_char == (u16)'o'",
    "key.unicode_char == (u16)'d'",
    "key.unicode_char == (u16)'s'",
    'key.unicode_char == 0x0020u',
    'nav_build_position_speech',
    'nav_append_decimal',
    'nav_append_u64_decimal',
    "key.unicode_char == (u16)'p'",
    'HII_GRAPH_NAV_WHERE_AM_I=PASS',
    'HII_GRAPH_NAV_WHERE_AM_I_CONTEXT=PASS',
    'nav_build_where_am_i_speech',
    'g_nav_where_text',
    'HII_GRAPH_NAV_LIVE_REFRESH=PASS',
    'HII_GRAPH_NAV_REFRESH_FOCUS_RESTORED=PASS',
    'HII_GRAPH_NAV_REFRESH_FOCUS_FALLBACK=PASS',
    'nav_refresh_current_form',
    'nav_find_question_index',
    "key.unicode_char == (u16)'w'",
    "key.unicode_char == (u16)'f'",
    "key.unicode_char == (u16)'b'",
    "key.unicode_char == (u16)'x'",
    "key.unicode_char == (u16)'c'",
    "key.unicode_char == (u16)'e'",
    'NAV_GROUP_BUTTON',
    'NAV_GROUP_CHECKBOX',
    'NAV_GROUP_CHOICE',
    'NAV_GROUP_EDITABLE',
    'nav_find_group',
    'nav_find_form',
    'MAX_HII_NAV_PROMPTS 240',
    'g_nav_prompt_overflow',
    'NAV_COND_SUPPRESS',
    'NAV_COND_GRAY',
    'NAV_COND_DISABLE',
    'ifr_condition_flag',
    'scope_condition_stack',
    'qev_digit_unit_count',
    'qev_digit_units',
    'g_nav_prompt_varstore_ids',
    'g_nav_prompt_var_infos',
    'g_nav_prompt_question_flags',
    "(ch >= (u16)'0' && ch <= (u16)'9')",
    "key.unicode_char == (u16)'h'",
    "key.unicode_char == (u16)'v'",
    'nav_read_scalar_value',
    'nav_format_u64',
    'nav_option_add',
    'nav_option_for_value',
    'nav_live_value_text',
    'HII_GRAPH_NAV_CHECKBOX_STATE=PASS',
    '"checked"',
    '"not checked"',
    'nav_option_value',
    'op == 0x09u',
    'nav_varstore_add',
    'op == 0x24u',
    'op == 0x26u',
    'get_variable_fn',
    'HII_GRAPH_NAV_PROFILE=M1603QA_BIOS_308',
    'HII_GRAPH_NAV_PACKAGE_GUID_MATCH=PASS',
    'HII_GRAPH_NAV_ROOT_FORM_2710=PASS',
    'HII_GRAPH_NAV_M1603QA_ROOT_FILTER=PASS',
    'HII_GRAPH_NAV_FORM_AWARE=PASS',
    'HII_GRAPH_NAV_FORM_LOAD=PASS',
    'HII_GRAPH_NAV_FORM_ENTER=PASS',
    'HII_GRAPH_NAV_FORM_BACK=PASS',
    'HII_GRAPH_NAV_FORM_TITLE_SPEECH=PASS',
    'HII_GRAPH_NAV_READ_ONLY_ACTION=BLOCKED',
    'HII_GRAPH_NAV_SELF_REF_ACTION=BLOCKED',
    'HII_GRAPH_NAV_DISABLED_ACTION=BLOCKED',
    'HII_GRAPH_NAV_CONDITIONAL_ACTION=BLOCKED',
    'g_m1603qa_308_setup_package_list_guid',
    'guid_bytes_equal',
    'nav_get_form_title',
    'nav_load_form',
    'g_nav_ref_form_ids',
    'g_nav_form_history',
    'key.unicode_char == 0x000du',
    'u16 help_token = oplen >= 6u ? rd16(q + 4) : 0u',
    'key.scan_code == 0x0001u',
    'key.scan_code == 0x0002u',
    'key.scan_code == 0x0003u',
    'key.scan_code == 0x0004u',
    'key.scan_code == 0x0005u',
    'key.scan_code == 0x0006u',
    'key.scan_code == 0x0009u',
    'key.scan_code == 0x000au',
    'key.unicode_char == 0x0009u',
)
missing = [needle for needle in required if needle not in text]
assert not missing, f"missing navigation contract markers: {missing}"

mask_start = text.index("#define NAV_REQUIRED_MASK")
mask_end = text.index("#endif", mask_start)
mask = text[mask_start:mask_end]
for required_name in (
    "NAV_SEEN_UP",
    "NAV_SEEN_DOWN",
    "NAV_SEEN_R",
    "NAV_SEEN_HOME",
    "NAV_SEEN_END",
    "NAV_SEEN_PAGE_UP",
    "NAV_SEEN_PAGE_DOWN",
):
    assert required_name in mask, required_name

# New ergonomic aliases must not weaken or silently replace the established
# physical-proof key set.
assert "LEFT" not in mask
assert "RIGHT" not in mask
assert "TAB" not in mask

# This repository evolves only voice/navigation. Guard the source contract from
# accidentally taking ownership of the preserved boot payload.
assert "BOOTX64.EFI" not in text
assert "KERNEL.BIN" not in text



# Password fields must be redacted before any VarStore scalar read attempt.
ev_start = text.index("static int nav_effective_value_text")
ev_end = text.index("static int nav_stage_cycle_oneof", ev_start)
ev = text[ev_start:ev_end]
assert 'op == 0x08u' in ev
assert '"protected"' in ev
assert ev.index('op == 0x08u') < ev.index('nav_effective_scalar_value')

# Preview values must participate in IFR conditional evaluation before live storage.
rq_start = text.index("static int nav_read_question_value")
rq_end = text.index("static int nav_eval_condition_expression", rq_start)
rq = text[rq_start:rq_end]
assert "nav_stage_find" in rq and "nav_find_question" in rq
assert rq.index("nav_stage_find") < rq.index("nav_find_question")



# RAM preview must never edit a question marked EFI_IFR_FLAG_READ_ONLY.
ss_start = text.index("static int nav_stage_set")
ss_end = text.index("static void nav_stage_clear_all", ss_start)
ss = text[ss_start:ss_end]
assert "NAV_Q_READ_ONLY" in ss
assert "HII_GRAPH_NAV_READ_ONLY_PREVIEW=BLOCKED" in ss
# Numeric (EFI_IFR_NUMERIC, 0x07) must use the same RAM-only staging path.
assert "g_nav_prompt_opcodes[prompt_index] != 0x07u" in ss



# HII text may be retained beyond one DMA chunk, but each low-level DMA build
# remains bounded to 32 characters and navigation uses the phrase queue.
norm_start = text.index("static int normalize_prompt")
norm_end = text.index("static int get_hii_string", norm_start)
norm = text[norm_start:norm_end]
assert "QEV_NAV_TEXT_MAX" in norm

dma_start = text.index("static int speech_dma_begin")
dma_end = text.index("static int speech_dma_poll", dma_start)
dma = text[dma_start:dma_end]
assert "text_count > 32u" in dma

wait_start = text.index("static int wait_navigation_keys")
wait_end = text.index("#endif", wait_start)
wait = text[wait_start:wait_end]
assert "speech_phrase_begin(speech_text, speech_length)" in wait
assert "speech_phrase_poll(1000u, &progressed)" in wait
assert "speech_phrase_cancel()" in wait
assert "HII_GRAPH_NAV_SPEECH_INTERRUPT_EARLY=PASS" in wait
assert wait.index("HII_GRAPH_NAV_SPEECH_INTERRUPT_EARLY=PASS") < wait.index("nav_build_focus_speech")
assert "HII_GRAPH_NAV_EXIT=BLOCKED_INCOMPLETE" not in wait
assert "HII_GRAPH_NAV_DIRECTIONAL_ALIASES=PASS" not in wait
assert "HII_GRAPH_NAV_SIMPLE_EXIT=PASS" in wait
assert "HII_GRAPH_NAV_ENTER_CONTEXT_ACTION=PASS" in wait
assert "NAV_COND_DISABLE" in wait

run_start = text.index("static int run_speech_dma")
run_end = text.index("static u16 rd16", run_start)
run = text[run_start:run_end]
assert "speech_phrase_begin" in run and "speech_phrase_poll" in run



# Common-word pronunciation must be attempted only at word boundaries and
# unknown words must retain the proven letter-name fallback.
sd_start = text.index("static int speech_dma_begin")
sd_end = text.index("static int speech_dma_poll", sd_start)
sd = text[sd_start:sd_end]
assert "speech_lookup_phrase(text, text_count" in sd
assert "speech_lookup_word(text + i, word_length" in sd
assert sd.index("speech_lookup_phrase(text, text_count") < sd.index("speech_lookup_word(text + i, word_length")
assert "(i == 0u || text[i - 1u] == ' ')" in sd
assert "qev_letter_unit_count" in sd and "qev_digit_unit_count" in sd
assert sd.index("speech_lookup_word(text + i, word_length") < sd.index("qev_letter_unit_count")

print("SCREEN_READER_NAVIGATION_SOURCE_CONTRACT=PASS")
