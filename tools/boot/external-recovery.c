/* Stand-in external recovery medium for the Recovery Core proof: a minimal UEFI application
 * that reports it ran on QEMU's debug console (port 0xE9, where the loader's markers go) and
 * returns to its caller, as a real recovery tool does when the user leaves it.
 *   clang --target=x86_64-pc-windows-msvc -ffreestanding -nostdlib -c external-recovery.c
 *   lld-link /subsystem:efi_application /entry:efi_main /nodefaultlib external-recovery.obj */

static void outb(unsigned short port, unsigned char value) {
    __asm__ volatile("outb %0, %1" : : "a"(value), "Nd"(port));
}

unsigned long long efi_main(void *image, void *system_table) {
    (void)image;
    (void)system_table;
    for (const char *m = "AW_EXTERNAL_RECOVERY_RAN\n"; *m; m++) {
        outb(0xe9, (unsigned char)*m);
    }
    return 0; /* EFI_SUCCESS: back to the omni-os recovery menu */
}
