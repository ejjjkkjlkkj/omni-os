#!/usr/bin/env python3
from __future__ import annotations
import hashlib, importlib.util, struct, sys
from pathlib import Path

TEXT_RVA=0x1000
DATA_RVA=0x8000
WAIT_REPEAT_KEY=False
WAIT_DOWN_PROBE=False
WAIT_DOWN_SPEAK=False
WAIT_UP_PROBE=False
WAIT_UP_SPEAK=False
WAIT_DOWN_COMMIT=False
WAIT_DOWN_CANCEL=False
EFI_VARSTORE_ONLY=False
# Writable HII commits are verified after reboot against the same isolated OVMF VARS image.
PLATFORM_TARGET_ONLY=False

ROOT=Path(__file__).resolve().parents[2]
SPEECH_BUILDER=ROOT/'boot'/'uefi-hii-option-speech-v1'/'build_uefi_hii_option_speech.py'

def load_module(name: str, path: Path):
 spec=importlib.util.spec_from_file_location(name,path)
 if spec is None or spec.loader is None:
  raise SystemExit(f'cannot load {path}')
 module=importlib.util.module_from_spec(spec)
 spec.loader.exec_module(module)
 return module

speech=load_module('qevarynx_proven_hii_option_speech_source',SPEECH_BUILDER)
# The shared allophone bank grew beyond the legacy 128-page arena.
# Keep the current-value reader below 4 GiB, but reserve 1 MiB so the complete
# first-party unit bank fits without truncation or runtime reallocation.
DMA_PAGES=max(speech.DMA_PAGES,256)
PCM_OFF=speech.PCM_OFF
LETTER_UNITS=speech.LETTER_UNITS
UNIT_LAYOUT=speech.UNIT_LAYOUT

MARKS={
 'start': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATE=START\r\nDOMAIN=PRE_OS_UEFI\r\nEND\r\n',
 'database': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_DATABASE_PROTOCOL=PASS\r\nEND\r\n',
 'string_protocol': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_STRING_PROTOCOL=PASS\r\nEND\r\n',
 'direct_export': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_EXPORT_ALL_PACKAGE_LISTS=PASS\r\nEND\r\n',
 'forms_handle': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_FORMS_HANDLE=LEGACY_NOT_USED\r\nEND\r\n',
 'strings_package': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_STRINGS_PACKAGE=PASS\r\nEND\r\n',
 'strings_retry': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_STRINGS_PACKAGE_RETRY=PASS\r\nEND\r\n',
 'first_handle': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_FIRST_HANDLE_NONZERO=PASS\r\nEND\r\n',
 'static_empty': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_BUFFER_NOT_WRITTEN\r\nEND\r\n',
 'handle_export': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_HANDLE_EXPORT=PASS\r\nEND\r\n',
 'export_size_ok': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_HANDLE_EXPORT_SIZE=PASS\r\nEND\r\n',
 'export_size_invalid': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_HANDLE_EXPORT_SIZE=EFI_INVALID_PARAMETER\r\nEND\r\n',
 'export_size_not_found': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_HANDLE_EXPORT_SIZE=EFI_NOT_FOUND\r\nEND\r\n',
 'export_fetch_invalid': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_HANDLE_EXPORT_FETCH=EFI_INVALID_PARAMETER\r\nEND\r\n',
 'export_fetch_not_found': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_HANDLE_EXPORT_FETCH=EFI_NOT_FOUND\r\nEND\r\n',
 'forms_package_seen': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_FORMS_PACKAGE=PASS\r\nEND\r\n',
 'ifr': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nIFR_QUESTION_PROMPT_STRING_ID=PASS\r\nEND\r\n',
 'language': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_LANGUAGE=PASS\r\nEND\r\n',
 'prefix': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nQUESTION_TEXT=',
 'suffix': b'\r\nHII_QUESTION_STRING=PASS\r\nEND\r\n',
 'selected_text_prefix': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSELECTED_OPTION_TEXT=',
 'selected_text_done': b'\r\nSELECTED_OPTION_STRING=PASS\r\nSELECTED_OPTION_LABEL_BINDING=PASS\r\nEND\r\n',
 'selected_string_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=SELECTED_OPTION_STRING_NOT_RESOLVED\r\nEND\r\n',
 'meta_qid': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nQUESTION_ID_LE_HEX=',
 'meta_varstore': b'\r\nVARSTORE_ID_LE_HEX=',
 'meta_varinfo': b'\r\nVARSTORE_INFO_LE_HEX=',
 'meta_qflags': b'\r\nQUESTION_FLAGS_HEX=',
 'meta_oneof': b'\r\nONEOF_FLAGS_HEX=',
 'meta_done': b'\r\nQUESTION_METADATA=PASS\r\nEND\r\n',
 'vs_opcode': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nVARSTORE_OPCODE_HEX=',
 'vs_size': b'\r\nVARSTORE_SIZE_LE_HEX=',
 'vs_attrs': b'\r\nVARSTORE_ATTRIBUTES_LE_HEX=',
 'vs_guid': b'\r\nVARSTORE_GUID_RAW_HEX=',
 'vs_match': b'\r\nVARSTORE_DEFINITION_MATCH=PASS\r\nEND\r\n',
 'vs_name_match': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nVARSTORE_NAME_CONFIG_MATCH=PASS\r\nEND\r\n',
 'routing': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_CONFIG_ROUTING_PROTOCOL=PASS\r\nEND\r\n',
 'efi_var_name': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nEFI_VARSTORE_NAME_UTF16=PASS\r\nEND\r\n',
 'efi_var_get': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nEFI_VARSTORE_GET_VARIABLE=PASS\r\nEND\r\n',
 'handle_list': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_HANDLE_LIST=PASS\r\nEND\r\n',
 'cfg_access': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_CONFIG_ROUTING_EXTERNAL_CALLER=PASS\r\nEND\r\n',
 'extract': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nCONFIG_ROUTING_EXPORT_CONFIG=PASS\r\nEND\r\n',
 'to_block': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nCONFIG_TO_BLOCK=PASS\r\nEND\r\n',
 'cur_width': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nCURRENT_VALUE_BYTES_HEX=',
 'cur_raw': b'\r\nCURRENT_VALUE_RAW8_HEX=',
 'cur_done': b'\r\nBUFFER_VARSTORE_MATCH=PASS\r\nCURRENT_VALUE_READ=PASS\r\nEND\r\n',
 'sel_token': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSELECTED_OPTION_STRING_ID_LE_HEX=',
 'sel_type': b'\r\nSELECTED_OPTION_TYPE_HEX=',
 'sel_raw': b'\r\nSELECTED_OPTION_VALUE_RAW8_HEX=',
 'sel_done': b'\r\nSELECTED_OPTION_CHILD_MATCH=PASS\r\nSELECTED_OPTION_CURRENT_MATCH=PASS\r\nEND\r\n',
 'no_protocol': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_PROTOCOL_NOT_FOUND\r\nEND\r\n',
 'list_size_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_SIZE_QUERY_FAILED\r\nEND\r\n',
 'list_fetch_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_FAILED\r\nEND\r\n',
 'list_fetch_invalid': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_EFI_INVALID_PARAMETER\r\nEND\r\n',
 'list_fetch_not_found': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_EFI_NOT_FOUND\r\nEND\r\n',
 'list_fetch_bts_twice': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_FETCH_BUFFER_TOO_SMALL_TWICE\r\nEND\r\n',
 'list_static_small': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LIST_STATIC_BUFFER_TOO_SMALL\r\nEND\r\n',
 'no_handle': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_FORMS_AND_STRINGS_PACKAGE_LIST_NOT_FOUND\r\nEND\r\n',
 'alloc': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=POOL_ALLOC_FAILED\r\nEND\r\n',
 'export': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_HANDLE_EXPORT_FAILED\r\nEND\r\n',
 'ifr_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=IFR_QUESTION_TOKEN_NOT_FOUND\r\nEND\r\n',
 'lang_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_LANGUAGE_NOT_FOUND\r\nEND\r\n',
 'string_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HII_QUESTION_STRING_NOT_RESOLVED\r\nEND\r\n',
 'speech_char': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nEVENT=CURRENT_OPTION_GRAPHEME_ACCEPTED\r\nEND\r\n',
 'speech_hii': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nCURRENT_OPTION_SOURCE=LIVE_SELECTED_HII_LABEL\r\nCURRENT_OPTION_SOURCE=PASS\r\nEND\r\n',
 'spoken_prefix': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nCURRENT_OPTION_SPOKEN_PREFIX=',
 'spoken_prefix_done': b'\r\nCURRENT_OPTION_SPOKEN_PREFIX=PASS\r\nEND\r\n',
 'controller': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHDA_CONTROLLER_CODEC=PASS\r\nEND\r\n',
 'topology': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nAFG_RUNTIME_DISCOVERY=PASS\r\nAUTO_DAC_WIDGET=PASS\r\nAUTO_OUTPUT_PIN_WIDGET=PASS\r\nAUTO_PIN_TO_DAC_DIRECT_ROUTE=PASS\r\nNO_FIXED_WIDGET_NIDS=PASS\r\nEND\r\n',
 'policy': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nPIN_CONFIG_DEFAULT=PASS\r\nPIN_CAPABILITIES_QUERY=PASS\r\nEAPD_IF_SUPPORTED=PASS\r\nDAC_OUTPUT_AMP_VERIFY=PASS\r\nEND\r\n',
 'dma': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nUNIT_BANK_COPY=PASS\r\nBDL_RUNTIME_TEXT_SCHEDULE=PASS\r\nEND\r\n',
 'codec': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nCODEC_DAC_STREAM=PASS\r\nCODEC_PIN_OUTPUT=PASS\r\nEND\r\n',
 'stream': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nOUTPUT_STREAM_DESCRIPTOR=PASS\r\nFORMAT_48K_S16_STEREO=PASS\r\nBDL_ENTRIES=RUNTIME\r\nEND\r\n',
 'text_ready': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nEVENT=CURRENT_OPTION_TEXT_COMMIT\r\nTEXT_BUFFER=PASS\r\nBDL_RUNTIME_TEXT_SCHEDULE=PASS\r\nEND\r\n',
 'progress': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nLPIB_PROGRESS=PASS\r\nCURRENT_OPTION_SPEECH_HDA=PASS\r\nEND\r\n',
 'repeat_wait': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nACCESSIBILITY_REPEAT_KEY=WAIT_R\r\nEND\r\n',
 'repeat_accept': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nACCESSIBILITY_REPEAT_KEY=R\r\nACCESSIBILITY_REPEAT_KEY=PASS\r\nEND\r\n',
 'nav_wait': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_NAVIGATION_KEY=WAIT_DOWN\r\nEND\r\n',
 'nav_accept': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_NAVIGATION_KEY=DOWN\r\nHII_NAVIGATION_KEY=PASS\r\nEND\r\n',
 'nav_token': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nNAV_NEXT_OPTION_STRING_ID_LE_HEX=',
 'nav_type': b'\r\nNAV_NEXT_OPTION_TYPE_HEX=',
 'nav_raw': b'\r\nNAV_NEXT_OPTION_VALUE_RAW8_HEX=',
 'nav_done': b'\r\nNAV_NEXT_OPTION_DIRECT_CHILD=PASS\r\nEND\r\n',
 'nav_focus_activate': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nNAV_FOCUS_OPTION_TOKEN_ACTIVATED=PASS\r\nEND\r\n',
 'nav_focus_text_prefix': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nNAV_FOCUS_OPTION_TEXT=',
 'nav_focus_text_done': b'\r\nNAV_FOCUS_OPTION_STRING=PASS\r\nNAV_FOCUS_LABEL_BINDING=PASS\r\nEND\r\n',
 'nav_speech_hii': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nNAV_FOCUS_SOURCE=LIVE_DIRECT_SIBLING_HII_LABEL\r\nNAV_FOCUS_SOURCE=PASS\r\nEND\r\n',
 'nav_speech_char': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nEVENT=NAV_FOCUS_GRAPHEME_ACCEPTED\r\nEND\r\n',
 'nav_text_ready': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nEVENT=NAV_FOCUS_TEXT_COMMIT\r\nTEXT_BUFFER=PASS\r\nBDL_RUNTIME_TEXT_SCHEDULE=PASS\r\nEND\r\n',
 'nav_progress': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nLPIB_PROGRESS=PASS\r\nNAV_FOCUS_SPEECH_HDA=PASS\r\nEND\r\n',
 'nav_spoken_prefix': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nNAV_FOCUS_SPOKEN_PREFIX=',
 'nav_spoken_prefix_done': b'\r\nNAV_FOCUS_SPOKEN_PREFIX=PASS\r\nEND\r\n',
 'nav_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=NAV_NEXT_OPTION_NOT_FOUND\r\nEND\r\n',
 'up_wait': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_NAVIGATION_KEY=WAIT_UP\r\nEND\r\n',
 'up_accept': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_NAVIGATION_KEY=UP\r\nHII_NAVIGATION_KEY=PASS\r\nEND\r\n',
 'up_token': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nNAV_PREV_OPTION_STRING_ID_LE_HEX=',
 'up_type': b'\r\nNAV_PREV_OPTION_TYPE_HEX=',
 'up_raw': b'\r\nNAV_PREV_OPTION_VALUE_RAW8_HEX=',
 'up_done': b'\r\nNAV_PREV_OPTION_DIRECT_CHILD=PASS\r\nNAV_PREV_OPTION_WRAP_POLICY=PASS\r\nEND\r\n',
 'platform_target': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nPLATFORM_WRITABLE_TARGET=PASS\r\nEND\r\n',
 'commit_wait': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_COMMIT_KEY=WAIT_ENTER\r\nEND\r\n',
 'commit_accept': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_COMMIT_KEY=ENTER\r\nHII_COMMIT_KEY=PASS\r\nEND\r\n',
 'commit_stage': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_COMMIT_TARGET_STAGED=PASS\r\nEND\r\n',
 'commit_request': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_COMMIT_CONFIG_REQUEST=PASS\r\nEND\r\n',
 'commit_block': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nBLOCK_TO_CONFIG=PASS\r\nEND\r\n',
 'commit_route': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nROUTE_CONFIG=PASS\r\nEND\r\n',
 'commit_verify': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nPOST_COMMIT_REREAD=PASS\r\nPOST_COMMIT_OPTION_MATCH=PASS\r\nEND\r\n',
 'commit_confirm': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nCOMMIT_CONFIRMATION_SPEECH_HDA=PASS\r\nEND\r\n',
 'cancel_wait': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_CANCEL_KEY=WAIT_ESC\r\nEND\r\n',
 'cancel_accept': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHII_CANCEL_KEY=ESC\r\nHII_CANCEL_KEY=PASS\r\nHII_CANCEL_NO_ROUTE=PASS\r\nEND\r\n',
 'commit_request_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=COMMIT_CONFIG_REQUEST_FAILED\r\nEND\r\n',
 'commit_block_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=BLOCK_TO_CONFIG_FAILED\r\nEND\r\n',
 'commit_route_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=ROUTE_CONFIG_FAILED\r\nEND\r\n',
 'commit_verify_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=POST_COMMIT_REREAD_MISMATCH\r\nEND\r\n',
 'done': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=PASS\r\nEND\r\n',
 'controller_preferred': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHDA_CONTROLLER_SELECTION=PREFERRED_AMD_1022_15E3\r\nEND\r\n',
 'controller_generic': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nHDA_CONTROLLER_SELECTION=GENERIC_CLASS_0403\r\nEND\r\n',
 'no_hda': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_PCI_NOT_FOUND\r\nEND\r\n',
 'bad_hda': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=HDA_CONTROLLER_OR_CODEC_FAILED\r\nEND\r\n',
 'dma_alloc': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=DMA_ALLOC_FAILED\r\nEND\r\n',
 'verb': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=CODEC_VERB_FAILED\r\nEND\r\n',
 'stream_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=STREAM_DMA_NO_PROGRESS\r\nEND\r\n',
 'speech_text_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=CURRENT_OPTION_TEXT_NOT_SPEAKABLE\r\nEND\r\n',
 'topology_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=AUTO_OUTPUT_ROUTE_DISCOVERY_FAILED\r\nEND\r\n',
 'policy_fail': b'QEVARYNOX-UEFI-HII-CURRENT-OPTION-SPEECH-V1\r\nSTATUS=BLOCKED\r\nREASON=OUTPUT_POLICY_FAILED\r\nEND\r\n',

}

class Code:
 def __init__(self):
  self.data=bytearray(); self.labels={}; self.fix=[]
 def pos(self): return len(self.data)
 def emit(self,b): self.data += bytes(b)
 def label(self,n): self.labels[n]=self.pos()
 def rel32(self,op,label):
  self.emit(op); off=self.pos(); self.emit(b'\0'*4); self.fix.append((off,self.pos(),label,4))
 def rel8(self,op,label):
  self.emit(bytes((op,0))); self.fix.append((self.pos()-1,self.pos(),label,1))
 def data_disp(self,opcode,off):
  self.emit(opcode); after=TEXT_RVA+self.pos()+4
  self.emit(struct.pack('<i',DATA_RVA+off-after))
 def lea_rax_data(self,off): self.data_disp(b'\x48\x8d\x05',off)
 def lea_rcx_data(self,off): self.data_disp(b'\x48\x8d\x0d',off)
 def lea_rdx_data(self,off): self.data_disp(b'\x48\x8d\x15',off)
 def lea_rsi_data(self,off): self.data_disp(b'\x48\x8d\x35',off)
 def lea_rdi_data(self,off): self.data_disp(b'\x48\x8d\x3d',off)
 def lea_r8_data(self,off): self.data_disp(b'\x4c\x8d\x05',off)
 def lea_r9_data(self,off): self.data_disp(b'\x4c\x8d\x0d',off)
 def mov_r13_data(self,off): self.data_disp(b'\x4c\x8b\x2d',off)
 def patch(self):
  for off,after,label,width in self.fix:
   disp=self.labels[label]-after
   if width==4: struct.pack_into('<i',self.data,off,disp)
   else:
    if not -128 <= disp <= 127: raise SystemExit(f'short jump overflow {label}: {disp}')
    self.data[off]=disp & 0xff

