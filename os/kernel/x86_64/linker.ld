/*
 * Accessible Windows native x86-64 kernel image layout.
 *
 * The kernel ships as a flat `objcopy -O binary` image. Previously it was
 * linked from virtual address 0 and loaded at an arbitrary firmware-chosen
 * address with no relocations applied, which silently broke every construct
 * the compiler does not keep RIP-relative:
 *
 *   - `&'static str` rodata pointer tables read back as zero (blank logs);
 *   - a `.got` entry pointed at physical 0xb000, so kernel statics stomped
 *     low memory instead of living inside the image;
 *   - absolute `match` jump tables transferred control into unmapped memory
 *     (observed as #UD/#PF at low RIPs).
 *
 * The image is now linked at a fixed load base and the UEFI loader is
 * required to place it at exactly that address, so absolute references are
 * correct by construction and the build can use the plain `static`
 * relocation model with jump tables enabled.
 *
 * The first bytes of the image are an `AWKN` header describing the load base,
 * the on-disk length, the total memory footprint (BSS included) and the entry
 * point, so the loader never has to hardcode any of them.
 */

ENTRY(_start)

/*
 * 2 MiB. Below the firmware's own runtime allocations, above the legacy
 * low-memory region (IVT/BDA/EBDA/VGA/option ROMs) that UEFI keeps reserved,
 * and 2 MiB-aligned so a future large-page mapping of the kernel is free.
 */
AW_KERNEL_LOAD_BASE = 0x0000000000200000;

AW_IMAGE_HEADER_MAGIC = 0x4e4b5741; /* "AWKN" little-endian */
AW_IMAGE_HEADER_VERSION = 1;
AW_IMAGE_HEADER_BYTES = 64;

SECTIONS
{
    . = AW_KERNEL_LOAD_BASE;

    __image_start = .;

    /*
     * Fixed 64-byte image header emitted by the linker, so every field is a
     * link-time constant instead of something the kernel or the loader has to
     * recompute. Offsets here are mirrored by `AwknImageHeader` in the UEFI
     * loader; changing one without the other is a boot-time magic/size failure,
     * not a silent misload.
     */
    .awhdr : ALIGN(0x1000)
    {
        LONG(AW_IMAGE_HEADER_MAGIC);        /* 0x00 magic      */
        LONG(AW_IMAGE_HEADER_VERSION);      /* 0x04 version    */
        QUAD(AW_KERNEL_LOAD_BASE);          /* 0x08 load_base  */
        QUAD(__image_end - __image_start);  /* 0x10 file bytes */
        QUAD(__memory_end - __image_start); /* 0x18 mem bytes  */
        QUAD(_start);                       /* 0x20 entry      */
        QUAD(__bss_start - __image_start);  /* 0x28 bss offset */
        QUAD(__bss_end - __bss_start);      /* 0x30 bss bytes  */
        QUAD(AW_IMAGE_HEADER_BYTES);        /* 0x38 hdr bytes  */
        . = ALIGN(0x1000);
    }

    /*
     * Section bounds are exported so the kernel can apply W^X permissions to
     * its own image at 4 KiB granularity once it owns the page tables. Every
     * boundary is page aligned, so no page ever mixes two permission classes.
     */
    .text : ALIGN(0x1000)
    {
        __text_start = .;
        KEEP(*(.text._start))
        *(.text .text.*)
        . = ALIGN(0x1000);
        __text_end = .;
    }

    .rodata : ALIGN(0x1000)
    {
        __rodata_start = .;
        *(.rodata .rodata.*)
        . = ALIGN(0x1000);
        __rodata_end = .;
    }

    .data : ALIGN(0x1000)
    {
        __data_start = .;
        *(.data .data.*)
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.plt)
        . = ALIGN(0x1000);
        __data_end = .;
    }

    . = ALIGN(0x1000);
    __image_end = .;

    .bss (NOLOAD) : ALIGN(0x1000)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(0x1000);
        __bss_end = .;
    }

    . = ALIGN(0x1000);
    __memory_end = .;

    /DISCARD/ :
    {
        *(.eh_frame .eh_frame.*)
        *(.comment)
        *(.note .note.*)
    }
}
