# Blind recovery hardware evidence contract

This contract defines the minimum evidence that `/usr/local/sbin/aw-dut-cycle-test` must produce before the physical hardware lab may claim an accessibility PASS.

A visible desktop, framebuffer console, native-kernel entry or successful USB boot is not accessibility evidence by itself.

## Required exact PASS records

The backend result file must contain each of these records exactly as shown:

```text
AW_PHYSICAL_BOOT_RESULT=PASS
AW_INTERNAL_DISK_UNCHANGED=PASS
AW_NATIVE_KERNEL_ENTRY=PASS
AW_KEYBOARD_RECOVERY=PASS
AW_STRUCTURED_DIAGNOSTICS=PASS
AW_ACCESSIBLE_RECOVERY_FLOW=PASS
AW_NONVISUAL_ROLLBACK_FLOW=PASS
AW_ROLLBACK_TARGET_IS_PREVIOUS_SUCCESSFUL=PASS
AW_DIAGNOSTIC_EXPORT=PASS
AW_SIGNED_REINSTALL_ACTION_AVAILABLE=PASS
AW_RECOVERY_INPUT=KEYBOARD
AW_VISUAL_ASSISTANCE_USED=NO
AW_POINTER_INPUT_USED=NO
AW_FAILURE_SEVERITY=CRITICAL
AW_FAILURE_ACTION=BOOT_PREVIOUS_GENERATION
```

The backend must also emit exactly one value for each typed field below.

## Direct nonvisual channel

```text
AW_NONVISUAL_CHANNEL=SPEECH
```

or

```text
AW_NONVISUAL_CHANNEL=BRAILLE
```

or

```text
AW_NONVISUAL_CHANNEL=SPEECH+BRAILLE
```

A framebuffer, visual console, remote desktop session, screenshot, camera operator or sighted assistant does not satisfy this field.

## Structured failure identity

The deliberately injected failure must be represented by one stable recovery diagnostic code from the recovery contract:

```text
AW_FAILURE_DIAGNOSTIC_CODE=0x1403
```

The hardware helper currently accepts the defined failure codes `0x1001`, `0x1002`, `0x1101`, `0x1102`, `0x1201`, `0x1401`, `0x1402`, `0x1403`, `0x1404` and `0x1501`. `0x1301` is the rollback-activated event and is not accepted as the injected failure identity.

The same failure identity and `BOOT_PREVIOUS_GENERATION` action must be available through structured diagnostics and the tested nonvisual channel. The backend must not substitute a private visual-only error message.

## Generation evidence

The backend must report the failed trial generation and the generation reached after rollback:

```text
AW_TRIAL_GENERATION=42
AW_ROLLBACK_GENERATION=41
```

Both values must be positive decimal integers and must differ. `AW_ROLLBACK_TARGET_IS_PREVIOUS_SUCCESSFUL=PASS` asserts that the rollback target was independently verified as the persisted previous successful generation rather than merely another bootable image.

## Test procedure requirements

The DUT backend must perform the acceptance flow without visual assistance:

1. boot the exact CI image identified by the supplied SHA-256;
2. enter a trial generation with a deliberately injected recovery-critical failure;
3. expose the failure through structured diagnostics and at least one direct nonvisual channel;
4. operate the recovery path with deterministic keyboard input only;
5. confirm that pointer input is not used for any recovery-critical action;
6. export diagnostics through the keyboard-accessible recovery path;
7. verify that the signed-reinstall action is discoverable and operable without sight;
8. select or automatically execute `BOOT_PREVIOUS_GENERATION`;
9. verify that the reached generation is the persisted previous successful generation;
10. verify that the internal disk state expected to remain untouched by the removable-media test is unchanged.

The helper rejects missing fields, duplicate typed fields, unknown diagnostic codes, visual assistance, pointer-dependent recovery, missing diagnostic export, missing signed-reinstall accessibility, an unproven previous-successful target, or a rollback result that reports the same generation as the failed trial.

## Scope of a PASS

A hardware-lab PASS proves only the recorded scenario on that exact DUT, image digest and commit. It does not by itself prove support across all x64 systems. Release-grade support still requires separate AMD and Intel physical systems and the broader blind-user walkthrough defined in `docs/ACCESSIBLE-RECOVERY.md`.
