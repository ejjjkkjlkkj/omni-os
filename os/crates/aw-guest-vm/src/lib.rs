#![no_std]
#![forbid(unsafe_code)]

use aw_runtime_sources::{CompleteRuntimeSources, RuntimeFamily};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestFamily {
    Linux,
    Android,
}

impl GuestFamily {
    #[must_use]
    pub const fn runtime_family(self) -> RuntimeFamily {
        match self {
            Self::Linux => RuntimeFamily::Linux,
            Self::Android => RuntimeFamily::Android,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestSurface {
    Headless,
    Graphical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestIsolation {
    HardwareVirtualMachine,
    SharedHostKernel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestVmCapability {
    HardwareVirtualization,
    SeparateGuestMemory,
    VirtualCpu,
    InterruptController,
    VerifiedGuestImage,
    CrashContainment,
    VirtioBlock,
    VirtioNetwork,
    VirtioInput,
    EntropySource,
    MonotonicClock,
    HostIpcTransport,
    AccessibilitySemanticBridge,
    VirtioGraphics,
    WindowSurfaceBridge,
    ClipboardBridge,
    NotificationBridge,
    FilePortalBridge,
    UrlIntentBridge,
    AudioBridge,
    NetworkPolicyBridge,
}

impl GuestVmCapability {
    const fn bit(self) -> u64 {
        1u64 << self as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestVmProfile {
    family: GuestFamily,
    surface: GuestSurface,
    isolation: GuestIsolation,
    capabilities: u64,
}

impl GuestVmProfile {
    #[must_use]
    pub const fn new(
        family: GuestFamily,
        surface: GuestSurface,
        isolation: GuestIsolation,
    ) -> Self {
        Self {
            family,
            surface,
            isolation,
            capabilities: 0,
        }
    }

    pub fn mark_capability(&mut self, capability: GuestVmCapability) {
        self.capabilities |= capability.bit();
    }

    #[must_use]
    pub const fn has(self, capability: GuestVmCapability) -> bool {
        self.capabilities & capability.bit() != 0
    }

    #[must_use]
    pub const fn family(self) -> GuestFamily {
        self.family
    }

    #[must_use]
    pub const fn surface(self) -> GuestSurface {
        self.surface
    }

    #[must_use]
    pub const fn isolation(self) -> GuestIsolation {
        self.isolation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestVmAdmissionError {
    SharedHostKernelForbidden,
    MissingCapability(GuestVmCapability),
    SourceFamilyMismatch {
        expected: RuntimeFamily,
        actual: RuntimeFamily,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestVmReady {
    family: GuestFamily,
    surface: GuestSurface,
}

impl GuestVmReady {
    #[must_use]
    pub const fn family(self) -> GuestFamily {
        self.family
    }

    #[must_use]
    pub const fn surface(self) -> GuestSurface {
        self.surface
    }
}

const REQUIRED_BASE: [GuestVmCapability; 13] = [
    GuestVmCapability::HardwareVirtualization,
    GuestVmCapability::SeparateGuestMemory,
    GuestVmCapability::VirtualCpu,
    GuestVmCapability::InterruptController,
    GuestVmCapability::VerifiedGuestImage,
    GuestVmCapability::CrashContainment,
    GuestVmCapability::VirtioBlock,
    GuestVmCapability::VirtioNetwork,
    GuestVmCapability::VirtioInput,
    GuestVmCapability::EntropySource,
    GuestVmCapability::MonotonicClock,
    GuestVmCapability::HostIpcTransport,
    GuestVmCapability::AccessibilitySemanticBridge,
];

const REQUIRED_GRAPHICAL: [GuestVmCapability; 8] = [
    GuestVmCapability::VirtioGraphics,
    GuestVmCapability::WindowSurfaceBridge,
    GuestVmCapability::ClipboardBridge,
    GuestVmCapability::NotificationBridge,
    GuestVmCapability::FilePortalBridge,
    GuestVmCapability::UrlIntentBridge,
    GuestVmCapability::AudioBridge,
    GuestVmCapability::NetworkPolicyBridge,
];

#[must_use]
pub const fn base_required_capabilities() -> &'static [GuestVmCapability] {
    &REQUIRED_BASE
}

#[must_use]
pub const fn graphical_required_capabilities() -> &'static [GuestVmCapability] {
    &REQUIRED_GRAPHICAL
}

/// Admits a Linux or Android guest only when it is a hardware-isolated VM and all mandatory
/// devices/security channels exist. A shared-host-kernel container can never satisfy this gate.
pub fn admit_guest_vm(profile: GuestVmProfile) -> Result<GuestVmReady, GuestVmAdmissionError> {
    if profile.isolation != GuestIsolation::HardwareVirtualMachine {
        return Err(GuestVmAdmissionError::SharedHostKernelForbidden);
    }

    for capability in REQUIRED_BASE {
        if !profile.has(capability) {
            return Err(GuestVmAdmissionError::MissingCapability(capability));
        }
    }

    if profile.surface == GuestSurface::Graphical {
        for capability in REQUIRED_GRAPHICAL {
            if !profile.has(capability) {
                return Err(GuestVmAdmissionError::MissingCapability(capability));
            }
        }
    }

    Ok(GuestVmReady {
        family: profile.family,
        surface: profile.surface,
    })
}

/// Release admission additionally requires the complete, matching runtime source closure.
pub fn admit_release_guest_vm(
    profile: GuestVmProfile,
    sources: CompleteRuntimeSources,
) -> Result<GuestVmReady, GuestVmAdmissionError> {
    let expected = profile.family.runtime_family();
    let actual = sources.family();
    if expected != actual {
        return Err(GuestVmAdmissionError::SourceFamilyMismatch { expected, actual });
    }
    admit_guest_vm(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_runtime_sources::{
        RuntimeSourceSet, common_required_components, family_required_components,
        validate_complete_source_set,
    };

    fn complete_profile(
        family: GuestFamily,
        surface: GuestSurface,
        isolation: GuestIsolation,
    ) -> GuestVmProfile {
        let mut profile = GuestVmProfile::new(family, surface, isolation);
        for capability in base_required_capabilities() {
            profile.mark_capability(*capability);
        }
        if surface == GuestSurface::Graphical {
            for capability in graphical_required_capabilities() {
                profile.mark_capability(*capability);
            }
        }
        profile
    }

    fn source_proof(family: RuntimeFamily) -> CompleteRuntimeSources {
        let mut set = RuntimeSourceSet::new(family);
        for component in common_required_components() {
            set.mark(*component);
        }
        for component in family_required_components(family) {
            set.mark(*component);
        }
        validate_complete_source_set(set).unwrap()
    }

    #[test]
    fn linux_and_android_cannot_share_the_host_kernel() {
        for family in [GuestFamily::Linux, GuestFamily::Android] {
            let profile = complete_profile(
                family,
                GuestSurface::Headless,
                GuestIsolation::SharedHostKernel,
            );
            assert_eq!(
                admit_guest_vm(profile),
                Err(GuestVmAdmissionError::SharedHostKernelForbidden)
            );
        }
    }

    #[test]
    fn semantic_accessibility_channel_is_mandatory_even_headless() {
        let mut profile = complete_profile(
            GuestFamily::Linux,
            GuestSurface::Headless,
            GuestIsolation::HardwareVirtualMachine,
        );
        profile.capabilities &= !GuestVmCapability::AccessibilitySemanticBridge.bit();
        assert_eq!(
            admit_guest_vm(profile),
            Err(GuestVmAdmissionError::MissingCapability(
                GuestVmCapability::AccessibilitySemanticBridge
            ))
        );
    }

    #[test]
    fn graphical_guest_requires_host_window_surface() {
        let mut profile = complete_profile(
            GuestFamily::Android,
            GuestSurface::Graphical,
            GuestIsolation::HardwareVirtualMachine,
        );
        profile.capabilities &= !GuestVmCapability::WindowSurfaceBridge.bit();
        assert_eq!(
            admit_guest_vm(profile),
            Err(GuestVmAdmissionError::MissingCapability(
                GuestVmCapability::WindowSurfaceBridge
            ))
        );
    }

    #[test]
    fn graphical_guest_requires_brokered_network_policy() {
        let mut profile = complete_profile(
            GuestFamily::Linux,
            GuestSurface::Graphical,
            GuestIsolation::HardwareVirtualMachine,
        );
        profile.capabilities &= !GuestVmCapability::NetworkPolicyBridge.bit();
        assert_eq!(
            admit_guest_vm(profile),
            Err(GuestVmAdmissionError::MissingCapability(
                GuestVmCapability::NetworkPolicyBridge
            ))
        );
    }

    #[test]
    fn complete_linux_and_android_guests_are_admitted() {
        for family in [GuestFamily::Linux, GuestFamily::Android] {
            let profile = complete_profile(
                family,
                GuestSurface::Graphical,
                GuestIsolation::HardwareVirtualMachine,
            );
            let ready = admit_guest_vm(profile).unwrap();
            assert_eq!(ready.family(), family);
            assert_eq!(ready.surface(), GuestSurface::Graphical);
        }
    }

    #[test]
    fn release_guest_requires_matching_source_family() {
        let profile = complete_profile(
            GuestFamily::Android,
            GuestSurface::Graphical,
            GuestIsolation::HardwareVirtualMachine,
        );
        let linux_sources = source_proof(RuntimeFamily::Linux);
        assert_eq!(
            admit_release_guest_vm(profile, linux_sources),
            Err(GuestVmAdmissionError::SourceFamilyMismatch {
                expected: RuntimeFamily::Android,
                actual: RuntimeFamily::Linux,
            })
        );
    }

    #[test]
    fn release_guest_accepts_matching_complete_sources() {
        for family in [GuestFamily::Linux, GuestFamily::Android] {
            let profile = complete_profile(
                family,
                GuestSurface::Graphical,
                GuestIsolation::HardwareVirtualMachine,
            );
            let sources = source_proof(family.runtime_family());
            assert!(admit_release_guest_vm(profile, sources).is_ok());
        }
    }
}
