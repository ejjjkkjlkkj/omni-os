#!/usr/bin/env python3
from __future__ import annotations
import json, re
from pathlib import Path

HERE=Path(__file__).resolve().parent
data=json.loads((HERE/"profiles"/"asus_m1603qa_308.json").read_text(encoding="utf-8"))
source=(HERE/"hii_graph_prompt_speech_uefi.c").read_text(encoding="utf-8")

assert data["schema"]=="m1603qa-308-runtime-profile-v1"
assert data["source_zip_sha256"]=="cf9f3058709626b42f51aff11eecb4d482d431906fe5dc76e1c31b017ad00eed"
assert data["bios_sha256"]=="12a932605d3a1ca35dd5023b14a0c7fdc2f8713465655265795f0f9799aaa0de"
assert data["formset_guid"]=="7b59104a-c00d-4158-87ff-f04d6396a915"
assert data["package_list_guid"]=="899407d7-99fe-43d8-9a21-79ec328cac21"
assert data["root_form_id"]==10000
assert data["statistics"]=={"forms":63,"varstores":48,"controls":791,"english_strings":1336,"max_controls_per_form":106}
cond=data["condition_engine"]
assert cond["total_conditions"]==368
assert cond["suppress_if"]==243 and cond["gray_out_if"]==124 and cond["disable_if"]==1
assert cond["expression_patterns"]==13
assert cond["expression_opcodes"]=={"eq_id_val":473,"eq_id_id":3,"eq_id_val_list":29,"and":20,"or":130,"not":32,"true":13}
assert cond["referenced_questions_unique"]==142
assert cond["live_scalar_resolvable_questions"]==140
assert cond["callback_or_password_questions"]==[380,381]
assert cond["callback_or_password_condition_count"]==4
assert [x["form_id"] for x in data["active_notebook_root"]]==[10006,10007,10008,10009,10010]
m=re.search(r"#define\s+MAX_HII_NAV_PROMPTS\s+(\d+)",source)
assert m and int(m.group(1))>=106
for marker in (
"HII_GRAPH_NAV_PROFILE=M1603QA_BIOS_308",
"HII_GRAPH_NAV_PACKAGE_GUID_MATCH=PASS",
"HII_GRAPH_NAV_ROOT_FORM_2710=PASS",
"HII_GRAPH_NAV_M1603QA_ROOT_FILTER=PASS",
"HII_GRAPH_NAV_FORM_AWARE=PASS",
"HII_GRAPH_NAV_FORM_LOAD=PASS",
"HII_GRAPH_NAV_FORM_ENTER=PASS",
"HII_GRAPH_NAV_FORM_BACK=PASS",
"HII_GRAPH_NAV_FORM_TITLE_SPEECH=PASS",
"HII_GRAPH_NAV_READ_ONLY_ACTION=BLOCKED",
"HII_GRAPH_NAV_SELF_REF_ACTION=BLOCKED",
"HII_GRAPH_NAV_CONDITION_EVALUATOR=PASS",
"HII_GRAPH_NAV_SUPPRESS_RUNTIME=PASS",
"HII_GRAPH_NAV_GRAY_RUNTIME=PASS",
"HII_GRAPH_NAV_DISABLE_RUNTIME=PASS",
"HII_GRAPH_NAV_CONDITION_UNKNOWN_SAFE=PASS",
"HII_GRAPH_NAV_QUESTION_CATALOG=PASS",
):
    assert marker in source, marker
for symbol in (
"g_m1603qa_308_setup_package_list_guid","guid_bytes_equal","nav_get_form_title",
"nav_eval_condition_expression","nav_read_question_value","nav_question_add","nav_find_question",
"nav_load_form","g_nav_ref_form_ids","g_nav_form_history","key.unicode_char == 0x000du",
):
    assert symbol in source, symbol
print("M1603QA_308_EXACT_PROFILE=PASS")
print("M1603QA_308_FORM_BROWSER=PASS")
print("M1603QA_308_READ_ONLY_NAVIGATION=PASS")
