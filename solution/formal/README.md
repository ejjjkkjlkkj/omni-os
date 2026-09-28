# Formal verification

The CBMC harness reasons about the actual implementation in `c/omni_protocol.c`, not a duplicate model.

Required properties include:
- no successful MM message can violate magic/version/size/command bounds;
- a successful MM payload is bounded by the supplied buffer;
- a password accessibility node cannot export a non-empty value;
- an accepted accessibility node must have a non-empty name.

Frama-C analyzes the same implementation. ACSL contracts are attached to the public declarations in `include/omni_protocol.h`.
