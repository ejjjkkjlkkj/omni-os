QEVARYNOX-UEFI-PHYSICAL-BOOT-V1
target = ASUS M1603QA / AMD Ryzen 7 5800H
source = boot/uefi-hii-graph-prompt-speech-v1/hii_graph_prompt_speech_uefi.c
boot-path = EFI/BOOT/BOOTX64.EFI
disk-layout = GPT + EFI System Partition
firmware-nvram-write = forbidden
host-windows-filesystem-write = forbidden
efi-runtime-write-scope = boot media only
expected-controller = PCI_VEN_1022_DEV_15E3
expected-codec = HDAUDIO_VEN_10EC_DEV_0256
expected-controller-selection = PREFERRED_AMD_1022_15E3
expected-proof-file = QEVARYNOX-PHYSICAL-PROOF.TXT
physical-controls = Up previous prompt; Down next prompt; Home first prompt; End last prompt; PageUp minus 5 prompts; PageDown plus 5 prompts; R repeat; Esc exits and persists proof only after all required spoken navigation events have passed
proof-navigation = HII_GRAPH_NAV_UP=PASS + HII_GRAPH_NAV_DOWN=PASS + HII_GRAPH_NAV_HOME=PASS + HII_GRAPH_NAV_END=PASS + HII_GRAPH_NAV_PAGE_UP=PASS + HII_GRAPH_NAV_PAGE_DOWN=PASS + HII_GRAPH_NAV_REPEAT=PASS + HII_GRAPH_NAV_REQUIRED_EVENTS=PASS + HII_GRAPH_NAV_EXIT=PASS
proof-navigation-speech-events = at least 7
proof-navigation-incomplete-exit = HII_GRAPH_NAV_EXIT=BLOCKED_INCOMPLETE
proof-DMA = HII_GRAPH_SPEECH_DMA=PASS + LPIB_PROGRESS=PASS
proof-DMA-reuse = HII_GRAPH_SPEECH_DMA_REUSE=PASS
proof-speech-mode = HII_GRAPH_SPEECH_MODE=CLEAR_LETTERNAME_SPELLING_FR_V3
audible-speaker-proof = human confirmation remains required
claim = a valid proof file after a real ASUS UEFI boot establishes native execution, analog HDA controller selection, live graph routing, selector programming and DMA progress
claim = the proof file alone does not establish that the laptop speaker was audibly heard
proof-internal-speaker-path = PHYSICAL_ASUS_M1603QA_HDA_RUNTIME=PASS + PHYSICAL_ASUS_M1603QA_CODEC=REALTEK_10EC_0256 + PHYSICAL_ASUS_M1603QA_INTERNAL_SPEAKER_PIN=PASS
proof-route-readback = HDA_ROUTE_POWER_D0=PASS + HDA_ROUTE_AMPLIFIERS=PASS + HDA_EAPD_POLICY=PASS + HDA_DAC_STREAM_READBACK=PASS + HDA_PIN_CONTROL_READBACK=PASS
proof-selector-count = HDA_SELECTOR_WRITES_REQUIRED must equal HDA_SELECTOR_WRITES_APPLIED
closure = audible confirmation is accepted only after machine-bound HDA runtime, internal-speaker path, route readbacks, DMA, and navigation all pass
proof-source-provenance = UEFI_SOURCE_BLOB must equal the current git blob of hii_graph_prompt_speech_uefi.c
proof-buffer-integrity = proof buffer is 4096 bytes and any overflow makes BOOT_MEDIA_PERSISTENT_PROOF fail
