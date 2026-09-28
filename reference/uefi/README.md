# UEFI accessibility reference (from accessible-windows)

Rust source of the sibling `accessible-windows` project's UEFI accessibility
bootloader and accessibility crates, captured here for reference (not built by
this repo's CI). Large binary speech assets (.pcm/.bin) were omitted; the source
(.rs/.toml) is kept. Covers a parallel implementation of HDA audio, HII/IFR,
screen-reader and braille at the UEFI boot layer.