def put(b,o,f,*v): struct.pack_into(f,b,o,*v)

def build():
 pcm=speech.make_pcm()
 data=bytearray(0x216000)
 L={
  'db_guid':0,'str_guid':16,'dbptr':32,'strptr':40,
  'handles_size':48,'handles_ptr':56,'pkg_size':64,'pkg_ptr':72,
  'langs_size':80,'langs_ptr':88,'string_size':96,'string_ptr':104,
  'token':112,'temp_handle':120,'handle_cursor':128,'handles_remaining':136,
  'forms_ptr':144,'strings_ptr':152,'list_len':160,'ifr_next_ptr':168,'ifr_next_remaining':176,
  'strings_first':184,'list_start':192,'global_remaining':200,
  'question_id':208,'varstore_id':210,'varstore_info':212,'question_flags':214,'oneof_flags':215,
  'varstore_opcode':216,'varstore_size':218,'varstore_attrs':220,'varstore_guid':224,
  'cfgacc_guid':240,'routing_guid':256,'routing_ptr':272,'config_ptr':280,
  'driver_handle':288,'config_progress':296,'results':304,'match_pkg_size':312,
  'matched_hii_handle':320,'block_size':328,'current_width':336,'current_raw':344,'config_boundary':352,
  'varstore_name_ptr':360,'varstore_name_remaining':368,
  'question_ptr':376,'selected_option_token':384,'selected_option_type':386,'selected_option_raw':392,
  'string_mode':400,
  'maxaddr':408,'dac_nid':416,'pin_nid':420,'speech_text_source':424,
  'textbuf':432,'text_count':452,'keybuf':456,
  'selected_option_ptr':464,'nav_option_token':472,'nav_option_type':474,'nav_option_raw':480,
  'prev_option_ptr':488,'prev_wrap_flag':496,'conin_ptr':504,
  'commit_config':512,'commit_progress':520,'commit_done':528,
  'rt_ptr':536,'efi_var_size':544,
  'efi_name_utf16':0x210000,'commit_request_buf':0x212000,
  'handles_static':0x400,'pkg_static':0x1400,'match_pkg_static':0x101400,
  'current_data':0x201400,
 }
 struct.pack_into('<IHH8B',data,L['db_guid'],
  0xef9fc172,0xa1b2,0x4693,0xb3,0x27,0x6d,0x32,0xfc,0x41,0x60,0x42)
 struct.pack_into('<IHH8B',data,L['str_guid'],
  0x0fd96974,0x23aa,0x4cdc,0xb9,0xcb,0x98,0xd1,0x77,0x50,0x32,0x2a)
 struct.pack_into('<IHH8B',data,L['cfgacc_guid'],
  0x330d4706,0xf2a0,0x4e4f,0xa3,0x69,0xb6,0x6f,0xa8,0xd5,0x43,0x85)
 struct.pack_into('<IHH8B',data,L['routing_guid'],
  0x587e72d7,0xcc50,0x4f79,0x82,0x09,0xca,0x29,0x1f,0xc1,0xa1,0x0f)
 struct.pack_into('<Q',data,L['maxaddr'],0xffffffff)
 for n,m in MARKS.items():
  L[n]=len(data); data+=m
 L['commit_offset_utf16']=len(data); data+='&OFFSET='.encode('utf-16le')
 L['commit_width_utf16']=len(data); data+='&WIDTH='.encode('utf-16le')
 L['pcm']=len(data); data+=pcm

 c=Code()
 c.emit(b'\x53\x55\x56\x57\x41\x54\x41\x55\x41\x56\x41\x57')
 c.emit(b'\x4c\x8b\x7a\x60')  # r15=BootServices
 # Persist RuntimeServices and ConIn before RDX is repurposed as scratch.
 c.emit(b'\x48\x8b\x42\x58\x48\x8b\x4a\x30')
 c.lea_rdx_data(L['rt_ptr']); c.emit(b'\x48\x89\x02')
 c.lea_rdx_data(L['conin_ptr']); c.emit(b'\x48\x89\x0a')
 c.emit(b'\x48\x83\xec\x68\xfc')

 for p,v in ((0x3f9,0),(0x3fb,0x80),(0x3f8,3),(0x3f9,0),(0x3fb,3),(0x3fa,0xc7),(0x3fc,0x0b)):
  c.emit(b'\x66\xba'+struct.pack('<H',p)+b'\xb0'+bytes((v,))+b'\xee')

 def serial(n):
  c.lea_rdx_data(L[n]); c.emit(b'\xb9'+struct.pack('<I',len(MARKS[n])))
  c.rel32(b'\xe8','serial_emit')
 def zero_qword(off):
  c.lea_rdx_data(off); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')
 def alloc(size_reg_is_rbx,ptr_off):
  c.emit(b'\xb9\x04\x00\x00\x00') # EfiBootServicesData
  if size_reg_is_rbx: c.emit(b'\x48\x89\xda')
  c.lea_r8_data(ptr_off)
  c.emit(b'\x41\xff\x57\x40')
  c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_alloc')
 def verb_data(nid_off,value):
  c.emit(b'\x44\x89\xe0\xc1\xe0\x1c')
  c.lea_rdx_data(nid_off)
  c.emit(b'\x8b\x0a\xc1\xe1\x14\x09\xc8')
  c.emit(b'\x0d'+struct.pack('<I',value))
  c.rel32(b'\xe8','immediate')
  c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_verb')

 serial('start')

 # Locate HII Database protocol -> r12.
 c.lea_rcx_data(L['db_guid']); c.emit(b'\x31\xd2'); c.lea_r8_data(L['dbptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_protocol')
 c.lea_rdx_data(L['dbptr']); c.emit(b'\x4c\x8b\x22')
 c.emit(b'\x4d\x85\xe4'); c.rel32(b'\x0f\x84','fail_protocol')
 serial('database')

 # Locate HII String protocol -> r13.
 c.lea_rcx_data(L['str_guid']); c.emit(b'\x31\xd2'); c.lea_r8_data(L['strptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_protocol')
 c.lea_rdx_data(L['strptr']); c.emit(b'\x4c\x8b\x2a')
 c.emit(b'\x4d\x85\xed'); c.rel32(b'\x0f\x84','fail_protocol')
 serial('string_protocol')

 # Locate the global HII Config Routing protocol -> data pointer.
 c.lea_rcx_data(L['routing_guid']); c.emit(b'\x31\xd2'); c.lea_r8_data(L['routing_ptr'])
 c.emit(b'\x41\xff\x97\x40\x01\x00\x00')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_protocol')
 c.lea_rdx_data(L['routing_ptr']); c.emit(b'\x48\x83\x3a\x00'); c.rel32(b'\x0f\x84','fail_protocol')
 serial('routing')

 # Snapshot all live HII handles. This lets the selected exported package list
 # be mapped back to its EFI_HII_HANDLE and then to its DriverHandle.
 c.lea_rdx_data(L['handles_size']); c.emit(b'\x48\xc7\x02\x00\x10\x00\x00')
 c.emit(b'\x4c\x89\xe1\x31\xd2\x45\x31\xc0')
 c.lea_r9_data(L['handles_size'])
 c.lea_rax_data(L['handles_static']); c.emit(b'\x48\x89\x44\x24\x20')
 c.emit(b'\x41\xff\x54\x24\x18')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_list_fetch')
 c.lea_rdx_data(L['handles_size']); c.emit(b'\x48\x83\x3a\x08'); c.rel32(b'\x0f\x82','fail_no_handle')
 serial('handle_list')

 # Export the whole live HII database read-only in one atomic protocol call.
 # A bridge-owned 1 MiB buffer avoids the observed OVMF sizing/fetch race.
 c.lea_rdx_data(L['pkg_size'])
 c.emit(b'\x48\xc7\x02'+struct.pack('<I',0x100000))
 c.lea_rax_data(L['pkg_static'])
 c.lea_rdx_data(L['pkg_ptr']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x4c\x89\xe1\x31\xd2')
 c.lea_r8_data(L['pkg_size'])
 c.lea_r9_data(L['pkg_static'])
 c.emit(b'\x41\xff\x54\x24\x20')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_export')
 c.lea_rdx_data(L['pkg_size']); c.emit(b'\x48\x83\x3a\x18')
 c.rel32(b'\x0f\x82','fail_export')
 serial('direct_export')

 # Parse concatenated EFI_HII_PACKAGE_LIST_HEADER records.  Select one package
 # list containing both Forms and Strings so the IFR StringId and text share
 # the same package-list namespace.
 c.lea_rdx_data(L['pkg_ptr']); c.emit(b'\x48\x8b\x32')
 c.lea_rdx_data(L['pkg_size']); c.emit(b'\x48\x8b\x1a')
 c.label('direct_list_loop')
 c.emit(b'\x48\x83\xfb\x14'); c.rel32(b'\x0f\x82','fail_no_handle')
 c.lea_rdx_data(L['global_remaining']); c.emit(b'\x48\x89\x1a')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x89\x32')
 c.emit(b'\x8b\x46\x10\x83\xf8\x18'); c.rel32(b'\x0f\x82','fail_ifr')
 c.emit(b'\x48\x39\xd8'); c.rel32(b'\x0f\x87','fail_ifr')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x89\x02')
 zero_qword(L['forms_ptr']); zero_qword(L['strings_ptr'])
 c.emit(b'\x48\x8d\x7e\x14\x89\xc1\x83\xe9\x14')
 c.label('direct_pkg_loop')
 # The enclosing package-list length is already validated against the exported
 # database. If one child package has a truncated/invalid header, abandon only
 # this package list and continue global HII discovery instead of converting
 # unrelated later firmware package lists into a global IFR failure.
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x82','direct_list_next')
 c.emit(b'\x8b\x07\x89\xc2\x81\xe2\xff\xff\xff\x00')
 c.emit(b'\x89\xc5\xc1\xed\x18')
 c.emit(b'\x83\xfa\x04'); c.rel32(b'\x0f\x82','direct_list_next')
 c.emit(b'\x39\xca'); c.rel32(b'\x0f\x87','direct_list_next')
 c.emit(b'\x81\xfd\xdf\x00\x00\x00'); c.rel32(b'\x0f\x84','direct_list_done')
 c.emit(b'\x83\xfd\x02'); c.rel32(b'\x0f\x85','direct_not_forms')
 c.lea_rax_data(L['forms_ptr']); c.emit(b'\x48\x83\x38\x00'); c.rel32(b'\x0f\x85','direct_not_forms')
 c.emit(b'\x48\x89\x38')
 c.label('direct_not_forms')
 c.emit(b'\x83\xfd\x04'); c.rel32(b'\x0f\x85','direct_not_strings')
 c.lea_rax_data(L['strings_ptr']); c.emit(b'\x48\x83\x38\x00'); c.rel32(b'\x0f\x85','direct_not_strings')
 c.emit(b'\x48\x89\x38')
 c.label('direct_not_strings')
 c.emit(b'\x48\x01\xd7\x29\xd1'); c.rel32(b'\xe9','direct_pkg_loop')

 c.label('direct_list_done')
 c.lea_rdx_data(L['forms_ptr']); c.emit(b'\x48\x83\x3a\x00'); c.rel32(b'\x0f\x84','direct_list_next')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x83\x3a\x00'); c.rel32(b'\x0f\x84','direct_list_next')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x89\x32')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x02')
 c.lea_rdx_data(L['strings_first']); c.emit(b'\x48\x89\x02')
 serial('forms_package_seen'); serial('strings_package')
 c.lea_rdx_data(L['forms_ptr']); c.emit(b'\x48\x8b\x3a')
 c.emit(b'\x8b\x07\x89\xc2\x81\xe2\xff\xff\xff\x00')
 c.rel32(b'\xe9','forms_pkg')

 c.label('direct_list_next')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x8b\x32')
 c.lea_rdx_data(L['global_remaining']); c.emit(b'\x48\x8b\x1a')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x8b\x02')
 c.emit(b'\x48\x01\xc6\x48\x29\xc3')
 c.emit(b'\x48\x85\xdb'); c.rel32(b'\x0f\x85','direct_list_loop')
 c.rel32(b'\xe9','fail_no_handle')

 c.label('forms_pkg')
 # r9=first IFR opcode, r10d=bytes available in verified Forms package.
 c.emit(b'\x4c\x8d\x4f\x04\x41\x89\xd2\x41\x83\xea\x04')
 c.label('ifr_loop')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x82','prompt_package_done')
 c.emit(b'\x41\x0f\xb6\x01')       # eax=OpCode
 c.emit(b'\x41\x0f\xb6\x49\x01\x83\xe1\x7f') # ecx=Length
 # A malformed/truncated IFR tail in one firmware Forms package must not
 # abort discovery of the remaining independent HII package lists. r10d is
 # already bounded by the enclosing package length, so abandoning this Forms
 # package preserves memory safety while avoiding a false global failure.
 c.emit(b'\x83\xf9\x02'); c.rel32(b'\x0f\x82','prompt_package_done')
 c.emit(b'\x44\x39\xd1'); c.rel32(b'\x0f\x87','prompt_package_done')
 c.emit(b'\x3c\x05'); c.rel32(b'\x0f\x84','ifr_question')
 c.label('ifr_next')
 c.emit(b'\x49\x01\xc9\x41\x29\xca')
 c.rel32(b'\xe9','ifr_loop')

 c.label('ifr_question')
 c.lea_rax_data(L['string_mode']); c.emit(b'\xc6\x00\x00')
 c.lea_rdx_data(L['question_ptr']); c.emit(b'\x4c\x89\x0a')
 # EFI_IFR_ONE_OF: OpHeader(2) + Statement(Prompt,Help=4) + QuestionId(2)
 # + VarStoreId(2) + VarStoreInfo(2) + QuestionFlags(1) + OneOfFlags(1).
 c.emit(b'\x83\xf9\x0e'); c.rel32(b'\x0f\x82','ifr_next')
 c.emit(b'\x41\x0f\xb7\x41\x06'); c.lea_rdx_data(L['question_id']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb7\x41\x08')
 c.emit(b'\x66\x85\xc0'); c.rel32(b'\x0f\x84','ifr_next')
 c.lea_rdx_data(L['varstore_id']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb7\x41\x0a'); c.lea_rdx_data(L['varstore_info']); c.emit(b'\x66\x89\x02')
 if PLATFORM_TARGET_ONLY:
  # OVMF PlatformDxe MAIN_FORM_STATE.NextPreferredResolution: QID=2, VarStore=1, offset=32.
  c.lea_rax_data(L['question_id']); c.emit(b'\x66\x83\x38\x02'); c.rel32(b'\x0f\x85','ifr_next')
  c.lea_rax_data(L['varstore_id']); c.emit(b'\x66\x83\x38\x01'); c.rel32(b'\x0f\x85','ifr_next')
  c.lea_rax_data(L['varstore_info']); c.emit(b'\x66\x83\x38\x20'); c.rel32(b'\x0f\x85','ifr_next')
 c.emit(b'\x41\x0f\xb6\x41\x0c'); c.lea_rdx_data(L['question_flags']); c.emit(b'\x88\x02')
 c.emit(b'\x41\x0f\xb6\x41\x0d'); c.lea_rdx_data(L['oneof_flags']); c.emit(b'\x88\x02')
 c.emit(b'\x41\x0f\xb7\x41\x02')
 c.emit(b'\x66\x85\xc0'); c.rel32(b'\x0f\x84','ifr_next')
 c.lea_rdx_data(L['token']); c.emit(b'\x66\x89\x02')
 # Preserve the next IFR opcode so an unresolved Prompt StringId does not
 # terminate discovery; real firmware can contain sparse language strings.
 c.emit(b'\x4c\x89\xc8\x48\x01\xc8')
 c.lea_rdx_data(L['ifr_next_ptr']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x44\x89\xd0\x29\xc8')
 c.lea_rdx_data(L['ifr_next_remaining']); c.emit(b'\x89\x02')
 # Start every Prompt candidate at the first language package; failures can
 # walk sibling EFI_HII_PACKAGE_STRINGS records in the same package list.
 c.lea_rdx_data(L['strings_first']); c.emit(b'\x48\x8b\x02')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x89\x02')
 serial('ifr')

 c.label('resolve_string_package')
 # Resolve the IFR StringId directly inside the selected Strings package.
 # EFI_HII_SIBT_STRING_SCSU=0x10 / STRINGS_SCSU=0x12 and
 # EFI_HII_SIBT_STRING_UCS2=0x14 / STRINGS_UCS2=0x16 are decoded;
 # FONT variants, DUPLICATE, SKIP1/SKIP2 and EXT1/EXT2/EXT4 are traversed.
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32')
 c.emit(b'\x48\x85\xf6'); c.rel32(b'\x0f\x84','fail_string')
 c.emit(b'\x8b\x06\x25\xff\xff\xff\x00\x89\xc3')
 c.emit(b'\x83\xfb\x2f'); c.rel32(b'\x0f\x82','fail_string')
 c.emit(b'\x8b\x4e\x04\x83\xf9\x2f'); c.rel32(b'\x0f\x82','fail_language')
 c.emit(b'\x39\xd9'); c.rel32(b'\x0f\x87','fail_language')
 c.emit(b'\x80\x7e\x2e\x00'); c.rel32(b'\x0f\x84','fail_language')
 serial('language')
 c.emit(b'\x8b\x46\x08\x39\xc8'); c.rel32(b'\x0f\x82','fail_string')
 c.emit(b'\x39\xd8'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x48\x8d\x3c\x1e\x48\x01\xc6')
 c.emit(b'\x41\xbc\x01\x00\x00\x00')
 c.lea_rdx_data(L['token']); c.emit(b'\x44\x0f\xb7\x2a')

 c.label('string_block_loop')
 c.emit(b'\x48\x39\xfe'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x0f\xb6\x06\x84\xc0'); c.rel32(b'\x0f\x84','fail_string')
 for typ,label in ((0x10,'str_scsu1'),(0x11,'str_scsu1_font'),(0x12,'str_scsun'),(0x13,'str_scsun_font'),
                   (0x14,'str_ucs1'),(0x15,'str_ucs1_font'),(0x16,'str_ucsn'),(0x17,'str_ucsn_font'),
                   (0x20,'str_duplicate'),(0x21,'str_skip2'),(0x22,'str_skip1'),
                   (0x30,'str_ext1'),(0x31,'str_ext2'),(0x32,'str_ext4')):
  c.emit(b'\x3c'+bytes((typ,))); c.rel32(b'\x0f\x84',label)
 c.rel32(b'\xe9','fail_string')

 c.label('str_scsu1')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_scsu1_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x01'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('str_scsu1_found'); c.emit(b'\x48\x83\xc6\x01'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('str_scsu1_font')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_scsu1_font_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x02'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('str_scsu1_font_found'); c.emit(b'\x48\x83\xc6\x02'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('scan_scsu_single')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x80\x3a\x00'); c.rel32(b'\x0f\x84','scan_scsu_single_done')
 c.emit(b'\x48\xff\xc2'); c.rel32(b'\xe9','scan_scsu_single')
 c.label('scan_scsu_single_done'); c.emit(b'\x48\x8d\x72\x01'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_scsun'); c.emit(b'\x44\x0f\xb7\x76\x01\x48\x8d\x56\x03'); c.rel32(b'\xe9','multi_scsu_loop')
 c.label('str_scsun_font'); c.emit(b'\x44\x0f\xb7\x76\x02\x48\x8d\x56\x04')
 c.label('multi_scsu_loop')
 c.emit(b'\x45\x85\xf6'); c.rel32(b'\x0f\x84','multi_scsu_done')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','multi_scsu_found')
 c.label('multi_scsu_scan')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x80\x3a\x00'); c.rel32(b'\x0f\x84','multi_scsu_next')
 c.emit(b'\x48\xff\xc2'); c.rel32(b'\xe9','multi_scsu_scan')
 c.label('multi_scsu_next'); c.emit(b'\x48\xff\xc2\x41\xff\xc4\x41\xff\xce'); c.rel32(b'\xe9','multi_scsu_loop')
 c.label('multi_scsu_found'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','direct_scsu_found')
 c.label('multi_scsu_done'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_ucs1')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_ucs1_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x01'); c.rel32(b'\xe9','scan_ucs_single')
 c.label('str_ucs1_found'); c.emit(b'\x48\x83\xc6\x01'); c.rel32(b'\xe9','direct_ucs_found')
 c.label('str_ucs1_font')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','str_ucs1_font_found')
 c.emit(b'\x41\xff\xc4\x48\x8d\x56\x02'); c.rel32(b'\xe9','scan_ucs_single')
 c.label('str_ucs1_font_found'); c.emit(b'\x48\x83\xc6\x02'); c.rel32(b'\xe9','direct_ucs_found')
 c.label('scan_ucs_single')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x66\x83\x3a\x00'); c.rel32(b'\x0f\x84','scan_ucs_single_done')
 c.emit(b'\x48\x83\xc2\x02'); c.rel32(b'\xe9','scan_ucs_single')
 c.label('scan_ucs_single_done'); c.emit(b'\x48\x8d\x72\x02'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_ucsn'); c.emit(b'\x44\x0f\xb7\x76\x01\x48\x8d\x56\x03'); c.rel32(b'\xe9','multi_ucs_loop')
 c.label('str_ucsn_font'); c.emit(b'\x44\x0f\xb7\x76\x02\x48\x8d\x56\x04')
 c.label('multi_ucs_loop')
 c.emit(b'\x45\x85\xf6'); c.rel32(b'\x0f\x84','multi_ucs_done')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x84','multi_ucs_found')
 c.label('multi_ucs_scan')
 c.emit(b'\x48\x39\xfa'); c.rel32(b'\x0f\x83','fail_string')
 c.emit(b'\x66\x83\x3a\x00'); c.rel32(b'\x0f\x84','multi_ucs_next')
 c.emit(b'\x48\x83\xc2\x02'); c.rel32(b'\xe9','multi_ucs_scan')
 c.label('multi_ucs_next'); c.emit(b'\x48\x83\xc2\x02\x41\xff\xc4\x41\xff\xce'); c.rel32(b'\xe9','multi_ucs_loop')
 c.label('multi_ucs_found'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','direct_ucs_found')
 c.label('multi_ucs_done'); c.emit(b'\x48\x89\xd6'); c.rel32(b'\xe9','string_block_loop')

 c.label('str_duplicate')
 c.emit(b'\x45\x39\xec'); c.rel32(b'\x0f\x85','str_duplicate_skip')
 c.emit(b'\x44\x0f\xb7\x6e\x01\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_string')
 c.emit(b'\x41\xbc\x01\x00\x00\x00')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32\x8b\x46\x08\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_duplicate_skip'); c.emit(b'\x41\xff\xc4\x48\x83\xc6\x03'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_skip1'); c.emit(b'\x0f\xb6\x46\x01\x41\x01\xc4\x48\x83\xc6\x02'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_skip2'); c.emit(b'\x0f\xb7\x46\x01\x41\x01\xc4\x48\x83\xc6\x03'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext1'); c.emit(b'\x0f\xb6\x46\x02\x83\xf8\x03'); c.rel32(b'\x0f\x82','fail_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext2'); c.emit(b'\x0f\xb7\x46\x02\x83\xf8\x04'); c.rel32(b'\x0f\x82','fail_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')
 c.label('str_ext4'); c.emit(b'\x8b\x46\x02\x83\xf8\x06'); c.rel32(b'\x0f\x82','fail_string'); c.emit(b'\x48\x01\xc6'); c.rel32(b'\xe9','string_block_loop')

 c.label('direct_scsu_found')
 c.emit(b'\x80\x3e\x00'); c.rel32(b'\x0f\x84','fail_string')
 c.lea_rax_data(L['string_mode']); c.emit(b'\x80\x38\x01'); c.rel32(b'\x0f\x84','selected_scsu_found')
 # Preserve the resolved question pointer in data memory, not with PUSH, so all
 # nested UEFI protocol calls retain Microsoft x64 / UEFI 16-byte stack alignment.
 c.lea_rdx_data(L['string_ptr']); c.emit(b'\x48\x89\x32')
 c.rel32(b'\xe8','resolve_varstore'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','prompt_next')
 if PLATFORM_TARGET_ONLY:
  c.lea_rax_data(L['varstore_size']); c.emit(b'\x66\x83\x38\x24'); c.rel32(b'\x0f\x85','prompt_next')
  serial('platform_target')
 c.rel32(b'\xe8','read_buffer_current'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','prompt_next')
 if PLATFORM_TARGET_ONLY:
  # Platform target diagnostics expose the live 32-bit current value before option binding.
  c.rel32(b'\xe8','emit_question_meta'); c.rel32(b'\xe8','emit_varstore_meta'); c.rel32(b'\xe8','emit_current_meta')
 c.rel32(b'\xe8','resolve_selected_option'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','prompt_next')
 c.rel32(b'\xe8','emit_question_meta'); c.rel32(b'\xe8','emit_varstore_meta'); c.rel32(b'\xe8','emit_current_meta'); c.rel32(b'\xe8','emit_selected_meta')
 if WAIT_DOWN_PROBE or WAIT_DOWN_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL:
  c.rel32(b'\xe8','wait_down_key'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','resolve_next_option'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','emit_nav_meta')
  if WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL:
   c.lea_rax_data(L['nav_option_token']); c.emit(b'\x0f\xb7\x00'); c.lea_rdx_data(L['selected_option_token']); c.emit(b'\x66\x89\x02')
   serial('nav_focus_activate')
 elif WAIT_UP_PROBE or WAIT_UP_SPEAK:
  c.rel32(b'\xe8','wait_up_key'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','resolve_prev_option'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','emit_up_meta')
  if WAIT_UP_SPEAK:
   c.lea_rax_data(L['nav_option_token']); c.emit(b'\x0f\xb7\x00'); c.lea_rdx_data(L['selected_option_token']); c.emit(b'\x66\x89\x02')
   serial('nav_focus_activate')
 c.lea_rdx_data(L['string_ptr']); c.emit(b'\x48\x8b\x32')
 serial('prefix'); c.rel32(b'\xe8','serial_scsu_ascii')
 c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_string')
 serial('suffix'); c.rel32(b'\xe9','begin_selected_label')

 c.label('direct_ucs_found')
 c.emit(b'\x66\x83\x3e\x00'); c.rel32(b'\x0f\x84','fail_string')
 c.lea_rax_data(L['string_mode']); c.emit(b'\x80\x38\x01'); c.rel32(b'\x0f\x84','selected_ucs_found')
 c.lea_rdx_data(L['string_ptr']); c.emit(b'\x48\x89\x32')
 c.rel32(b'\xe8','resolve_varstore'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','prompt_next')
 if PLATFORM_TARGET_ONLY:
  c.lea_rax_data(L['varstore_size']); c.emit(b'\x66\x83\x38\x24'); c.rel32(b'\x0f\x85','prompt_next')
  serial('platform_target')
 c.rel32(b'\xe8','read_buffer_current'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','prompt_next')
 if PLATFORM_TARGET_ONLY:
  # Platform target diagnostics expose the live 32-bit current value before option binding.
  c.rel32(b'\xe8','emit_question_meta'); c.rel32(b'\xe8','emit_varstore_meta'); c.rel32(b'\xe8','emit_current_meta')
 c.rel32(b'\xe8','resolve_selected_option'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','prompt_next')
 c.rel32(b'\xe8','emit_question_meta'); c.rel32(b'\xe8','emit_varstore_meta'); c.rel32(b'\xe8','emit_current_meta'); c.rel32(b'\xe8','emit_selected_meta')
 if WAIT_DOWN_PROBE or WAIT_DOWN_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL:
  c.rel32(b'\xe8','wait_down_key'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','resolve_next_option'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','emit_nav_meta')
  if WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL:
   c.lea_rax_data(L['nav_option_token']); c.emit(b'\x0f\xb7\x00'); c.lea_rdx_data(L['selected_option_token']); c.emit(b'\x66\x89\x02')
   serial('nav_focus_activate')
 elif WAIT_UP_PROBE or WAIT_UP_SPEAK:
  c.rel32(b'\xe8','wait_up_key'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','resolve_prev_option'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_nav')
  c.rel32(b'\xe8','emit_up_meta')
  if WAIT_UP_SPEAK:
   c.lea_rax_data(L['nav_option_token']); c.emit(b'\x0f\xb7\x00'); c.lea_rdx_data(L['selected_option_token']); c.emit(b'\x66\x89\x02')
   serial('nav_focus_activate')
 c.lea_rdx_data(L['string_ptr']); c.emit(b'\x48\x8b\x32')
 serial('prefix'); c.rel32(b'\xe8','serial_utf16')
 serial('suffix'); c.rel32(b'\xe9','begin_selected_label')

 c.label('begin_selected_label')
 # Reuse the same proven HII string-block decoder with the selected option's
 # StringId and the first sibling Strings package in this exact package-list.
 c.lea_rax_data(L['selected_option_token']); c.emit(b'\x0f\xb7\x00')
 c.lea_rdx_data(L['token']); c.emit(b'\x66\x89\x02')
 c.lea_rax_data(L['string_mode']); c.emit(b'\xc6\x00\x01')
 # Keep strings_ptr on the exact sibling Strings package/language that resolved
 # the question prompt.  This binds the selected label to the same live
 # language namespace instead of restarting from an unrelated first package.
 c.rel32(b'\xe9','resolve_string_package')

 c.label('selected_scsu_found')
 c.emit(b'\x80\x3e\x00'); c.rel32(b'\x0f\x84','fail_selected_string')
 c.lea_rdx_data(L['speech_text_source']); c.emit(b'\x48\x89\x32')
 serial('nav_focus_text_prefix' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'selected_text_prefix'); c.rel32(b'\xe8','serial_scsu_ascii')
 c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','fail_selected_string')
 serial('nav_focus_text_done' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'selected_text_done')
 c.lea_rdx_data(L['speech_text_source']); c.emit(b'\x48\x8b\x32')
 c.emit(b'\x31\xff')
 c.label('capture_scsu_loop')
 c.emit(b'\x8a\x06\x84\xc0'); c.rel32(b'\x0f\x84','capture_done')
 c.emit(b'\x48\xff\xc6\x0c\x20')
 c.emit(b'\x3c\x30'); c.rel32(b'\x0f\x82','capture_scsu_loop')
 c.emit(b'\x3c\x39'); c.rel32(b'\x0f\x86','capture_scsu_accept')
 c.emit(b'\x3c\x61'); c.rel32(b'\x0f\x82','capture_scsu_loop')
 c.emit(b'\x3c\x7a'); c.rel32(b'\x0f\x87','capture_scsu_loop')
 c.label('capture_scsu_accept')
 c.emit(b'\x83\xff\x08'); c.rel32(b'\x0f\x83','capture_done')
 c.emit(b'\x41\x89\xc3'); serial('nav_speech_char' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'speech_char')
 c.lea_rdx_data(L['textbuf']); c.emit(b'\x89\xf8\x48\x8d\x04\x42\x66\x44\x89\x18\xff\xc7')
 c.rel32(b'\xe9','capture_scsu_loop')

 c.label('selected_ucs_found')
 c.emit(b'\x66\x83\x3e\x00'); c.rel32(b'\x0f\x84','fail_selected_string')
 c.lea_rdx_data(L['speech_text_source']); c.emit(b'\x48\x89\x32')
 serial('nav_focus_text_prefix' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'selected_text_prefix'); c.rel32(b'\xe8','serial_utf16')
 serial('nav_focus_text_done' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'selected_text_done')
 c.lea_rdx_data(L['speech_text_source']); c.emit(b'\x48\x8b\x32')
 c.emit(b'\x31\xff')
 c.label('capture_ucs_loop')
 c.emit(b'\x0f\xb7\x06\x85\xc0'); c.rel32(b'\x0f\x84','capture_done')
 c.emit(b'\x48\x83\xc6\x02\x83\xc8\x20')
 c.emit(b'\x83\xf8\x30'); c.rel32(b'\x0f\x82','capture_ucs_loop')
 c.emit(b'\x83\xf8\x39'); c.rel32(b'\x0f\x86','capture_ucs_accept')
 c.emit(b'\x83\xf8\x61'); c.rel32(b'\x0f\x82','capture_ucs_loop')
 c.emit(b'\x83\xf8\x7a'); c.rel32(b'\x0f\x87','capture_ucs_loop')
 c.label('capture_ucs_accept')
 c.emit(b'\x83\xff\x08'); c.rel32(b'\x0f\x83','capture_done')
 c.emit(b'\x41\x89\xc3'); serial('nav_speech_char' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'speech_char')
 c.lea_rdx_data(L['textbuf']); c.emit(b'\x89\xf8\x48\x8d\x04\x42\x66\x44\x89\x18\xff\xc7')
 c.rel32(b'\xe9','capture_ucs_loop')

 c.label('capture_done')
 c.emit(b'\x85\xff'); c.rel32(b'\x0f\x84','fail_speech_text')
 c.lea_rdx_data(L['textbuf']); c.emit(b'\x89\xf8\x48\x8d\x04\x42\x66\xc7\x00\x00\x00')
 c.lea_rdx_data(L['text_count']); c.emit(b'\x89\x3a')
 if WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL:
  serial('nav_speech_hii')
  serial('nav_spoken_prefix'); c.rel32(b'\xe8','serial_textbuf'); serial('nav_spoken_prefix_done')
 else:
  serial('speech_hii')
  serial('spoken_prefix'); c.rel32(b'\xe8','serial_textbuf'); serial('spoken_prefix_done')
 if WAIT_REPEAT_KEY:
  # ReadKeyStroke is read-only. Ignore EFI_NOT_READY and unrelated keys; only
  # Latin R/r authorizes the already-resolved current label to reach HDA.
  serial('repeat_wait')
  c.label('repeat_read_key')
  c.lea_rax_data(L['conin_ptr']); c.emit(b'\x48\x8b\x08')
  c.lea_rdx_data(L['keybuf'])
  c.emit(b'\x48\x8b\x41\x08\xff\xd0\x48\x85\xc0')
  c.rel32(b'\x0f\x85','repeat_read_key')
  c.lea_rdx_data(L['keybuf']); c.emit(b'\x0f\xb7\x42\x02\x66\x83\xc8\x20\x66\x3d\x72\x00')
  c.rel32(b'\x0f\x85','repeat_read_key')
  serial('repeat_accept')

 c.label('hda_begin')
 # Two-pass PCI HDA selection. On AMD-5800H-REAL, the internal speakers are
 # behind AMD 1022:15E3 -> Realtek 10EC:0256. The 1002:1637 HDA function is
 # HDMI audio, so a plain first-class-0403 policy can route speech off-laptop.
 # Pass 1 prefers the proven analog controller; pass 2 preserves generic QEMU/
 # VMware and non-ASUS operation.
 c.emit(b'\x45\x31\xe4')
 c.label('scan_preferred')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x08\x0d\x00\x00\x00\x80\x41\x89\xc5')
 c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x66\x3d\xff\xff'); c.rel32(b'\x0f\x84','scan_preferred_next')
 c.emit(b'\x3d'+struct.pack('<I',0x15E31022)); c.rel32(b'\x0f\x85','scan_preferred_next')
 c.emit(b'\x44\x89\xe8\x83\xc8\x08'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\xc1\xe8\x10\x66\x3d\x03\x04'); c.rel32(b'\x0f\x85','scan_preferred_next')
 serial('controller_preferred')
 c.rel32(b'\xe9','found')
 c.label('scan_preferred_next')
 c.emit(b'\x41\xff\xc4\x41\x81\xfc\x00\x00\x01\x00'); c.rel32(b'\x0f\x82','scan_preferred')

 # Generic fallback: first standards-class HDA controller.
 c.emit(b'\x45\x31\xe4')
 c.label('scan')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x08\x0d\x00\x00\x00\x80\x41\x89\xc5')
 c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x66\x3d\xff\xff'); c.rel32(b'\x0f\x84','scan_next')
 c.emit(b'\x44\x89\xe8\x83\xc8\x08'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\xc1\xe8\x10\x66\x3d\x03\x04'); c.rel32(b'\x0f\x84','found_generic')
 c.label('scan_next')
 c.emit(b'\x41\xff\xc4\x41\x81\xfc\x00\x00\x01\x00'); c.rel32(b'\x0f\x82','scan')
 c.rel32(b'\xe9','fail_no_hda')

 c.label('found_generic')
 serial('controller_generic')
 c.rel32(b'\xe9','found')

 c.label('found')
 # Enable PCI memory space and bus mastering.
 c.emit(b'\x44\x89\xe8\x83\xc8\x04'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x89\xc1\x83\xc9\x06\x44\x89\xe8\x83\xc8\x04'); c.rel32(b'\xe8','pci_write32')
 # BAR0, including 64-bit BARs.
 c.emit(b'\x44\x89\xe8\x83\xc8\x10'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x41\x89\xc6\xa8\x01'); c.rel32(b'\x0f\x85','fail_bad_hda')
 c.emit(b'\x89\xc1\x83\xe1\x06\x83\xf9\x04'); c.rel32(b'\x0f\x85','bar_low')
 c.emit(b'\x44\x89\xe8\x83\xc8\x14'); c.rel32(b'\xe8','pci_read32')
 c.emit(b'\x48\xc1\xe0\x20\x49\x09\xc6')
 c.label('bar_low')
 c.emit(b'\x49\x81\xe6\xf0\xff\xff\xff\x4d\x85\xf6'); c.rel32(b'\x0f\x84','fail_bad_hda')
 # Move BAR to r14 and sanity check GCAP/version.
 c.emit(b'\x4d\x89\xf6')
 c.emit(b'\x41\x0f\xb7\x06\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x41\x0f\xb6\x46\x03\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 # Controller reset.
 c.emit(b'\x41\x8b\x46\x08\x83\xe0\xfe\x41\x89\x46\x08\xb9\xa0\x86\x01\x00')
 c.label('reset_clear_poll'); c.emit(b'\x41\x8b\x46\x08\xa8\x01'); c.rel32(b'\x0f\x84','reset_clear_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','reset_clear_poll'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('reset_clear_ok')
 c.emit(b'\xb9\x64\x00\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x41\x8b\x46\x08\x83\xc8\x01\x41\x89\x46\x08\xb9\xa0\x86\x01\x00')
 c.label('reset_set_poll'); c.emit(b'\x41\x8b\x46\x08\xa8\x01'); c.rel32(b'\x0f\x85','reset_set_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','reset_set_poll'); c.rel32(b'\xe9','fail_bad_hda')
 c.label('reset_set_ok')
 c.emit(b'\xb9\xe8\x03\x00\x00\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 # Pick first codec address from STATESTS.
 c.emit(b'\x41\x0f\xb7\x46\x0e\x66\x85\xc0'); c.rel32(b'\x0f\x84','fail_bad_hda')
 c.emit(b'\x45\x31\xe4')
 c.label('cad_loop')
 c.emit(b'\xa8\x01'); c.rel32(b'\x0f\x85','cad_found')
 c.emit(b'\x66\xd1\xe8\x41\xff\xc4\x41\x83\xfc\x0f'); c.rel32(b'\x0f\x82','cad_loop')
 c.rel32(b'\xe9','fail_bad_hda')
 c.label('cad_found')
 serial('controller')

 # Discover Audio Function Group and its widget range.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x0d\x04\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x41\x89\xc5\x41\x81\xe5\xff\x00\x00\x00\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc6\xc1\xee\x10\x81\xe6\xff\x00\x00\x00\x85\xf6'); c.rel32(b'\x0f\x84','fail_topology')
 c.label('afg_loop')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x05\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x01'); c.rel32(b'\x0f\x84','afg_found')
 c.emit(b'\xff\xc6\x41\xff\xcd'); c.rel32(b'\x0f\x85','afg_loop')
 c.rel32(b'\xe9','fail_topology')
 c.label('afg_found')

 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x04\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x41\x89\xc5\x41\x81\xe5\xff\x00\x00\x00\x45\x85\xed'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc6\xc1\xee\x10\x81\xe6\xff\x00\x00\x00\x85\xf6'); c.rel32(b'\x0f\x84','fail_topology')

 # r10d = DAC NID; r11d = output Pin NID.
 c.emit(b'\x45\x31\xd2\x45\x31\xdb')
 c.label('widget_scan')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x09\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc1\xc1\xe9\x14\x83\xe1\x0f')
 c.emit(b'\x83\xf9\x00'); c.rel32(b'\x0f\x85','maybe_pin')
 c.emit(b'\x45\x85\xd2'); c.rel32(b'\x0f\x85','widget_next')
 c.emit(b'\x41\x89\xf2'); c.rel32(b'\xe9','widget_next')
 c.label('maybe_pin')
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x85','widget_next')
 c.emit(b'\x45\x85\xdb'); c.rel32(b'\x0f\x85','widget_next')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x89\xf1\xc1\xe1\x14\x09\xc8\x0d\x0c\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\xa8\x10'); c.rel32(b'\x0f\x84','widget_next')
 c.emit(b'\x41\x89\xf3')
 c.label('widget_next')
 c.emit(b'\xff\xc6\x41\xff\xcd'); c.rel32(b'\x0f\x85','widget_scan')
 c.emit(b'\x45\x85\xd2'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x45\x85\xdb'); c.rel32(b'\x0f\x84','fail_topology')

 # Direct route: selected pin must connect to selected DAC.
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x44\x89\xd9\xc1\xe1\x14\x09\xc8\x0d\x0e\x00\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x3d\xff\xff\xff\xff'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\x89\xc1\x80\xe1\x7f'); c.rel32(b'\x0f\x84','fail_topology')
 c.emit(b'\xa8\x80'); c.rel32(b'\x0f\x85','route_long')
 c.label('route_short')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x44\x89\xd9\xc1\xe1\x14\x09\xc8\x0d\x00\x02\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x25\x7f\x00\x00\x00\x44\x39\xd0'); c.rel32(b'\x0f\x85','fail_topology')
 c.rel32(b'\xe9','route_ok')
 c.label('route_long')
 c.emit(b'\x44\x89\xe0\xc1\xe0\x1c\x44\x89\xd9\xc1\xe1\x14\x09\xc8\x0d\x00\x02\x0f\x00')
 c.rel32(b'\xe8','immediate')
 c.emit(b'\x25\xff\x7f\x00\x00\x44\x39\xd0'); c.rel32(b'\x0f\x85','fail_topology')
 c.label('route_ok')

 c.lea_rdx_data(L['dac_nid']); c.emit(b'\x44\x89\x12')
 c.lea_rdx_data(L['pin_nid']); c.emit(b'\x44\x89\x1a')
 serial('topology')

 # Apply runtime output policy to the selected pin and DAC.
 verb_data(L['pin_nid'],0x000f1c00)
 c.emit(b'\xc1\xe8\x14\x83\xe0\x0f\x83\xf8\x02'); c.rel32(b'\x0f\x87','fail_policy')
 # Query Pin Capabilities. If EAPD is advertised (bit 16), enable it and
 # read it back before speech. Unsupported pins take the verified skip path.
 verb_data(L['pin_nid'],0x000f000c)
 c.emit(b'\xa9\x00\x00\x01\x00'); c.rel32(b'\x0f\x84','eapd_done')
 verb_data(L['pin_nid'],0x00070c02)
 verb_data(L['pin_nid'],0x000f0c00)
 c.emit(b'\xa8\x02'); c.rel32(b'\x0f\x84','fail_policy')
 c.label('eapd_done')
 verb_data(L['dac_nid'],0x000f0012)
 c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x84','fail_policy')
 verb_data(L['dac_nid'],0x0003b040)
 verb_data(L['dac_nid'],0x000ba000)
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x40'); c.rel32(b'\x0f\x85','fail_policy')
 verb_data(L['dac_nid'],0x000b8000)
 c.emit(b'\x25\xff\x00\x00\x00\x83\xf8\x40'); c.rel32(b'\x0f\x85','fail_policy')
 serial('policy')

 # Allocate DMA pages below 4 GiB: BDL at base, unit bank at base+0x1000.
 c.emit(b'\xb9\x01\x00\x00\x00\xba\x04\x00\x00\x00')
 c.emit(b'\x41\xb8'+struct.pack('<I',DMA_PAGES))
 c.lea_r9_data(L['maxaddr'])
 c.emit(b'\x49\x8b\x47\x28\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','fail_dma_alloc')
 c.mov_r13_data(L['maxaddr'])
 c.emit(b'\x4d\x85\xed'); c.rel32(b'\x0f\x84','fail_dma_alloc')
 # Copy the unordered first-party allophone bank into DMA memory.
 c.lea_rsi_data(L['pcm'])
 c.emit(b'\x49\x8d\xbd'+struct.pack('<i',PCM_OFF))
 c.emit(b'\xb9'+struct.pack('<I',len(pcm))+b'\xf3\xa4')
 # Native HII title input was decoded into textbuf before HDA setup.
 c.lea_rdx_data(L['text_count']); c.emit(bytes.fromhex('8b3a'))
 c.emit(bytes.fromhex('85ff')); c.rel32(bytes.fromhex('0f84'),'fail_speech_text')
 c.label('text_commit')
 c.emit(bytes.fromhex('85ff')); c.rel32(bytes.fromhex('0f84'),'fail_speech_text')
 c.lea_rdx_data(L['textbuf'])
 c.emit(bytes.fromhex('89f8488d044266c7000000'))
 c.emit(bytes.fromhex('31f6'))      # esi = character index
 c.emit(bytes.fromhex('31db'))      # ebx = BDL descriptor count
 c.emit(bytes.fromhex('4531d2'))    # r10d = total PCM bytes
 c.label('expand_char')
 c.lea_rdx_data(L['textbuf'])
 c.emit(bytes.fromhex('89f00fb70442'))
 for ch in LETTER_UNITS:
  c.emit(bytes.fromhex('663d')+struct.pack('<H',ord(ch)))
  c.rel32(bytes.fromhex('0f84'),'expand_'+ch)
 c.rel32(bytes.fromhex('e9'),'fail_speech_text')

 def emit_unit_descriptor(name):
  unit_off,unit_len=UNIT_LAYOUT[name]
  c.emit(bytes.fromhex('89d848c1e0044c01e8'))
  c.emit(bytes.fromhex('498d95')+struct.pack('<i',PCM_OFF+unit_off))
  c.emit(bytes.fromhex('488910'))
  c.emit(bytes.fromhex('c74008')+struct.pack('<I',unit_len))
  c.emit(bytes.fromhex('c7400c00000000'))
  c.emit(bytes.fromhex('ffc3'))
  c.emit(bytes.fromhex('4181c2')+struct.pack('<I',unit_len))

 def emit_expansion(ch,sequence):
  c.label('expand_'+ch)
  for name in sequence:
   emit_unit_descriptor(name)
  c.rel32(bytes.fromhex('e9'),'expanded_char')

 for ch,sequence in LETTER_UNITS.items():
  emit_expansion(ch,sequence)

 c.label('expanded_char')
 c.emit(bytes.fromhex('ffc6'))
 c.emit(bytes.fromhex('39fe'))
 c.rel32(bytes.fromhex('0f83'),'all_chars_expanded')
 # The imported option-speech unit bank includes a 65 ms silence unit.
 # Insert it only between graphemes, never after the final grapheme.
 emit_unit_descriptor('sil')
 c.rel32(bytes.fromhex('e9'),'expand_char')
 c.label('all_chars_expanded')
 c.emit(bytes.fromhex('85db')); c.rel32(bytes.fromhex('0f84'),'fail_speech_text')
 c.emit(bytes.fromhex('89d8ffc848c1e0044c01e8'))
 c.emit(bytes.fromhex('c7400c01000000'))
 c.emit(bytes.fromhex('4189db41ffcb'))
 serial('nav_text_ready' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'text_ready')
 c.emit(bytes.fromhex('0f09'))
 serial('dma')

 # Derive first output stream descriptor from GCAP.ISS.
 c.emit(b'\x41\x0f\xb7\x06\xc1\xe8\x08\x83\xe0\x0f\xc1\xe0\x05\x05\x80\x00\x00\x00')
 c.emit(b'\x49\x8d\x1c\x06')
 # Reset SDCTL with byte accesses so SDSTS at +3 is never overwritten.
 c.emit(b'\x8a\x03\x24\xfd\x88\x03\xb9\xa0\x86\x01\x00')
 c.label('sd_run_clear'); c.emit(b'\xf6\x03\x02'); c.rel32(b'\x0f\x84','sd_run_clear_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sd_run_clear'); c.rel32(b'\xe9','fail_stream')
 c.label('sd_run_clear_ok')
 c.emit(b'\x8a\x03\x0c\x01\x88\x03\xb9\xa0\x86\x01\x00')
 c.label('sd_reset_set'); c.emit(b'\xf6\x03\x01'); c.rel32(b'\x0f\x85','sd_reset_set_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sd_reset_set'); c.rel32(b'\xe9','fail_stream')
 c.label('sd_reset_set_ok')
 c.emit(b'\x8a\x03\x24\xfe\x88\x03\xb9\xa0\x86\x01\x00')
 c.label('sd_reset_clear'); c.emit(b'\xf6\x03\x01'); c.rel32(b'\x0f\x84','sd_reset_clear_ok')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','sd_reset_clear'); c.rel32(b'\xe9','fail_stream')
 c.label('sd_reset_clear_ok')
 # CBL, LVI, format, BDL address.
 c.emit(b'\x44\x89\x53\x08')
 c.emit(b'\x66\x44\x89\x5b\x0c')
 c.emit(b'\x66\xc7\x43\x12\x11\x00')
 c.emit(b'\x44\x89\xe8\x89\x43\x18')
 c.emit(b'\x4c\x89\xe8\x48\xc1\xe8\x20\x89\x43\x1c')
 serial('stream')

 # Route the runtime-discovered DAC and output pin.
 verb_data(L['dac_nid'],0x00070610)
 verb_data(L['dac_nid'],0x00020011)
 verb_data(L['pin_nid'],0x00070740)
 serial('codec')

 # Program stream number 1 in SDCTL byte 2, clear stale status, then RUN.
 c.emit(b'\xc6\x43\x03\x1c\xc6\x43\x02\x10\xc6\x03\x02')
 # Wait for the complete PCM payload, not a fixed 400 ms window.
 # 48 kHz * 16-bit * stereo = 192000 bytes/s, so duration_us =
 # ceil(total_bytes * 125 / 24). Add 150 ms device/codec guard time.
 c.emit(b'\x44\x89\xd0')                  # eax = total PCM bytes (r10d)
 c.emit(b'\x6b\xc0\x7d')                  # eax *= 125
 c.emit(b'\x83\xc0\x17')                  # +23 for ceil(/24)
 c.emit(b'\x31\xd2')                       # edx:eax dividend
 c.emit(b'\xb9\x18\x00\x00\x00\xf7\xf1')  # div 24
 c.emit(b'\x05\xf0\x49\x02\x00\x89\xc1')  # +150000 us; ecx=delay
 c.emit(b'\x49\x8b\x87\xf8\x00\x00\x00\xff\xd0')
 c.emit(b'\x8b\x43\x04\x85\xc0'); c.rel32(b'\x0f\x84','fail_stream')
 # IOC on the final BDL descriptor must complete before speech is considered done.
 c.emit(b'\xf6\x43\x03\x04'); c.rel32(b'\x0f\x84','fail_stream')
 serial('nav_progress' if (WAIT_DOWN_SPEAK or WAIT_UP_SPEAK or WAIT_DOWN_COMMIT or WAIT_DOWN_CANCEL) else 'progress')
 # Stop stream.
 c.emit(b'\x8a\x03\x24\xfd\x88\x03')
 if WAIT_DOWN_CANCEL:
  c.rel32(b'\xe8','wait_cancel_key'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','return_fail')
  serial('done'); c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')
 elif WAIT_DOWN_COMMIT:
  c.lea_rax_data(L['commit_done']); c.emit(b'\x80\x38\x01'); c.rel32(b'\x0f\x84','commit_second_audio_done')
  c.rel32(b'\xe8','wait_commit_key'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','return_fail')
  c.rel32(b'\xe8','commit_focused_option'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','return_fail')
  c.lea_rax_data(L['commit_done']); c.emit(b'\xc6\x00\x01'); c.rel32(b'\xe9','hda_begin')
  c.label('commit_second_audio_done'); serial('commit_confirm'); serial('done')
  c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')
 else:
  serial('done')
  c.emit(b'\x31\xc0'); c.rel32(b'\xe9','return')

 c.label('fail_selected_string'); serial('selected_string_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_nav'); serial('nav_fail'); c.rel32(b'\xe9','return_fail')

 c.label('fail_no_hda'); serial('no_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_bad_hda'); serial('bad_hda'); c.rel32(b'\xe9','return_fail')
 c.label('fail_dma_alloc'); serial('dma_alloc'); c.rel32(b'\xe9','return_fail')
 c.label('fail_verb'); serial('verb'); c.rel32(b'\xe9','return_fail')
 c.label('fail_stream'); serial('stream_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_speech_text'); serial('speech_text_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_topology'); serial('topology_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_policy'); serial('policy_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_protocol'); serial('no_protocol'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_size'); serial('list_size_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch'); serial('list_fetch_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch_invalid'); serial('list_fetch_invalid'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch_not_found'); serial('list_fetch_not_found'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_fetch_bts_twice'); serial('list_fetch_bts_twice'); c.rel32(b'\xe9','return_fail')
 c.label('fail_list_static_small'); serial('list_static_small'); c.rel32(b'\xe9','return_fail')
 c.label('fail_no_handle'); serial('no_handle'); c.rel32(b'\xe9','return_fail')
 c.label('fail_static_empty'); serial('static_empty'); c.rel32(b'\xe9','return_fail')
 c.label('fail_alloc'); serial('alloc'); c.rel32(b'\xe9','return_fail')
 c.label('fail_export'); serial('export'); c.rel32(b'\xe9','return_fail')
 c.label('fail_ifr'); serial('ifr_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_language'); serial('lang_fail'); c.rel32(b'\xe9','return_fail')
 c.label('fail_string')
 # Try every sibling Strings package (typically alternate languages) before
 # abandoning this Prompt StringId.
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x8b\x32')
 c.emit(b'\x8b\x06\x25\xff\xff\xff\x00')
 c.emit(b'\x83\xf8\x04'); c.rel32(b'\x0f\x82','prompt_next')
 c.emit(b'\x48\x01\xc6')
 c.lea_rdx_data(L['list_start']); c.emit(b'\x48\x8b\x3a')
 c.lea_rdx_data(L['list_len']); c.emit(b'\x8b\x02\x48\x01\xc7')
 c.label('next_strings_scan')
 c.emit(b'\x48\x8d\x46\x04\x48\x39\xf8'); c.rel32(b'\x0f\x87','prompt_next')
 c.emit(b'\x8b\x06\x89\xc1\x81\xe1\xff\xff\xff\x00')
 c.emit(b'\x89\xc2\xc1\xea\x18')
 c.emit(b'\x83\xf9\x04'); c.rel32(b'\x0f\x82','prompt_next')
 c.emit(b'\x48\x8d\x04\x0e\x48\x39\xf8'); c.rel32(b'\x0f\x87','prompt_next')
 c.emit(b'\x83\xfa\x04'); c.rel32(b'\x0f\x84','use_next_strings')
 c.emit(b'\x81\xfa\xdf\x00\x00\x00'); c.rel32(b'\x0f\x84','prompt_next')
 c.emit(b'\x48\x89\xc6'); c.rel32(b'\xe9','next_strings_scan')
 c.label('use_next_strings')
 c.lea_rdx_data(L['strings_ptr']); c.emit(b'\x48\x89\x32')
 serial('strings_retry')
 c.rel32(b'\xe9','resolve_string_package')

 c.label('prompt_next')
 c.lea_rax_data(L['string_mode']); c.emit(b'\x80\x38\x01'); c.rel32(b'\x0f\x84','fail_selected_string')
 # No language package resolved this prompt token: continue with the next real
 # IFR question, preserving firmware statement order.
 c.lea_rdx_data(L['ifr_next_ptr']); c.emit(b'\x4c\x8b\x0a')
 c.lea_rdx_data(L['ifr_next_remaining']); c.emit(b'\x44\x8b\x12')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x83','ifr_loop')

 c.label('prompt_package_done')
 # This Forms package-list did not yield a resolvable prompt. Continue through
 # the remaining exported HII package-lists rather than producing a false FAIL.
 c.rel32(b'\xe9','direct_list_next')

 c.label('return_fail'); c.emit(b'\xb8\x01\x00\x00\x00')
 c.label('return')
 c.emit(b'\x48\x83\xc4\x68\x41\x5f\x41\x5e\x41\x5d\x41\x5c\x5f\x5e\x5d\x5b\xc3')

 # Input rsi=UTF-16 string. Emit low ASCII byte, '?' for non-ASCII codepoints.
 c.label('serial_utf16')
 c.label('utf16_loop')
 c.emit(b'\x66\x8b\x06\x66\x85\xc0'); c.rel32(b'\x0f\x84','utf16_done')
 c.emit(b'\x66\x3d\x7f\x00'); c.rel32(b'\x0f\x87','utf16_question')
 c.emit(b'\x3c\x20'); c.rel32(b'\x0f\x83','utf16_emit')
 c.label('utf16_question'); c.emit(b'\xb0\x3f')
 c.label('utf16_emit')
 c.emit(b'\x88\xc3\x66\xba\xfd\x03')
 c.label('utf16_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'utf16_wait')
 c.emit(b'\x66\xba\xf8\x03\x88\xd8\xee\x48\x83\xc6\x02')
 c.rel32(b'\xe9','utf16_loop')
 c.label('utf16_done'); c.emit(b'\xc3')

 c.label('serial_scsu_ascii')
 c.label('scsu_ascii_loop')
 c.emit(b'\x8a\x06\x84\xc0'); c.rel32(b'\x0f\x84','scsu_ascii_ok')
 c.emit(b'\x3c\x20'); c.rel32(b'\x0f\x82','scsu_ascii_bad')
 c.emit(b'\x3c\x7e'); c.rel32(b'\x0f\x87','scsu_ascii_bad')
 c.emit(b'\x88\xc3\x66\xba\xfd\x03')
 c.label('scsu_ascii_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'scsu_ascii_wait')
 c.emit(b'\x66\xba\xf8\x03\x88\xd8\xee\x48\xff\xc6'); c.rel32(b'\xe9','scsu_ascii_loop')
 c.label('scsu_ascii_ok'); c.emit(b'\x31\xc0\xc3')
 c.label('scsu_ascii_bad'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('resolve_varstore')
 # Re-scan the selected Forms package from its first IFR opcode and match the
 # non-zero Question.VarStoreId against VARSTORE(0x24), NAME_VALUE(0x25), or
 # VARSTORE_EFI(0x26). This routine is read-only and never calls GetVariable.
 c.lea_rdx_data(L['forms_ptr']); c.emit(b'\x4c\x8b\x0a')
 c.emit(b'\x41\x8b\x01\x25\xff\xff\xff\x00')
 c.emit(b'\x83\xf8\x04'); c.rel32(b'\x0f\x82','varstore_not_found')
 c.emit(b'\x41\x89\xc2\x41\x83\xea\x04\x49\x83\xc1\x04')
 c.label('varstore_scan')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x82','varstore_not_found')
 c.emit(b'\x41\x0f\xb6\x01')
 c.emit(b'\x41\x0f\xb6\x49\x01\x83\xe1\x7f')
 c.emit(b'\x83\xf9\x02'); c.rel32(b'\x0f\x82','varstore_not_found')
 c.emit(b'\x44\x39\xd1'); c.rel32(b'\x0f\x87','varstore_not_found')
 c.emit(b'\x3c\x24'); c.rel32(b'\x0f\x84','varstore_buffer')
 c.emit(b'\x3c\x25'); c.rel32(b'\x0f\x84','varstore_name_value')
 c.emit(b'\x3c\x26'); c.rel32(b'\x0f\x84','varstore_efi')
 c.label('varstore_next')
 c.emit(b'\x49\x01\xc9\x41\x29\xca'); c.rel32(b'\xe9','varstore_scan')

 c.label('varstore_buffer')
 c.emit(b'\x83\xf9\x17'); c.rel32(b'\x0f\x82','varstore_next')
 c.emit(b'\x41\x0f\xb7\x41\x12')
 c.lea_rdx_data(L['varstore_id']); c.emit(b'\x66\x3b\x02'); c.rel32(b'\x0f\x85','varstore_next')
 c.lea_rdx_data(L['varstore_opcode']); c.emit(b'\xc6\x02\x24')
 c.emit(b'\x41\x0f\xb7\x41\x14'); c.lea_rdx_data(L['varstore_size']); c.emit(b'\x66\x89\x02')
 c.lea_rdx_data(L['varstore_attrs']); c.emit(b'\xc7\x02\x00\x00\x00\x00')
 c.emit(b'\x49\x8b\x41\x02'); c.lea_rdx_data(L['varstore_guid']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x49\x8b\x41\x0a'); c.lea_rdx_data(L['varstore_guid']+8); c.emit(b'\x48\x89\x02')
 # EFI_IFR_VARSTORE.Name is the NUL-terminated ASCII name starting at +0x16.
 # Preserve its live pointer so ConfigResp routing is bound to GUID + NAME.
 c.emit(b'\x49\x8d\x41\x16'); c.lea_rdx_data(L['varstore_name_ptr']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x89\xc8\x83\xe8\x16'); c.lea_rdx_data(L['varstore_name_remaining']); c.emit(b'\x89\x02')
 c.rel32(b'\xe9','varstore_found')

 c.label('varstore_name_value')
 c.emit(b'\x83\xf9\x14'); c.rel32(b'\x0f\x82','varstore_next')
 c.emit(b'\x41\x0f\xb7\x41\x02')
 c.lea_rdx_data(L['varstore_id']); c.emit(b'\x66\x3b\x02'); c.rel32(b'\x0f\x85','varstore_next')
 c.lea_rdx_data(L['varstore_opcode']); c.emit(b'\xc6\x02\x25')
 c.lea_rdx_data(L['varstore_size']); c.emit(b'\x66\xc7\x02\x00\x00')
 c.lea_rdx_data(L['varstore_attrs']); c.emit(b'\xc7\x02\x00\x00\x00\x00')
 c.emit(b'\x49\x8b\x41\x04'); c.lea_rdx_data(L['varstore_guid']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x49\x8b\x41\x0c'); c.lea_rdx_data(L['varstore_guid']+8); c.emit(b'\x48\x89\x02')
 c.rel32(b'\xe9','varstore_found')

 c.label('varstore_efi')
 c.emit(b'\x83\xf9\x1b'); c.rel32(b'\x0f\x82','varstore_next')
 c.emit(b'\x41\x0f\xb7\x41\x02')
 c.lea_rdx_data(L['varstore_id']); c.emit(b'\x66\x3b\x02'); c.rel32(b'\x0f\x85','varstore_next')
 c.lea_rdx_data(L['varstore_opcode']); c.emit(b'\xc6\x02\x26')
 c.emit(b'\x41\x0f\xb7\x41\x18'); c.lea_rdx_data(L['varstore_size']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x8b\x41\x14'); c.lea_rdx_data(L['varstore_attrs']); c.emit(b'\x89\x02')
 c.emit(b'\x49\x8b\x41\x04'); c.lea_rdx_data(L['varstore_guid']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x49\x8b\x41\x0c'); c.lea_rdx_data(L['varstore_guid']+8); c.emit(b'\x48\x89\x02')
 # EFI_IFR_VARSTORE_EFI.Name is the trailing NUL-terminated ASCII name at +0x1a.
 c.emit(b'\x49\x8d\x41\x1a'); c.lea_rdx_data(L['varstore_name_ptr']); c.emit(b'\x48\x89\x02')
 c.emit(b'\x89\xc8\x83\xe8\x1a'); c.lea_rdx_data(L['varstore_name_remaining']); c.emit(b'\x89\x02')

 c.label('varstore_found'); c.emit(b'\x31\xc0\xc3')
 c.label('varstore_not_found'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('read_buffer_current')
 # Storage backends are intentionally separated. Existing proofs keep using
 # Buffer Storage (0x24). EFI_VARSTORE_ONLY probes only EFI variable storage
 # (0x26), which the UEFI browser itself services through RuntimeServices.
 c.lea_rax_data(L['varstore_opcode'])
 if EFI_VARSTORE_ONLY:
  c.emit(b'\x80\x38\x26'); c.rel32(b'\x0f\x85','buffer_current_not_found')
  c.rel32(b'\xe9','efi_var_current')
 else:
  c.emit(b'\x80\x38\x24'); c.rel32(b'\x0f\x85','buffer_current_not_found')
 # ONE_OF numeric width: Flags&3 => 1/2/4/8.
 c.lea_rax_data(L['oneof_flags']); c.emit(b'\x0f\xb6\x00\x83\xe0\x03\x89\xc1')
 c.emit(b'\xb8\x01\x00\x00\x00\xd3\xe0')
 c.lea_rdx_data(L['current_width']); c.emit(b'\x88\x02')
 # Declared VarStore bounds.
 c.lea_rdx_data(L['varstore_info']); c.emit(b'\x0f\xb7\x12\x01\xc2')
 c.lea_rax_data(L['varstore_size']); c.emit(b'\x0f\xb7\x00\x39\xc2'); c.rel32(b'\x0f\x87','buffer_current_not_found')

 # ExportConfig(This,&Results), routing method +0x08, returns the current
 # configuration for the entirety of the HII database. This is also the safe
 # post-RouteConfig verifier: some drivers accept a targeted ExtractConfig
 # request but fault internally, while ExportConfig remains read-only.
 c.lea_rdx_data(L['results']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')
 c.lea_rax_data(L['routing_ptr']); c.emit(b'\x48\x8b\x08')
 c.lea_rdx_data(L['results']); c.emit(b'\xff\x51\x08')
 c.emit(b'\x48\x85\xc0'); c.rel32(b'\x0f\x85','buffer_current_not_found')
 c.lea_rax_data(L['results']); c.emit(b'\x48\x8b\x30\x48\x85\xf6'); c.rel32(b'\x0f\x84','buffer_current_not_found')
 serial('cfg_access'); serial('extract')

 # Find a UTF-16 ConfigHdr whose GUID= field exactly matches the raw 16-byte
 # EFI_IFR_VARSTORE GUID. Hex comparison is case-insensitive.
 c.label('config_search')
 c.emit(b'\x66\x83\x3e\x00'); c.rel32(b'\x0f\x84','buffer_export_exhausted')
 c.emit(b'\x66\x81\x3e\x47\x00'); c.rel32(b'\x0f\x85','config_search_next')
 c.emit(b'\x66\x81\x7e\x02\x55\x00'); c.rel32(b'\x0f\x85','config_search_next')
 c.emit(b'\x66\x81\x7e\x04\x49\x00'); c.rel32(b'\x0f\x85','config_search_next')
 c.emit(b'\x66\x81\x7e\x06\x44\x00'); c.rel32(b'\x0f\x85','config_search_next')
 c.emit(b'\x66\x81\x7e\x08\x3d\x00'); c.rel32(b'\x0f\x85','config_search_next')
 c.emit(b'\x48\x8d\x7e\x0a')
 c.lea_rax_data(L['varstore_guid']); c.emit(b'\x48\x89\xc3')
 c.emit(b'\xb9\x10\x00\x00\x00')
 c.label('config_guid_loop')
 c.emit(b'\x44\x8a\x13')
 c.emit(b'\x0f\xb7\x07'); c.rel32(b'\xe8','hex_utf16_nibble')
 c.emit(b'\x3c\xff'); c.rel32(b'\x0f\x84','config_guid_mismatch')
 c.emit(b'\xc0\xe0\x04\x41\x88\xc1')
 c.emit(b'\x0f\xb7\x47\x02'); c.rel32(b'\xe8','hex_utf16_nibble')
 c.emit(b'\x3c\xff'); c.rel32(b'\x0f\x84','config_guid_mismatch')
 c.emit(b'\x44\x08\xc8\x44\x38\xd0'); c.rel32(b'\x0f\x85','config_guid_mismatch')
 c.emit(b'\x48\xff\xc3\x48\x83\xc7\x04\xff\xc9'); c.rel32(b'\x0f\x85','config_guid_loop')

 # Bind this ConfigHdr to the exact Buffer Storage NAME as well as GUID.
 # HiiConstructConfigHdr encodes each CHAR16 name code unit as four hex digits;
 # EFI_IFR_VARSTORE.Name is CHAR8, therefore every byte must encode as 00xx.
 for disp,ch in ((0,0x26),(2,0x4e),(4,0x41),(6,0x4d),(8,0x45),(10,0x3d)):
  if disp==0: c.emit(b'\x66\x81\x3f'+struct.pack('<H',ch))
  else: c.emit(b'\x66\x81\x7f'+bytes((disp,))+struct.pack('<H',ch))
  c.rel32(b'\x0f\x85','config_guid_mismatch')
 c.emit(b'\x48\x83\xc7\x0c')
 c.lea_rax_data(L['varstore_name_ptr']); c.emit(b'\x48\x8b\x18')
 c.lea_rax_data(L['varstore_name_remaining']); c.emit(b'\x8b\x08')
 c.label('config_name_loop')
 c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','config_guid_mismatch')
 c.emit(b'\x44\x8a\x13\x45\x84\xd2'); c.rel32(b'\x0f\x84','config_name_done')
 c.emit(b'\x66\x81\x3f\x30\x00'); c.rel32(b'\x0f\x85','config_guid_mismatch')
 c.emit(b'\x66\x81\x7f\x02\x30\x00'); c.rel32(b'\x0f\x85','config_guid_mismatch')
 c.emit(b'\x0f\xb7\x47\x04'); c.rel32(b'\xe8','hex_utf16_nibble')
 c.emit(b'\x3c\xff'); c.rel32(b'\x0f\x84','config_guid_mismatch')
 c.emit(b'\xc0\xe0\x04\x41\x88\xc1')
 c.emit(b'\x0f\xb7\x47\x06'); c.rel32(b'\xe8','hex_utf16_nibble')
 c.emit(b'\x3c\xff'); c.rel32(b'\x0f\x84','config_guid_mismatch')
 c.emit(b'\x44\x08\xc8\x44\x38\xd0'); c.rel32(b'\x0f\x85','config_guid_mismatch')
 c.emit(b'\x48\xff\xc3\x48\x83\xc7\x08\xff\xc9'); c.rel32(b'\xe9','config_name_loop')
 c.label('config_name_done')
 for disp,ch in ((0,0x26),(2,0x50),(4,0x41),(6,0x54),(8,0x48),(10,0x3d)):
  if disp==0: c.emit(b'\x66\x81\x3f'+struct.pack('<H',ch))
  else: c.emit(b'\x66\x81\x7f'+bytes((disp,))+struct.pack('<H',ch))
  c.rel32(b'\x0f\x85','config_guid_mismatch')
 serial('vs_name_match')

 # Save the selected current ConfigResp start and temporarily terminate it at
 # the next &GUID= boundary so ConfigToBlock sees exactly one ConfigResp.
 c.lea_rdx_data(L['config_ptr']); c.emit(b'\x48\x89\x32')
 c.emit(b'\x48\x8d\x7e\x02')
 c.label('config_boundary_scan')
 c.emit(b'\x66\x8b\x07\x66\x85\xc0'); c.rel32(b'\x0f\x84','config_boundary_end')
 c.emit(b'\x66\x83\xf8\x26'); c.rel32(b'\x0f\x85','config_boundary_next')
 c.emit(b'\x66\x81\x7f\x02\x47\x00'); c.rel32(b'\x0f\x85','config_boundary_next')
 c.emit(b'\x66\x81\x7f\x04\x55\x00'); c.rel32(b'\x0f\x85','config_boundary_next')
 c.emit(b'\x66\x81\x7f\x06\x49\x00'); c.rel32(b'\x0f\x85','config_boundary_next')
 c.emit(b'\x66\x81\x7f\x08\x44\x00'); c.rel32(b'\x0f\x85','config_boundary_next')
 c.emit(b'\x66\x81\x7f\x0a\x3d\x00'); c.rel32(b'\x0f\x85','config_boundary_next')
 c.lea_rdx_data(L['config_boundary']); c.emit(b'\x48\x89\x3a')
 c.emit(b'\x66\xc7\x07\x00\x00'); c.rel32(b'\xe9','config_to_block_call')
 c.label('config_boundary_next'); c.emit(b'\x48\x83\xc7\x02'); c.rel32(b'\xe9','config_boundary_scan')
 c.label('config_boundary_end'); c.lea_rdx_data(L['config_boundary']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')

 c.label('config_to_block_call')
 c.lea_rax_data(L['varstore_size']); c.emit(b'\x0f\xb7\x00'); c.lea_rdx_data(L['block_size']); c.emit(b'\x48\x89\x02')
 c.lea_rdx_data(L['config_progress']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')
 c.lea_rax_data(L['routing_ptr']); c.emit(b'\x48\x8b\x08')
 c.lea_rdx_data(L['config_ptr']); c.emit(b'\x48\x8b\x12')
 c.lea_r8_data(L['current_data']); c.lea_r9_data(L['block_size'])
 c.lea_rax_data(L['config_progress']); c.emit(b'\x48\x89\x44\x24\x20')
 c.emit(b'\xff\x51\x20\x49\x89\xc2')
 # Restore the allocated ExportConfig string before any next candidate/free.
 c.lea_rax_data(L['config_boundary']); c.emit(b'\x48\x8b\x10\x48\x85\xd2'); c.rel32(b'\x0f\x84','config_restore_done')
 c.emit(b'\x66\xc7\x02\x26\x00')
 c.label('config_restore_done')
 c.emit(b'\x4d\x85\xd2'); c.rel32(b'\x0f\x84','config_to_block_ok')
 # A same-GUID ConfigResp can belong to another storage name. Continue safely.
 c.lea_rax_data(L['config_boundary']); c.emit(b'\x48\x8b\x30\x48\x85\xf6'); c.rel32(b'\x0f\x84','buffer_export_exhausted')
 c.emit(b'\x48\x83\xc6\x02'); c.rel32(b'\xe9','config_search')

 c.label('config_to_block_ok'); serial('to_block')
 # Require the mapped block to cover the selected question field.
 c.lea_rax_data(L['block_size']); c.emit(b'\x48\x8b\x00')
 c.lea_rdx_data(L['varstore_info']); c.emit(b'\x0f\xb7\x12')
 c.lea_rcx_data(L['current_width']); c.emit(b'\x0f\xb6\x09\x48\x01\xca')
 # ConfigToBlock success returns the largest modified byte index, not a byte count.\n c.emit(b'\x48\xff\xc0\x48\x39\xd0'); c.rel32(b'\x0f\x82','buffer_export_exhausted')
 c.lea_rsi_data(L['current_data']); c.lea_rax_data(L['varstore_info']); c.emit(b'\x0f\xb7\x00\x48\x01\xc6')
 c.lea_rdx_data(L['current_raw']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00\xc7\x42\x04\x00\x00\x00\x00')
 c.lea_rax_data(L['current_width']); c.emit(b'\x0f\xb6\x08')
 c.label('buffer_copy_loop')
 c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','buffer_copy_done')
 c.emit(b'\x8a\x06\x88\x02\x48\xff\xc6\x48\xff\xc2\xff\xc9'); c.rel32(b'\xe9','buffer_copy_loop')
 c.label('buffer_copy_done')
 if WAIT_DOWN_COMMIT:
  c.emit(b'\x31\xc0\xc3')
 else:
  c.lea_rax_data(L['results']); c.emit(b'\x48\x8b\x08\x41\xff\x57\x48')
  c.emit(b'\x31\xc0\xc3')

 c.label('config_guid_mismatch')
 c.label('config_search_next'); c.emit(b'\x48\x83\xc6\x02'); c.rel32(b'\xe9','config_search')
 c.label('buffer_export_exhausted')
 c.lea_rax_data(L['results']); c.emit(b'\x48\x8b\x08\x48\x85\xc9'); c.rel32(b'\x0f\x84','buffer_current_not_found')
 c.emit(b'\x41\xff\x57\x48')
 # Buffer Storage exhaustion must never fall through into EFI VarStore handling.
 c.rel32(b'\xe9','buffer_current_not_found')
 c.label('efi_var_current')
 # ONE_OF numeric width: Flags&3 => 1/2/4/8 and declared VarStore bounds.
 c.lea_rax_data(L['oneof_flags']); c.emit(b'\x0f\xb6\x00\x83\xe0\x03\x89\xc1')
 c.emit(b'\xb8\x01\x00\x00\x00\xd3\xe0')
 c.lea_rdx_data(L['current_width']); c.emit(b'\x88\x02')
 c.lea_rdx_data(L['varstore_info']); c.emit(b'\x0f\xb7\x12\x01\xc2')
 c.lea_rax_data(L['varstore_size']); c.emit(b'\x0f\xb7\x00\x39\xc2'); c.rel32(b'\x0f\x87','buffer_current_not_found')

 # Convert the IFR ASCII variable name to the CHAR16 name required by GetVariable.
 c.lea_rax_data(L['varstore_name_ptr']); c.emit(b'\x48\x8b\x30\x48\x85\xf6'); c.rel32(b'\x0f\x84','buffer_current_not_found')
 c.lea_rax_data(L['varstore_name_remaining']); c.emit(b'\x8b\x08\x85\xc9'); c.rel32(b'\x0f\x84','buffer_current_not_found')
 c.lea_rdi_data(L['efi_name_utf16'])
 c.label('efi_name_copy')
 c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','buffer_current_not_found')
 c.emit(b'\x0f\xb6\x06\x84\xc0'); c.rel32(b'\x0f\x84','efi_name_done')
 c.emit(b'\x66\x89\x07\x48\xff\xc6\x48\x83\xc7\x02\xff\xc9'); c.rel32(b'\xe9','efi_name_copy')
 c.label('efi_name_done'); c.emit(b'\x66\xc7\x07\x00\x00'); serial('efi_var_name')

 # GetVariable(Name,Guid,NULL,&DataSize,current_data) from EFI Runtime Services.
 c.lea_rax_data(L['varstore_size']); c.emit(b'\x0f\xb7\x00')
 c.lea_rdx_data(L['efi_var_size']); c.emit(b'\x48\x89\x02')
 c.lea_rcx_data(L['efi_name_utf16']); c.lea_rdx_data(L['varstore_guid']); c.emit(b'\x45\x31\xc0'); c.lea_r9_data(L['efi_var_size'])
 c.lea_rax_data(L['current_data']); c.emit(b'\x48\x89\x44\x24\x20')
 c.lea_rax_data(L['rt_ptr']); c.emit(b'\x48\x8b\x00\x48\x85\xc0'); c.rel32(b'\x0f\x84','buffer_current_not_found')
 c.emit(b'\x48\x8b\x40\x48\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','buffer_current_not_found')
 serial('efi_var_get')

 # Copy only the selected question bytes to current_raw, preserving zero padding.
 c.lea_rdx_data(L['current_raw']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')
 c.lea_rsi_data(L['current_data']); c.lea_rax_data(L['varstore_info']); c.emit(b'\x0f\xb7\x00\x48\x01\xc6')
 c.lea_rdi_data(L['current_raw']); c.lea_rax_data(L['current_width']); c.emit(b'\x0f\xb6\x08')
 c.label('efi_current_copy'); c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','efi_current_done')
 c.emit(b'\x8a\x06\x88\x07\x48\xff\xc6\x48\xff\xc7\xff\xc9'); c.rel32(b'\xe9','efi_current_copy')
 c.label('efi_current_done'); c.emit(b'\x31\xc0\xc3')

 c.label('buffer_current_not_found'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('resolve_selected_option')
 # Match the live current ONE_OF value to a direct EFI_IFR_ONE_OF_OPTION child.
 # EFI_IFR_ONE_OF_OPTION is variable length: 7/8/10/14 bytes for
 # UINT8/16/32/64.  Read and compare exactly the Type width, never a fixed
 # eight-byte union, so the parser stays inside the verified IFR opcode.
 c.lea_rax_data(L['question_ptr']); c.emit(b'\x48\x8b\x00\x48\x85\xc0'); c.rel32(b'\x0f\x84','selected_option_not_found')
 # A ONE_OF owns a scoped child list; reject malformed/unscoped candidates.
 c.emit(b'\xf6\x40\x01\x80'); c.rel32(b'\x0f\x84','selected_option_not_found')
 c.lea_rdx_data(L['ifr_next_ptr']); c.emit(b'\x4c\x8b\x0a')
 c.lea_rdx_data(L['ifr_next_remaining']); c.emit(b'\x44\x8b\x12')
 c.emit(b'\x41\xbb\x01\x00\x00\x00') # r11d = ONE_OF scope depth
 c.label('selected_option_scan')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x82','selected_option_not_found')
 c.emit(b'\x41\x0f\xb6\x01')              # eax = opcode
 c.emit(b'\x41\x0f\xb6\x59\x01\x89\xd9\x83\xe1\x7f') # ebx=raw header byte; ecx=length
 c.emit(b'\x83\xf9\x02'); c.rel32(b'\x0f\x82','selected_option_not_found')
 c.emit(b'\x44\x39\xd1'); c.rel32(b'\x0f\x87','selected_option_not_found')
 c.emit(b'\x3c\x29'); c.rel32(b'\x0f\x84','selected_option_end')
 c.emit(b'\x3c\x09'); c.rel32(b'\x0f\x85','selected_option_scope_advance')
 c.emit(b'\x41\x83\xfb\x01'); c.rel32(b'\x0f\x85','selected_option_scope_advance')
 c.emit(b'\x83\xf9\x07'); c.rel32(b'\x0f\x82','selected_option_advance')
 c.emit(b'\x41\x89\xc8') # r8d = exact opcode Length

 # Type at +5 is numeric 0/1/2/3 => width 1/2/4/8. It must agree with the
 # parent ONE_OF numeric size, and Header.Length must cover 6 + width bytes.
 c.emit(b'\x41\x0f\xb6\x41\x05')
 c.emit(b'\x3c\x03'); c.rel32(b'\x0f\x87','selected_option_advance')
 c.lea_rdx_data(L['oneof_flags']); c.emit(b'\x0f\xb6\x12\x83\xe2\x03\x39\xd0'); c.rel32(b'\x0f\x85','selected_option_advance')
 c.emit(b'\x89\xc1\xba\x01\x00\x00\x00\xd3\xe2') # edx = 1 << Type
 c.emit(b'\x8d\x42\x06\x41\x39\xc0'); c.rel32(b'\x0f\x82','selected_option_restore_advance')
 c.emit(b'\x44\x89\xc1') # restore ecx = opcode Length for scan advance
 # EDX still holds the typed width after the bounds check.
 c.emit(b'\x83\xfa\x01'); c.rel32(b'\x0f\x84','selected_cmp8')
 c.emit(b'\x83\xfa\x02'); c.rel32(b'\x0f\x84','selected_cmp16')
 c.emit(b'\x83\xfa\x04'); c.rel32(b'\x0f\x84','selected_cmp32')
 c.rel32(b'\xe9','selected_cmp64')
 c.label('selected_option_restore_advance'); c.emit(b'\x44\x89\xc1'); c.rel32(b'\xe9','selected_option_advance')

 c.label('selected_cmp8')
 c.lea_rax_data(L['current_raw']); c.emit(b'\x8a\x00\x41\x3a\x41\x06'); c.rel32(b'\x0f\x85','selected_option_advance'); c.rel32(b'\xe9','selected_option_match')
 c.label('selected_cmp16')
 c.lea_rax_data(L['current_raw']); c.emit(b'\x66\x8b\x00\x66\x41\x3b\x41\x06'); c.rel32(b'\x0f\x85','selected_option_advance'); c.rel32(b'\xe9','selected_option_match')
 c.label('selected_cmp32')
 c.lea_rax_data(L['current_raw']); c.emit(b'\x8b\x00\x41\x3b\x41\x06'); c.rel32(b'\x0f\x85','selected_option_advance'); c.rel32(b'\xe9','selected_option_match')
 c.label('selected_cmp64')
 c.lea_rax_data(L['current_raw']); c.emit(b'\x48\x8b\x00\x49\x3b\x41\x06'); c.rel32(b'\x0f\x85','selected_option_advance')

 c.label('selected_option_match')
 c.lea_rdx_data(L['selected_option_ptr']); c.emit(b'\x4c\x89\x0a')
 c.emit(b'\x41\x0f\xb7\x41\x02'); c.lea_rdx_data(L['selected_option_token']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb6\x41\x05'); c.lea_rdx_data(L['selected_option_type']); c.emit(b'\x88\x02')
 # Zero-pad the persisted raw option value, then copy exactly its typed width.
 c.lea_rdx_data(L['selected_option_raw']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00\xc7\x42\x04\x00\x00\x00\x00')
 c.emit(b'\x41\x0f\xb6\x49\x05\xba\x01\x00\x00\x00\xd3\xe2')
 c.emit(b'\x49\x8d\x71\x06'); c.lea_rdi_data(L['selected_option_raw']); c.emit(b'\x89\xd1')
 c.label('selected_option_copy_loop')
 c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','selected_option_copy_done')
 c.emit(b'\x8a\x06\x88\x07\x48\xff\xc6\x48\xff\xc7\xff\xc9'); c.rel32(b'\xe9','selected_option_copy_loop')
 c.label('selected_option_copy_done'); c.emit(b'\x31\xc0\xc3')

 c.label('selected_option_end')
 c.emit(b'\x41\xff\xcb\x41\x83\xfb\x00'); c.rel32(b'\x0f\x84','selected_option_not_found')
 c.rel32(b'\xe9','selected_option_advance')
 c.label('selected_option_scope_advance')
 c.emit(b'\xf6\xc3\x80'); c.rel32(b'\x0f\x84','selected_option_advance')
 c.emit(b'\x41\xff\xc3')
 c.label('selected_option_advance')
 c.emit(b'\x49\x01\xc9\x41\x29\xca'); c.rel32(b'\xe9','selected_option_scan')
 c.label('selected_option_not_found'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('wait_down_key')
 serial('nav_wait')
 c.label('nav_read_key')
 c.lea_rax_data(L['conin_ptr']); c.emit(b'\x48\x8b\x08'); c.lea_rdx_data(L['keybuf'])
 c.emit(b'\x48\x8b\x41\x08\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','nav_read_key')
 c.lea_rdx_data(L['keybuf']); c.emit(b'\x0f\xb7\x02\x66\x83\xf8\x02'); c.rel32(b'\x0f\x85','nav_read_key')
 serial('nav_accept'); c.emit(b'\x31\xc0\xc3')

 c.label('wait_up_key')
 serial('up_wait')
 c.label('up_read_key')
 c.lea_rax_data(L['conin_ptr']); c.emit(b'\x48\x8b\x08'); c.lea_rdx_data(L['keybuf'])
 c.emit(b'\x48\x8b\x41\x08\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','up_read_key')
 c.lea_rdx_data(L['keybuf']); c.emit(b'\x0f\xb7\x02\x66\x83\xf8\x01'); c.rel32(b'\x0f\x85','up_read_key')
 serial('up_accept'); c.emit(b'\x31\xc0\xc3')

 c.label('wait_commit_key')
 serial('commit_wait')
 c.label('commit_read_key')
 c.lea_rax_data(L['conin_ptr']); c.emit(b'\x48\x8b\x08'); c.lea_rdx_data(L['keybuf'])
 c.emit(b'\x48\x8b\x41\x08\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','commit_read_key')
 c.lea_rdx_data(L['keybuf']); c.emit(b'\x0f\xb7\x42\x02\x66\x83\xf8\x0d'); c.rel32(b'\x0f\x85','commit_read_key')
 serial('commit_accept'); c.emit(b'\x31\xc0\xc3')

 c.label('wait_cancel_key')
 serial('cancel_wait')
 c.label('cancel_read_key')
 c.lea_rax_data(L['conin_ptr']); c.emit(b'\x48\x8b\x08'); c.lea_rdx_data(L['keybuf'])
 c.emit(b'\x48\x8b\x41\x08\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','cancel_read_key')
 # Native UEFI keyboards report Escape as SCAN_ESC (0x0017); also accept a Unicode ESC fallback.
 c.lea_rdx_data(L['keybuf']); c.emit(b'\x66\x83\x3a\x17'); c.rel32(b'\x0f\x84','cancel_key_ok')
 c.emit(b'\x0f\xb7\x42\x02\x66\x83\xf8\x1b'); c.rel32(b'\x0f\x85','cancel_read_key')
 c.label('cancel_key_ok')
 serial('cancel_accept'); c.emit(b'\x31\xc0\xc3')

 c.label('write_hex16_4')
 c.emit(b'\xb9\x04\x00\x00\x00')
 c.label('write_hex16_4_loop')
 c.emit(b'\x66\xc1\xc0\x04\x88\xc2\x80\xe2\x0f\x80\xfa\x09'); c.rel8(0x76,'write_hex16_4_digit')
 c.emit(b'\x80\xc2\x37'); c.rel8(0xeb,'write_hex16_4_store')
 c.label('write_hex16_4_digit'); c.emit(b'\x80\xc2\x30')
 c.label('write_hex16_4_store'); c.emit(b'\x88\x17\xc6\x47\x01\x00\x48\x83\xc7\x02\xff\xc9'); c.rel8(0x75,'write_hex16_4_loop')
 c.emit(b'\xc3')

 c.label('build_commit_request')
 c.lea_rax_data(L['config_ptr']); c.emit(b'\x48\x8b\x30\x48\x85\xf6'); c.rel32(b'\x0f\x84','commit_request_bad')
 c.lea_rdi_data(L['commit_request_buf']); c.emit(b'\xb9\x00\x18\x00\x00')
 c.label('commit_hdr_scan')
 c.emit(b'\x66\x83\x3e\x00'); c.rel32(b'\x0f\x84','commit_request_bad')
 c.emit(b'\x66\x83\x3e\x26'); c.rel32(b'\x0f\x85','commit_hdr_copy')
 for off,ch in ((2,0x4f),(4,0x46),(6,0x46),(8,0x53),(10,0x45),(12,0x54),(14,0x3d)):
  c.emit(b'\x66\x83\x7e'+bytes((off,ch))); c.rel32(b'\x0f\x85','commit_hdr_copy')
 c.lea_rsi_data(L['commit_offset_utf16']); c.emit(b'\xb9\x08\x00\x00\x00\xf3\x66\xa5')
 c.lea_rdx_data(L['varstore_info']); c.emit(b'\x0f\xb7\x02'); c.rel32(b'\xe8','write_hex16_4')
 c.lea_rsi_data(L['commit_width_utf16']); c.emit(b'\xb9\x07\x00\x00\x00\xf3\x66\xa5')
 c.lea_rdx_data(L['current_width']); c.emit(b'\x0f\xb6\x02'); c.rel32(b'\xe8','write_hex16_4')
 c.emit(b'\x66\xc7\x07\x00\x00'); serial('commit_request'); c.emit(b'\x31\xc0\xc3')
 c.label('commit_hdr_copy')
 c.emit(b'\x66\x8b\x06\x66\x89\x07\x48\x83\xc6\x02\x48\x83\xc7\x02\xff\xc9'); c.rel32(b'\x0f\x85','commit_hdr_scan')
 c.label('commit_request_bad'); serial('commit_request_fail'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('commit_focused_option')
 # Internal helper calls UEFI services with six arguments: reserve 32-byte
 # shadow space + 16 bytes for stack args 5/6 + 8 bytes for 16-byte alignment.
 c.emit(b'\x48\x83\xec\x38')
 c.lea_rdi_data(L['current_data']); c.lea_rdx_data(L['varstore_info']); c.emit(b'\x0f\xb7\x02\x48\x01\xc7')
 c.lea_rsi_data(L['nav_option_raw']); c.lea_rdx_data(L['current_width']); c.emit(b'\x0f\xb6\x0a')
 c.label('commit_stage_copy'); c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','commit_stage_done')
 c.emit(b'\x8a\x06\x88\x07\x48\xff\xc6\x48\xff\xc7\xff\xc9'); c.rel32(b'\xe9','commit_stage_copy')
 c.label('commit_stage_done'); serial('commit_stage')
 c.rel32(b'\xe8','build_commit_request'); c.emit(b'\x85\xc0'); c.rel32(b'\x0f\x85','commit_return_fail')
 zero_qword(L['commit_config']); zero_qword(L['commit_progress'])
 c.lea_rax_data(L['routing_ptr']); c.emit(b'\x48\x8b\x08'); c.lea_rdx_data(L['commit_request_buf']); c.lea_r8_data(L['current_data'])
 c.lea_rax_data(L['varstore_size']); c.emit(b'\x44\x0f\xb7\x08')
 c.lea_rax_data(L['commit_config']); c.emit(b'\x48\x89\x44\x24\x20'); c.lea_rax_data(L['commit_progress']); c.emit(b'\x48\x89\x44\x24\x28')
 c.emit(b'\x48\x8b\x41\x18\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','commit_block_bad'); serial('commit_block')
 zero_qword(L['commit_progress']); c.lea_rax_data(L['routing_ptr']); c.emit(b'\x48\x8b\x08'); c.lea_rax_data(L['commit_config']); c.emit(b'\x48\x8b\x10'); c.lea_r8_data(L['commit_progress'])
 c.emit(b'\x48\x8b\x41\x10\xff\xd0\x48\x85\xc0'); c.rel32(b'\x0f\x85','commit_route_bad'); serial('commit_route')
 c.lea_rax_data(L['commit_config']); c.emit(b'\x48\x8b\x08\x48\x85\xc9'); c.rel32(b'\x0f\x84','commit_free_results')
 c.emit(b'\x41\xff\x57\x48'); zero_qword(L['commit_config'])
 c.label('commit_free_results'); c.lea_rax_data(L['results']); c.emit(b'\x48\x8b\x08\x48\x85\xc9'); c.rel32(b'\x0f\x84','commit_route_complete')
 c.emit(b'\x41\xff\x57\x48'); zero_qword(L['results'])
 c.label('commit_route_complete')
 # RouteConfig can cause the target firmware driver to rebuild or invalidate its
 # HII/config-routing backing state.  Re-entering ExportConfig in the same boot
 # after a successful RouteConfig provokes a #GP in OVMF's iSCSI driver.
 # Preserve proof integrity: finish the audible confirmation from our already
 # cached live option label, then verify the committed value after rebooting the
 # exact same isolated VARS image in the workflow.
 c.lea_rax_data(L['commit_done']); c.emit(b'\xc6\x00\x01')
 c.emit(b'\x31\xc0\x48\x83\xc4\x38\xc3')
 c.label('commit_block_bad'); serial('commit_block_fail'); c.rel32(b'\xe9','commit_return_fail')
 c.label('commit_route_bad'); serial('commit_route_fail'); c.rel32(b'\xe9','commit_return_fail')
 c.label('commit_verify_bad'); serial('commit_verify_fail')
 c.label('commit_return_fail'); c.emit(b'\xb8\x01\x00\x00\x00\x48\x83\xc4\x38\xc3')

 c.label('resolve_next_option')
 # Re-scan only the direct children of the same scoped ONE_OF. The current
 # option pointer is the boundary: the first later direct numeric option is the
 # read-only DOWN focus target. No HII write service is called.
 c.lea_rax_data(L['question_ptr']); c.emit(b'\x48\x8b\x00\x48\x85\xc0'); c.rel32(b'\x0f\x84','nav_option_not_found')
 c.emit(b'\xf6\x40\x01\x80'); c.rel32(b'\x0f\x84','nav_option_not_found')
 c.lea_rdx_data(L['ifr_next_ptr']); c.emit(b'\x4c\x8b\x0a')
 c.lea_rdx_data(L['ifr_next_remaining']); c.emit(b'\x44\x8b\x12')
 c.emit(b'\x41\xbb\x01\x00\x00\x00\x45\x31\xc0') # depth=1, seen-current=false
 c.label('nav_option_scan')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x82','nav_option_not_found')
 c.emit(b'\x41\x0f\xb6\x01')
 c.emit(b'\x41\x0f\xb6\x59\x01\x89\xd9\x83\xe1\x7f')
 c.emit(b'\x83\xf9\x02'); c.rel32(b'\x0f\x82','nav_option_not_found')
 c.emit(b'\x44\x39\xd1'); c.rel32(b'\x0f\x87','nav_option_not_found')
 c.emit(b'\x3c\x29'); c.rel32(b'\x0f\x84','nav_option_end')
 c.emit(b'\x3c\x09'); c.rel32(b'\x0f\x85','nav_option_scope_advance')
 c.emit(b'\x41\x83\xfb\x01'); c.rel32(b'\x0f\x85','nav_option_scope_advance')
 c.emit(b'\x45\x85\xc0'); c.rel32(b'\x0f\x85','nav_option_candidate')
 c.lea_rdx_data(L['selected_option_ptr']); c.emit(b'\x4c\x3b\x0a'); c.rel32(b'\x0f\x85','nav_option_scope_advance')
 c.emit(b'\x41\xb8\x01\x00\x00\x00'); c.rel32(b'\xe9','nav_option_scope_advance')

 c.label('nav_option_candidate')
 c.emit(b'\x83\xf9\x07'); c.rel32(b'\x0f\x82','nav_option_advance')
 c.emit(b'\x41\x89\xc8')
 c.emit(b'\x41\x0f\xb6\x41\x05\x3c\x03'); c.rel32(b'\x0f\x87','nav_option_advance')
 c.lea_rdx_data(L['oneof_flags']); c.emit(b'\x0f\xb6\x12\x83\xe2\x03\x39\xd0'); c.rel32(b'\x0f\x85','nav_option_advance')
 c.emit(b'\x89\xc1\xba\x01\x00\x00\x00\xd3\xe2\x8d\x42\x06\x41\x39\xc0'); c.rel32(b'\x0f\x82','nav_option_restore_advance')
 c.emit(b'\x44\x89\xc1')
 c.emit(b'\x41\x0f\xb7\x41\x02'); c.lea_rdx_data(L['nav_option_token']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb6\x41\x05'); c.lea_rdx_data(L['nav_option_type']); c.emit(b'\x88\x02')
 c.lea_rdx_data(L['nav_option_raw']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00\xc7\x42\x04\x00\x00\x00\x00')
 c.emit(b'\x41\x0f\xb6\x49\x05\xba\x01\x00\x00\x00\xd3\xe2')
 c.emit(b'\x49\x8d\x71\x06'); c.lea_rdi_data(L['nav_option_raw']); c.emit(b'\x89\xd1')
 c.label('nav_option_copy_loop')
 c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','nav_option_copy_done')
 c.emit(b'\x8a\x06\x88\x07\x48\xff\xc6\x48\xff\xc7\xff\xc9'); c.rel32(b'\xe9','nav_option_copy_loop')
 c.label('nav_option_copy_done'); c.emit(b'\x31\xc0\xc3')
 c.label('nav_option_restore_advance'); c.emit(b'\x44\x89\xc1'); c.rel32(b'\xe9','nav_option_advance')
 c.label('nav_option_end')
 c.emit(b'\x41\xff\xcb\x41\x83\xfb\x00'); c.rel32(b'\x0f\x84','nav_option_not_found')
 c.rel32(b'\xe9','nav_option_advance')
 c.label('nav_option_scope_advance')
 c.emit(b'\xf6\xc3\x80'); c.rel32(b'\x0f\x84','nav_option_advance'); c.emit(b'\x41\xff\xc3')
 c.label('nav_option_advance')
 c.emit(b'\x49\x01\xc9\x41\x29\xca'); c.rel32(b'\xe9','nav_option_scan')
 c.label('nav_option_not_found'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('emit_nav_meta')
 serial('nav_token'); c.lea_rsi_data(L['nav_option_token']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('nav_token_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','nav_token_hex_loop')
 serial('nav_type'); c.lea_rax_data(L['nav_option_type']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('nav_raw'); c.lea_rsi_data(L['nav_option_raw']); c.emit(b'\xb9\x08\x00\x00\x00')
 c.label('nav_raw_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','nav_raw_hex_loop')
 serial('nav_done'); c.emit(b'\xc3')

 c.label('resolve_prev_option')
 # Read-only UP focus. Track the preceding direct numeric ONE_OF_OPTION; if
 # current is first, continue to the scope end and wrap to the last sibling.
 c.lea_rax_data(L['question_ptr']); c.emit(b'\x48\x8b\x00\x48\x85\xc0'); c.rel32(b'\x0f\x84','prev_option_not_found')
 c.emit(b'\xf6\x40\x01\x80'); c.rel32(b'\x0f\x84','prev_option_not_found')
 c.lea_rdx_data(L['ifr_next_ptr']); c.emit(b'\x4c\x8b\x0a')
 c.lea_rdx_data(L['ifr_next_remaining']); c.emit(b'\x44\x8b\x12')
 c.emit(b'\x41\xbb\x01\x00\x00\x00')
 c.lea_rdx_data(L['prev_option_ptr']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00')
 c.lea_rdx_data(L['prev_wrap_flag']); c.emit(b'\xc6\x02\x00')
 c.label('prev_option_scan')
 c.emit(b'\x41\x83\xfa\x02'); c.rel32(b'\x0f\x82','prev_option_not_found')
 c.emit(b'\x41\x0f\xb6\x01')
 c.emit(b'\x41\x0f\xb6\x59\x01\x89\xd9\x83\xe1\x7f')
 c.emit(b'\x83\xf9\x02'); c.rel32(b'\x0f\x82','prev_option_not_found')
 c.emit(b'\x44\x39\xd1'); c.rel32(b'\x0f\x87','prev_option_not_found')
 c.emit(b'\x3c\x29'); c.rel32(b'\x0f\x84','prev_option_end')
 c.emit(b'\x3c\x09'); c.rel32(b'\x0f\x85','prev_option_scope_advance')
 c.emit(b'\x41\x83\xfb\x01'); c.rel32(b'\x0f\x85','prev_option_scope_advance')
 c.lea_rdx_data(L['selected_option_ptr']); c.emit(b'\x4c\x3b\x0a'); c.rel32(b'\x0f\x85','prev_option_candidate')
 # Do not load prev_option_ptr into R9 merely to test it: R9 is the live IFR
 # scan cursor.  In the first-option wrap case prev_option_ptr is zero, and
 # clobbering R9 here would restart the scan near address zero.
 c.lea_rax_data(L['prev_option_ptr']); c.emit(b'\x48\x83\x38\x00'); c.rel32(b'\x0f\x84','prev_option_mark_wrap')
 c.emit(b'\x4c\x8b\x08'); c.rel32(b'\xe9','prev_option_capture')
 c.label('prev_option_mark_wrap')
 c.lea_rdx_data(L['prev_wrap_flag']); c.emit(b'\xc6\x02\x01'); c.rel32(b'\xe9','prev_option_advance')

 c.label('prev_option_candidate')
 c.emit(b'\x83\xf9\x07'); c.rel32(b'\x0f\x82','prev_option_advance')
 c.emit(b'\x41\x89\xc8')
 c.emit(b'\x41\x0f\xb6\x41\x05\x3c\x03'); c.rel32(b'\x0f\x87','prev_option_advance')
 c.lea_rdx_data(L['oneof_flags']); c.emit(b'\x0f\xb6\x12\x83\xe2\x03\x39\xd0'); c.rel32(b'\x0f\x85','prev_option_restore_advance')
 c.emit(b'\x89\xc1\xba\x01\x00\x00\x00\xd3\xe2\x8d\x42\x06\x41\x39\xc0'); c.rel32(b'\x0f\x82','prev_option_restore_advance')
 c.lea_rdx_data(L['prev_option_ptr']); c.emit(b'\x4c\x89\x0a')
 c.emit(b'\x44\x89\xc1'); c.rel32(b'\xe9','prev_option_advance')
 c.label('prev_option_restore_advance'); c.emit(b'\x44\x89\xc1'); c.rel32(b'\xe9','prev_option_advance')

 c.label('prev_option_end')
 c.emit(b'\x41\xff\xcb\x41\x83\xfb\x00'); c.rel32(b'\x0f\x85','prev_option_advance')
 c.lea_rax_data(L['prev_wrap_flag']); c.emit(b'\x80\x38\x01'); c.rel32(b'\x0f\x85','prev_option_not_found')
 c.lea_rax_data(L['prev_option_ptr']); c.emit(b'\x4c\x8b\x08\x4d\x85\xc9'); c.rel32(b'\x0f\x84','prev_option_not_found')
 c.rel32(b'\xe9','prev_option_capture')
 c.label('prev_option_scope_advance')
 c.emit(b'\xf6\xc3\x80'); c.rel32(b'\x0f\x84','prev_option_advance'); c.emit(b'\x41\xff\xc3')
 c.label('prev_option_advance')
 c.emit(b'\x49\x01\xc9\x41\x29\xca'); c.rel32(b'\xe9','prev_option_scan')

 c.label('prev_option_capture')
 c.emit(b'\x41\x0f\xb7\x41\x02'); c.lea_rdx_data(L['nav_option_token']); c.emit(b'\x66\x89\x02')
 c.emit(b'\x41\x0f\xb6\x41\x05'); c.lea_rdx_data(L['nav_option_type']); c.emit(b'\x88\x02')
 c.lea_rdx_data(L['nav_option_raw']); c.emit(b'\x48\xc7\x02\x00\x00\x00\x00\xc7\x42\x04\x00\x00\x00\x00')
 c.emit(b'\x41\x0f\xb6\x49\x05\xba\x01\x00\x00\x00\xd3\xe2')
 c.emit(b'\x49\x8d\x71\x06'); c.lea_rdi_data(L['nav_option_raw']); c.emit(b'\x89\xd1')
 c.label('prev_option_copy_loop')
 c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','prev_option_copy_done')
 c.emit(b'\x8a\x06\x88\x07\x48\xff\xc6\x48\xff\xc7\xff\xc9'); c.rel32(b'\xe9','prev_option_copy_loop')
 c.label('prev_option_copy_done'); c.emit(b'\x31\xc0\xc3')
 c.label('prev_option_not_found'); c.emit(b'\xb8\x01\x00\x00\x00\xc3')

 c.label('emit_up_meta')
 serial('up_token'); c.lea_rsi_data(L['nav_option_token']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('up_token_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','up_token_hex_loop')
 serial('up_type'); c.lea_rax_data(L['nav_option_type']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('up_raw'); c.lea_rsi_data(L['nav_option_raw']); c.emit(b'\xb9\x08\x00\x00\x00')
 c.label('up_raw_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','up_raw_hex_loop')
 serial('up_done'); c.emit(b'\xc3')

 c.label('emit_question_meta')
 serial('meta_qid'); c.lea_rsi_data(L['question_id']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('qid_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','qid_hex_loop')
 serial('meta_varstore'); c.lea_rsi_data(L['varstore_id']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('varstore_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','varstore_hex_loop')
 serial('meta_varinfo'); c.lea_rsi_data(L['varstore_info']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('varinfo_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','varinfo_hex_loop')
 serial('meta_qflags'); c.lea_rax_data(L['question_flags']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('meta_oneof'); c.lea_rax_data(L['oneof_flags']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('meta_done'); c.emit(b'\xc3')

 c.label('emit_varstore_meta')
 serial('vs_opcode'); c.lea_rax_data(L['varstore_opcode']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('vs_size'); c.lea_rsi_data(L['varstore_size']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('vs_size_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','vs_size_loop')
 serial('vs_attrs'); c.lea_rsi_data(L['varstore_attrs']); c.emit(b'\xb9\x04\x00\x00\x00')
 c.label('vs_attrs_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','vs_attrs_loop')
 serial('vs_guid'); c.lea_rsi_data(L['varstore_guid']); c.emit(b'\xb9\x10\x00\x00\x00')
 c.label('vs_guid_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','vs_guid_loop')
 serial('vs_match'); c.emit(b'\xc3')

 c.label('emit_current_meta')
 serial('cur_width'); c.lea_rax_data(L['current_width']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('cur_raw'); c.lea_rsi_data(L['current_raw']); c.emit(b'\xb9\x08\x00\x00\x00')
 c.label('buffer_raw_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','buffer_raw_loop')
 serial('cur_done'); c.emit(b'\xc3')

 c.label('emit_selected_meta')
 serial('sel_token'); c.lea_rsi_data(L['selected_option_token']); c.emit(b'\xb9\x02\x00\x00\x00')
 c.label('selected_token_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','selected_token_hex_loop')
 serial('sel_type'); c.lea_rax_data(L['selected_option_type']); c.emit(b'\x8a\x00'); c.rel32(b'\xe8','hex8_emit')
 serial('sel_raw'); c.lea_rsi_data(L['selected_option_raw']); c.emit(b'\xb9\x08\x00\x00\x00')
 c.label('selected_raw_hex_loop'); c.emit(b'\x8a\x06'); c.rel32(b'\xe8','hex8_emit'); c.emit(b'\x48\xff\xc6\xff\xc9'); c.rel32(b'\x0f\x85','selected_raw_hex_loop')
 serial('sel_done'); c.emit(b'\xc3')

 c.label('hex8_emit')
 c.emit(b'\x41\x88\xc3\xc0\xe8\x04'); c.rel32(b'\xe8','hex_nibble_emit')
 c.emit(b'\x44\x88\xd8\x24\x0f'); c.rel32(b'\xe8','hex_nibble_emit'); c.emit(b'\xc3')
 c.label('hex_nibble_emit')
 c.emit(b'\x3c\x09'); c.rel32(b'\x0f\x86','hex_digit')
 c.emit(b'\x04\x37'); c.rel32(b'\xe9','hex_char_ready')
 c.label('hex_digit'); c.emit(b'\x04\x30')
 c.label('hex_char_ready'); c.emit(b'\x88\xc3\x66\xba\xfd\x03')
 c.label('hex_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'hex_wait')
 c.emit(b'\x66\xba\xf8\x03\x88\xd8\xee\xc3')

 c.label('hex_utf16_nibble')
 # EAX contains one UTF-16 code unit. Return AL=0..15 or 0xff when it is not
 # an ASCII hexadecimal digit. Accept both upper and lower case.
 c.emit(b'\x84\xe4'); c.rel32(b'\x0f\x85','hex_utf16_invalid')
 c.emit(b'\x3c\x30'); c.rel32(b'\x0f\x82','hex_utf16_invalid')
 c.emit(b'\x3c\x39'); c.rel32(b'\x0f\x86','hex_utf16_digit')
 c.emit(b'\x0c\x20\x3c\x61'); c.rel32(b'\x0f\x82','hex_utf16_invalid')
 c.emit(b'\x3c\x66'); c.rel32(b'\x0f\x87','hex_utf16_invalid')
 c.emit(b'\x2c\x57\xc3')
 c.label('hex_utf16_digit'); c.emit(b'\x2c\x30\xc3')
 c.label('hex_utf16_invalid'); c.emit(b'\xb0\xff\xc3')

 # Immediate Command helper, MMIO base r14, command eax.
 c.label('immediate')
 c.emit(b'\x41\x89\xc0\xb9\xa0\x86\x01\x00')
 c.label('ic_ready_poll')
 c.emit(b'\x41\x0f\xb7\x56\x68\xf6\xc2\x01'); c.rel32(b'\x0f\x84','ic_ready')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','ic_ready_poll'); c.rel32(b'\xe9','ic_timeout')
 c.label('ic_ready')
 c.emit(b'\x66\x41\xc7\x46\x68\x02\x00')
 c.emit(b'\x45\x89\x46\x60')
 c.emit(b'\x66\x41\xc7\x46\x68\x01\x00')
 c.emit(b'\xb9\xa0\x86\x01\x00')
 c.label('irv_poll')
 c.emit(b'\x41\x0f\xb7\x56\x68\xf6\xc2\x02'); c.rel32(b'\x0f\x85','irv_ready')
 c.emit(b'\xff\xc9'); c.rel32(b'\x0f\x85','irv_poll')
 c.label('ic_timeout'); c.emit(b'\xb8\xff\xff\xff\xff\xc3')
 c.label('irv_ready')
 c.emit(b'\x41\x8b\x46\x64')
 c.emit(b'\x66\x41\xc7\x46\x68\x02\x00\xc3')

 c.label('pci_read32'); c.emit(b'\x66\xba\xf8\x0c\xef\x66\xba\xfc\x0c\xed\xc3')
 c.label('pci_write32'); c.emit(b'\x66\xba\xf8\x0c\xef\x89\xc8\x66\xba\xfc\x0c\xef\xc3')

 c.label('serial_textbuf')
 c.lea_rsi_data(L['textbuf']); c.lea_rdx_data(L['text_count']); c.emit(b'\x8b\x0a')
 c.label('serial_textbuf_loop')
 c.emit(b'\x85\xc9'); c.rel32(b'\x0f\x84','serial_textbuf_done')
 c.emit(b'\x0f\xb7\x06\x48\x83\xc6\x02\x88\xc3\x66\xba\xfd\x03')
 c.label('serial_textbuf_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'serial_textbuf_wait')
 c.emit(b'\x66\xba\xf8\x03\x88\xd8\xee\xff\xc9'); c.rel32(b'\xe9','serial_textbuf_loop')
 c.label('serial_textbuf_done'); c.emit(b'\xc3')

 c.label('serial_emit')
 c.emit(b'\x49\x89\xd0\x66\xba\xfd\x03')
 c.label('serial_wait'); c.emit(b'\xec\xa8\x20'); c.rel8(0x74,'serial_wait')
 c.emit(b'\x66\xba\xf8\x03\x41\x8a\x00\xee\x49\xff\xc0\x66\xba\xfd\x03\xff\xc9')
 c.rel8(0x75,'serial_wait'); c.emit(b'\xc3')

 c.patch(); code=bytes(c.data)
 if len(code)>0x7000: raise SystemExit(f'HII current-option speech code too large: {len(code)}')

 text_raw=0x200; text_raw_size=(len(code)+0x1ff)&~0x1ff
 data_raw=text_raw+text_raw_size; data_raw_size=(len(data)+0x1ff)&~0x1ff
 reloc_rva=(DATA_RVA+len(data)+0xfff)&~0xfff; reloc_raw=data_raw+data_raw_size
 image=bytearray(reloc_raw+0x200); image_size=reloc_rva+0x1000
 put(image,0,'<H',0x5a4d); put(image,0x3c,'<I',0x80)
 pe=0x80; image[pe:pe+4]=b'PE\0\0'; coff=pe+4
 put(image,coff,'<HHIIIHH',0x8664,3,0,0,0,0xf0,0x22); opt=coff+20
 put(image,opt,'<H',0x20b); put(image,opt+4,'<I',text_raw_size)
 put(image,opt+8,'<I',data_raw_size+0x200); put(image,opt+0x10,'<I',TEXT_RVA)
 put(image,opt+0x14,'<I',TEXT_RVA); put(image,opt+0x18,'<Q',0x400000)
 put(image,opt+0x20,'<I',0x1000); put(image,opt+0x24,'<I',0x200)
 put(image,opt+0x38,'<I',image_size); put(image,opt+0x3c,'<I',0x200)
 put(image,opt+0x44,'<H',10); put(image,opt+0x48,'<Q',0x100000)
 put(image,opt+0x50,'<Q',0x1000); put(image,opt+0x58,'<Q',0x100000)
 put(image,opt+0x60,'<Q',0x1000); put(image,opt+0x6c,'<I',16)
 put(image,opt+0x70+5*8,'<II',reloc_rva,8)
 sec=opt+0xf0; image[sec:sec+8]=b'.text\0\0\0'
 put(image,sec+8,'<I',len(code)); put(image,sec+0xc,'<I',TEXT_RVA)
 put(image,sec+0x10,'<I',text_raw_size); put(image,sec+0x14,'<I',text_raw)
 put(image,sec+0x24,'<I',0x60000020)
 ds=sec+40; image[ds:ds+8]=b'.data\0\0\0'
 put(image,ds+8,'<I',len(data)); put(image,ds+0xc,'<I',DATA_RVA)
 put(image,ds+0x10,'<I',data_raw_size); put(image,ds+0x14,'<I',data_raw)
 put(image,ds+0x24,'<I',0xc0000040)
 rs=sec+80; image[rs:rs+8]=b'.reloc\0\0'
 put(image,rs+8,'<I',8); put(image,rs+0xc,'<I',reloc_rva)
 put(image,rs+0x10,'<I',0x200); put(image,rs+0x14,'<I',reloc_raw)
 put(image,rs+0x24,'<I',0x42000040)
 image[text_raw:text_raw+len(code)]=code; image[data_raw:data_raw+len(data)]=data
 put(image,reloc_raw,'<II',TEXT_RVA,8)
 return bytes(image),pcm

def validate(image,pcm):
 assert image[:2]==b'MZ'
 assert pcm in image
 for token in (
  b'HII_DATABASE_PROTOCOL=PASS',
  b'HII_EXPORT_ALL_PACKAGE_LISTS=PASS',
  b'HII_FORMS_PACKAGE=PASS',
  b'HII_STRINGS_PACKAGE=PASS',
  b'IFR_QUESTION_PROMPT_STRING_ID=PASS',
  b'QUESTION_TEXT=',
  b'HII_QUESTION_STRING=PASS',
  b'QUESTION_METADATA=PASS',
  b'VARSTORE_DEFINITION_MATCH=PASS',
  b'VARSTORE_NAME_CONFIG_MATCH=PASS',
  b'HII_CONFIG_ROUTING_PROTOCOL=PASS',
  b'HII_HANDLE_LIST=PASS',
  b'HII_CONFIG_ROUTING_EXTERNAL_CALLER=PASS',
  b'CONFIG_ROUTING_EXPORT_CONFIG=PASS',
  b'CONFIG_TO_BLOCK=PASS',
  b'BUFFER_VARSTORE_MATCH=PASS',
  b'CURRENT_VALUE_READ=PASS',
  b'CURRENT_VALUE_BYTES_HEX=',
  b'CURRENT_VALUE_RAW8_HEX=',
  b'SELECTED_OPTION_STRING_ID_LE_HEX=',
  b'SELECTED_OPTION_TYPE_HEX=',
  b'SELECTED_OPTION_VALUE_RAW8_HEX=',
  b'SELECTED_OPTION_CHILD_MATCH=PASS',
  b'SELECTED_OPTION_CURRENT_MATCH=PASS',
  b'SELECTED_OPTION_TEXT=',
  b'SELECTED_OPTION_STRING=PASS',
  b'SELECTED_OPTION_LABEL_BINDING=PASS',
  b'CURRENT_OPTION_SOURCE=PASS',
  b'CURRENT_OPTION_SPOKEN_PREFIX=',
  b'CURRENT_OPTION_SPOKEN_PREFIX=PASS',
  b'NAV_FOCUS_OPTION_TEXT=',
  b'NAV_FOCUS_OPTION_STRING=PASS',
  b'NAV_FOCUS_LABEL_BINDING=PASS',
  b'NAV_FOCUS_SOURCE=PASS',
  b'NAV_FOCUS_SPOKEN_PREFIX=PASS',
  b'NAV_FOCUS_SPEECH_HDA=PASS',
  b'HII_COMMIT_KEY=WAIT_ENTER',
  b'HII_CANCEL_KEY=WAIT_ESC',
  b'HII_CANCEL_KEY=PASS',
  b'HII_CANCEL_NO_ROUTE=PASS',
  b'HII_COMMIT_KEY=PASS',
  b'HII_COMMIT_CONFIG_REQUEST=PASS',
  b'BLOCK_TO_CONFIG=PASS',
  b'ROUTE_CONFIG=PASS',
  b'POST_COMMIT_REREAD=PASS',
  b'COMMIT_CONFIRMATION_SPEECH_HDA=PASS',
  b'HDA_CONTROLLER_CODEC=PASS',
  b'AFG_RUNTIME_DISCOVERY=PASS',
  b'BDL_RUNTIME_TEXT_SCHEDULE=PASS',
  b'LPIB_PROGRESS=PASS',
  b'CURRENT_OPTION_SPEECH_HDA=PASS',
  b'VARSTORE_OPCODE_HEX=',
  b'VARSTORE_SIZE_LE_HEX=',
  b'VARSTORE_ATTRIBUTES_LE_HEX=',
  b'VARSTORE_GUID_RAW_HEX=',
  b'QUESTION_ID_LE_HEX=',
  b'VARSTORE_ID_LE_HEX=',
  b'VARSTORE_INFO_LE_HEX=',
  b'QUESTION_FLAGS_HEX=',
  b'ONEOF_FLAGS_HEX=',
 ):
  assert token in image,token

def main():
 global WAIT_REPEAT_KEY, WAIT_DOWN_PROBE, WAIT_DOWN_SPEAK, WAIT_UP_PROBE, WAIT_UP_SPEAK, WAIT_DOWN_COMMIT, WAIT_DOWN_CANCEL, EFI_VARSTORE_ONLY, PLATFORM_TARGET_ONLY
 if len(sys.argv) not in {2,3}: raise SystemExit('usage: build_uefi_hii_current_option_speech.py OUTPUT_EFI [--wait-repeat|--wait-down-probe|--wait-down-speak|--wait-up-probe|--wait-up-speak|--wait-down-repeat-speak|--wait-down-commit|--wait-down-cancel|--efivar-probe|--platform-probe|--wait-platform-commit|--wait-platform-cancel]')
 if len(sys.argv)==3:
  if sys.argv[2]=='--wait-repeat': WAIT_REPEAT_KEY=True
  elif sys.argv[2]=='--wait-down-probe': WAIT_DOWN_PROBE=True
  elif sys.argv[2]=='--wait-down-speak': WAIT_DOWN_SPEAK=True
  elif sys.argv[2]=='--wait-up-probe': WAIT_UP_PROBE=True
  elif sys.argv[2]=='--wait-up-speak': WAIT_UP_SPEAK=True
  elif sys.argv[2]=='--wait-down-repeat-speak': WAIT_DOWN_SPEAK=True; WAIT_REPEAT_KEY=True
  elif sys.argv[2]=='--wait-down-commit': WAIT_DOWN_COMMIT=True
  elif sys.argv[2]=='--wait-down-cancel': WAIT_DOWN_CANCEL=True
  elif sys.argv[2]=='--efivar-probe': EFI_VARSTORE_ONLY=True
  elif sys.argv[2]=='--platform-probe': PLATFORM_TARGET_ONLY=True
  elif sys.argv[2]=='--wait-platform-commit': PLATFORM_TARGET_ONLY=True; WAIT_DOWN_COMMIT=True
  elif sys.argv[2]=='--wait-platform-cancel': PLATFORM_TARGET_ONLY=True; WAIT_DOWN_CANCEL=True
  else: raise SystemExit('unknown mode: '+sys.argv[2])
 image,pcm=build(); validate(image,pcm)
 p=Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(image)
 print('OS_UEFI_HII_CURRENT_OPTION_SPEECH_BUILD=PASS')
 print('bytes='+str(len(image)))
 print('current-option-graphemes=a-z')
 print('current-option-max-spoken-graphemes=8')
 print('accessibility-repeat-key=' + ('enabled' if WAIT_REPEAT_KEY else 'disabled'))
 print('hii-down-probe=' + ('enabled' if WAIT_DOWN_PROBE else 'disabled'))
 print('hii-down-speak=' + ('enabled' if WAIT_DOWN_SPEAK else 'disabled'))
 print('hii-up-probe=' + ('enabled' if WAIT_UP_PROBE else 'disabled'))
 print('efi-varstore-only=' + ('enabled' if EFI_VARSTORE_ONLY else 'disabled'))
 print('platform-target-only=' + ('enabled' if PLATFORM_TARGET_ONLY else 'disabled'))
 print('hii-up-speak=' + ('enabled' if WAIT_UP_SPEAK else 'disabled'))
 print('hii-down-repeat-speak=' + ('enabled' if (WAIT_DOWN_SPEAK and WAIT_REPEAT_KEY) else 'disabled'))
 print('hii-down-commit=' + ('enabled' if WAIT_DOWN_COMMIT else 'disabled'))
 print('hii-down-cancel=' + ('enabled' if WAIT_DOWN_CANCEL else 'disabled'))
 print('pcm-bytes='+str(len(pcm)))
 print('pcm-sha256='+hashlib.sha256(pcm).hexdigest())
 print('sha256='+hashlib.sha256(image).hexdigest())

if __name__=='__main__':
 main()
