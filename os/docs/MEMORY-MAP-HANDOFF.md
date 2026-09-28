# Kernel memory-map handoff

The native kernel must receive the final UEFI memory map returned by the same `ExitBootServices` operation that successfully ended firmware boot services.

The handoff carries:

- address of the final memory-map backing buffer;
- exact byte length;
- descriptor count;
- firmware-reported descriptor size;
- descriptor version.

The kernel must never assume `size_of::<UEFI_MEMORY_DESCRIPTOR>()` is the descriptor stride. Firmware-reported descriptor size is authoritative.

For the current x86-64 bootstrap, the kernel keeps the firmware page tables active and validates the final map in place before any physical allocator is enabled. A later milestone will normalize the UEFI map into Accessible Windows-owned physical-memory regions and reserve the kernel, framebuffer, ACPI and boot-handoff ranges explicitly.
