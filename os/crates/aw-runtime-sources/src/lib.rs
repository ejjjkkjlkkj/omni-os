#![no_std]
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeFamily {
    Linux,
    Android,
    Darwin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceComponent {
    LinuxKernel,
    LinuxLibc,
    LinuxInitServices,
    LinuxDbus,
    LinuxWayland,
    LinuxXwayland,
    LinuxMesa,
    LinuxPipewire,
    LinuxDesktopPortal,
    LinuxAccessibilityAtSpi,
    AndroidBuildSystem,
    AndroidKernelContract,
    AndroidBionic,
    AndroidArt,
    AndroidBinder,
    AndroidFrameworkBase,
    AndroidFrameworkNative,
    AndroidGraphics,
    AndroidMediaAudio,
    AndroidPackageActivityServices,
    AndroidPermissionSecurity,
    AndroidAccessibility,
    DarwinMachAbi,
    DarwinMachOLoader,
    DarwinDynamicLoader,
    DarwinLibSystem,
    DarwinLibc,
    DarwinObjectiveCRuntime,
    DarwinDispatch,
    DarwinCoreFoundationCompat,
    DarwinFoundationCompat,
    DarwinAppKitCompat,
    DarwinGraphicsCompat,
    DarwinAudioCompat,
    DarwinSecurityCompat,
    DarwinAccessibilityCompat,
    ImmutableSourcePins,
    LicenseInventory,
    SourceProvenance,
    ReproducibleBuildRecipe,
}

impl SourceComponent {
    const fn bit(self) -> u64 {
        1u64 << self as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeSourceSet {
    family: RuntimeFamily,
    components: u64,
}

impl RuntimeSourceSet {
    #[must_use]
    pub const fn new(family: RuntimeFamily) -> Self {
        Self {
            family,
            components: 0,
        }
    }

    pub fn mark(&mut self, component: SourceComponent) {
        self.components |= component.bit();
    }

    #[must_use]
    pub const fn has(self, component: SourceComponent) -> bool {
        self.components & component.bit() != 0
    }

    #[must_use]
    pub const fn family(self) -> RuntimeFamily {
        self.family
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceCompletenessError {
    Missing(SourceComponent),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompleteRuntimeSources {
    family: RuntimeFamily,
}

impl CompleteRuntimeSources {
    #[must_use]
    pub const fn family(self) -> RuntimeFamily {
        self.family
    }
}

const COMMON_REQUIRED: [SourceComponent; 4] = [
    SourceComponent::ImmutableSourcePins,
    SourceComponent::LicenseInventory,
    SourceComponent::SourceProvenance,
    SourceComponent::ReproducibleBuildRecipe,
];

const LINUX_REQUIRED: [SourceComponent; 10] = [
    SourceComponent::LinuxKernel,
    SourceComponent::LinuxLibc,
    SourceComponent::LinuxInitServices,
    SourceComponent::LinuxDbus,
    SourceComponent::LinuxWayland,
    SourceComponent::LinuxXwayland,
    SourceComponent::LinuxMesa,
    SourceComponent::LinuxPipewire,
    SourceComponent::LinuxDesktopPortal,
    SourceComponent::LinuxAccessibilityAtSpi,
];

const ANDROID_REQUIRED: [SourceComponent; 12] = [
    SourceComponent::AndroidBuildSystem,
    SourceComponent::AndroidKernelContract,
    SourceComponent::AndroidBionic,
    SourceComponent::AndroidArt,
    SourceComponent::AndroidBinder,
    SourceComponent::AndroidFrameworkBase,
    SourceComponent::AndroidFrameworkNative,
    SourceComponent::AndroidGraphics,
    SourceComponent::AndroidMediaAudio,
    SourceComponent::AndroidPackageActivityServices,
    SourceComponent::AndroidPermissionSecurity,
    SourceComponent::AndroidAccessibility,
];

const DARWIN_REQUIRED: [SourceComponent; 14] = [
    SourceComponent::DarwinMachAbi,
    SourceComponent::DarwinMachOLoader,
    SourceComponent::DarwinDynamicLoader,
    SourceComponent::DarwinLibSystem,
    SourceComponent::DarwinLibc,
    SourceComponent::DarwinObjectiveCRuntime,
    SourceComponent::DarwinDispatch,
    SourceComponent::DarwinCoreFoundationCompat,
    SourceComponent::DarwinFoundationCompat,
    SourceComponent::DarwinAppKitCompat,
    SourceComponent::DarwinGraphicsCompat,
    SourceComponent::DarwinAudioCompat,
    SourceComponent::DarwinSecurityCompat,
    SourceComponent::DarwinAccessibilityCompat,
];

#[must_use]
pub const fn common_required_components() -> &'static [SourceComponent] {
    &COMMON_REQUIRED
}

#[must_use]
pub const fn family_required_components(family: RuntimeFamily) -> &'static [SourceComponent] {
    match family {
        RuntimeFamily::Linux => &LINUX_REQUIRED,
        RuntimeFamily::Android => &ANDROID_REQUIRED,
        RuntimeFamily::Darwin => &DARWIN_REQUIRED,
    }
}

fn require(
    set: RuntimeSourceSet,
    required: &[SourceComponent],
) -> Result<(), SourceCompletenessError> {
    for component in required {
        if !set.has(*component) {
            return Err(SourceCompletenessError::Missing(*component));
        }
    }
    Ok(())
}

/// Proves that a runtime family has every source class needed by the host integration contract.
///
/// This gate deliberately models logical source coverage rather than accepting a single giant
/// upstream checkout as proof of completeness. Every family must also have immutable pins,
/// provenance, a license inventory and a reproducible build recipe. Proprietary macOS components
/// cannot satisfy Darwin entries: unavailable APIs must be implemented by clean-room compatible
/// components before Darwin can pass this gate.
pub fn validate_complete_source_set(
    set: RuntimeSourceSet,
) -> Result<CompleteRuntimeSources, SourceCompletenessError> {
    require(set, common_required_components())?;
    require(set, family_required_components(set.family))?;
    Ok(CompleteRuntimeSources { family: set.family })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark_all(set: &mut RuntimeSourceSet, components: &[SourceComponent]) {
        for component in components {
            set.mark(*component);
        }
    }

    fn complete_set(family: RuntimeFamily) -> RuntimeSourceSet {
        let mut set = RuntimeSourceSet::new(family);
        mark_all(&mut set, common_required_components());
        mark_all(&mut set, family_required_components(family));
        set
    }

    #[test]
    fn every_family_can_prove_complete_source_coverage() {
        for family in [
            RuntimeFamily::Linux,
            RuntimeFamily::Android,
            RuntimeFamily::Darwin,
        ] {
            let proof = validate_complete_source_set(complete_set(family)).unwrap();
            assert_eq!(proof.family(), family);
        }
    }

    #[test]
    fn immutable_source_pin_is_mandatory() {
        let mut set = complete_set(RuntimeFamily::Linux);
        set.components &= !SourceComponent::ImmutableSourcePins.bit();
        assert_eq!(
            validate_complete_source_set(set),
            Err(SourceCompletenessError::Missing(
                SourceComponent::ImmutableSourcePins
            ))
        );
    }

    #[test]
    fn android_without_accessibility_is_not_complete() {
        let mut set = complete_set(RuntimeFamily::Android);
        set.components &= !SourceComponent::AndroidAccessibility.bit();
        assert_eq!(
            validate_complete_source_set(set),
            Err(SourceCompletenessError::Missing(
                SourceComponent::AndroidAccessibility
            ))
        );
    }

    #[test]
    fn linux_without_portal_is_not_complete() {
        let mut set = complete_set(RuntimeFamily::Linux);
        set.components &= !SourceComponent::LinuxDesktopPortal.bit();
        assert_eq!(
            validate_complete_source_set(set),
            Err(SourceCompletenessError::Missing(
                SourceComponent::LinuxDesktopPortal
            ))
        );
    }

    #[test]
    fn darwin_without_clean_room_appkit_compat_is_not_complete() {
        let mut set = complete_set(RuntimeFamily::Darwin);
        set.components &= !SourceComponent::DarwinAppKitCompat.bit();
        assert_eq!(
            validate_complete_source_set(set),
            Err(SourceCompletenessError::Missing(
                SourceComponent::DarwinAppKitCompat
            ))
        );
    }

    #[test]
    fn license_inventory_is_never_optional() {
        let mut set = complete_set(RuntimeFamily::Darwin);
        set.components &= !SourceComponent::LicenseInventory.bit();
        assert_eq!(
            validate_complete_source_set(set),
            Err(SourceCompletenessError::Missing(
                SourceComponent::LicenseInventory
            ))
        );
    }
}
