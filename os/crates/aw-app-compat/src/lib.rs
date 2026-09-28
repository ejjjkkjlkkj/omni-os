#![no_std]
#![forbid(unsafe_code)]

use aw_guest_vm::GuestVmReady;
use aw_runtime_sources::{CompleteRuntimeSources, RuntimeFamily};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatibilityTarget {
    AndroidDex,
    AndroidNativeX86_64,
    AndroidNativeArm64,
    LinuxElfX86_64,
    LinuxElfArm64,
    DarwinMachOX86_64,
    DarwinMachOArm64,
}

impl CompatibilityTarget {
    #[must_use]
    pub const fn runtime_family(self) -> RuntimeFamily {
        match self {
            Self::AndroidDex | Self::AndroidNativeX86_64 | Self::AndroidNativeArm64 => {
                RuntimeFamily::Android
            }
            Self::LinuxElfX86_64 | Self::LinuxElfArm64 => RuntimeFamily::Linux,
            Self::DarwinMachOX86_64 | Self::DarwinMachOArm64 => RuntimeFamily::Darwin,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeBoundary {
    HardwareIsolatedVm,
    UserModeCompatibility,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppSurface {
    CommandLine,
    Graphical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeCapability {
    PackageIdentity,
    SyscallIsolation,
    IpcBroker,
    FilesystemBroker,
    InputBridge,
    AccessibilityBridge,
    GraphicsBridge,
    AudioBridge,
    NetworkBroker,
    ForeignIsaTranslation,
    WindowIntegration,
    LauncherRegistration,
    ClipboardBridge,
    NotificationBridge,
    FileOpenPortal,
    UrlIntentBridge,
}

impl RuntimeCapability {
    const fn bit(self) -> u32 {
        match self {
            Self::PackageIdentity => 1 << 0,
            Self::SyscallIsolation => 1 << 1,
            Self::IpcBroker => 1 << 2,
            Self::FilesystemBroker => 1 << 3,
            Self::InputBridge => 1 << 4,
            Self::AccessibilityBridge => 1 << 5,
            Self::GraphicsBridge => 1 << 6,
            Self::AudioBridge => 1 << 7,
            Self::NetworkBroker => 1 << 8,
            Self::ForeignIsaTranslation => 1 << 9,
            Self::WindowIntegration => 1 << 10,
            Self::LauncherRegistration => 1 << 11,
            Self::ClipboardBridge => 1 << 12,
            Self::NotificationBridge => 1 << 13,
            Self::FileOpenPortal => 1 << 14,
            Self::UrlIntentBridge => 1 << 15,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeProfile {
    target: CompatibilityTarget,
    boundary: RuntimeBoundary,
    surface: AppSurface,
    capabilities: u32,
}

impl RuntimeProfile {
    #[must_use]
    pub const fn new(
        target: CompatibilityTarget,
        boundary: RuntimeBoundary,
        surface: AppSurface,
    ) -> Self {
        Self {
            target,
            boundary,
            surface,
            capabilities: 0,
        }
    }

    pub fn mark_capability(&mut self, capability: RuntimeCapability) {
        self.capabilities |= capability.bit();
    }

    #[must_use]
    pub const fn has(self, capability: RuntimeCapability) -> bool {
        self.capabilities & capability.bit() != 0
    }

    #[must_use]
    pub const fn target(self) -> CompatibilityTarget {
        self.target
    }

    #[must_use]
    pub const fn boundary(self) -> RuntimeBoundary {
        self.boundary
    }

    #[must_use]
    pub const fn surface(self) -> AppSurface {
        self.surface
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeAdmissionError {
    AndroidRequiresHardwareVm,
    LinuxRequiresHardwareVm,
    DarwinRequiresUserModeCompatibility,
    MissingCapability(RuntimeCapability),
    ForeignIsaTranslationUnavailable,
    SourceFamilyMismatch {
        expected: RuntimeFamily,
        actual: RuntimeFamily,
    },
    GuestVmFamilyMismatch {
        expected: RuntimeFamily,
        actual: RuntimeFamily,
    },
    GuestVmNotApplicableToDarwin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeReady {
    target: CompatibilityTarget,
    surface: AppSurface,
}

impl RuntimeReady {
    #[must_use]
    pub const fn target(self) -> CompatibilityTarget {
        self.target
    }

    #[must_use]
    pub const fn surface(self) -> AppSurface {
        self.surface
    }
}

const REQUIRED_BASE: [RuntimeCapability; 6] = [
    RuntimeCapability::PackageIdentity,
    RuntimeCapability::SyscallIsolation,
    RuntimeCapability::IpcBroker,
    RuntimeCapability::FilesystemBroker,
    RuntimeCapability::InputBridge,
    RuntimeCapability::AccessibilityBridge,
];

const REQUIRED_SYSTEM_INTEGRATION: [RuntimeCapability; 7] = [
    RuntimeCapability::LauncherRegistration,
    RuntimeCapability::ClipboardBridge,
    RuntimeCapability::NotificationBridge,
    RuntimeCapability::FileOpenPortal,
    RuntimeCapability::UrlIntentBridge,
    RuntimeCapability::AudioBridge,
    RuntimeCapability::NetworkBroker,
];

pub fn admit_runtime(profile: RuntimeProfile) -> Result<RuntimeReady, RuntimeAdmissionError> {
    match profile.target {
        CompatibilityTarget::AndroidDex
        | CompatibilityTarget::AndroidNativeX86_64
        | CompatibilityTarget::AndroidNativeArm64 => {
            if profile.boundary != RuntimeBoundary::HardwareIsolatedVm {
                return Err(RuntimeAdmissionError::AndroidRequiresHardwareVm);
            }
        }
        CompatibilityTarget::LinuxElfX86_64 | CompatibilityTarget::LinuxElfArm64 => {
            if profile.boundary != RuntimeBoundary::HardwareIsolatedVm {
                return Err(RuntimeAdmissionError::LinuxRequiresHardwareVm);
            }
        }
        CompatibilityTarget::DarwinMachOX86_64 | CompatibilityTarget::DarwinMachOArm64 => {
            if profile.boundary != RuntimeBoundary::UserModeCompatibility {
                return Err(RuntimeAdmissionError::DarwinRequiresUserModeCompatibility);
            }
        }
    }

    for capability in REQUIRED_BASE {
        if !profile.has(capability) {
            return Err(RuntimeAdmissionError::MissingCapability(capability));
        }
    }

    if profile.surface == AppSurface::Graphical && !profile.has(RuntimeCapability::GraphicsBridge) {
        return Err(RuntimeAdmissionError::MissingCapability(
            RuntimeCapability::GraphicsBridge,
        ));
    }

    if matches!(
        profile.target,
        CompatibilityTarget::AndroidNativeArm64
            | CompatibilityTarget::LinuxElfArm64
            | CompatibilityTarget::DarwinMachOArm64
    ) && !profile.has(RuntimeCapability::ForeignIsaTranslation)
    {
        return Err(RuntimeAdmissionError::ForeignIsaTranslationUnavailable);
    }

    Ok(RuntimeReady {
        target: profile.target,
        surface: profile.surface,
    })
}

pub fn admit_system_integrated_app(
    profile: RuntimeProfile,
) -> Result<RuntimeReady, RuntimeAdmissionError> {
    let ready = admit_runtime(profile)?;

    for capability in REQUIRED_SYSTEM_INTEGRATION {
        if !profile.has(capability) {
            return Err(RuntimeAdmissionError::MissingCapability(capability));
        }
    }

    if profile.surface == AppSurface::Graphical
        && !profile.has(RuntimeCapability::WindowIntegration)
    {
        return Err(RuntimeAdmissionError::MissingCapability(
            RuntimeCapability::WindowIntegration,
        ));
    }

    Ok(ready)
}

/// Release-grade admission requires the full immutable source closure from the same runtime family.
pub fn admit_release_integrated_app(
    profile: RuntimeProfile,
    sources: CompleteRuntimeSources,
) -> Result<RuntimeReady, RuntimeAdmissionError> {
    let expected = profile.target.runtime_family();
    let actual = sources.family();
    if expected != actual {
        return Err(RuntimeAdmissionError::SourceFamilyMismatch { expected, actual });
    }
    admit_system_integrated_app(profile)
}

/// Linux and Android release applications additionally require a successfully admitted hardware
/// guest VM from the same family. Darwin deliberately cannot use this path because macOS is not
/// bundled or virtualized by the compatibility architecture.
pub fn admit_release_vm_backed_integrated_app(
    profile: RuntimeProfile,
    sources: CompleteRuntimeSources,
    guest_vm: GuestVmReady,
) -> Result<RuntimeReady, RuntimeAdmissionError> {
    let expected = profile.target.runtime_family();
    if expected == RuntimeFamily::Darwin {
        return Err(RuntimeAdmissionError::GuestVmNotApplicableToDarwin);
    }

    let actual = guest_vm.family().runtime_family();
    if expected != actual {
        return Err(RuntimeAdmissionError::GuestVmFamilyMismatch { expected, actual });
    }

    admit_release_integrated_app(profile, sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_guest_vm::{
        GuestFamily, GuestIsolation, GuestSurface, GuestVmProfile, admit_guest_vm,
        base_required_capabilities, graphical_required_capabilities,
    };
    use aw_runtime_sources::{
        RuntimeSourceSet, common_required_components, family_required_components,
        validate_complete_source_set,
    };

    fn base_profile(
        target: CompatibilityTarget,
        boundary: RuntimeBoundary,
        surface: AppSurface,
    ) -> RuntimeProfile {
        let mut profile = RuntimeProfile::new(target, boundary, surface);
        for capability in REQUIRED_BASE {
            profile.mark_capability(capability);
        }
        profile
    }

    fn fully_integrated_profile(
        target: CompatibilityTarget,
        boundary: RuntimeBoundary,
        surface: AppSurface,
    ) -> RuntimeProfile {
        let mut profile = base_profile(target, boundary, surface);
        for capability in REQUIRED_SYSTEM_INTEGRATION {
            profile.mark_capability(capability);
        }
        if surface == AppSurface::Graphical {
            profile.mark_capability(RuntimeCapability::GraphicsBridge);
            profile.mark_capability(RuntimeCapability::WindowIntegration);
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

    fn guest_proof(family: GuestFamily) -> GuestVmReady {
        let mut profile = GuestVmProfile::new(
            family,
            GuestSurface::Graphical,
            GuestIsolation::HardwareVirtualMachine,
        );
        for capability in base_required_capabilities() {
            profile.mark_capability(*capability);
        }
        for capability in graphical_required_capabilities() {
            profile.mark_capability(*capability);
        }
        admit_guest_vm(profile).unwrap()
    }

    #[test]
    fn android_is_not_admitted_as_shared_host_userspace() {
        let profile = base_profile(
            CompatibilityTarget::AndroidDex,
            RuntimeBoundary::UserModeCompatibility,
            AppSurface::CommandLine,
        );
        assert_eq!(
            admit_runtime(profile),
            Err(RuntimeAdmissionError::AndroidRequiresHardwareVm)
        );
    }

    #[test]
    fn linux_is_not_admitted_as_shared_host_userspace() {
        let profile = base_profile(
            CompatibilityTarget::LinuxElfX86_64,
            RuntimeBoundary::UserModeCompatibility,
            AppSurface::CommandLine,
        );
        assert_eq!(
            admit_runtime(profile),
            Err(RuntimeAdmissionError::LinuxRequiresHardwareVm)
        );
    }

    #[test]
    fn android_and_linux_x86_64_can_run_locally_in_isolated_vms() {
        for target in [
            CompatibilityTarget::AndroidNativeX86_64,
            CompatibilityTarget::LinuxElfX86_64,
        ] {
            let profile = base_profile(
                target,
                RuntimeBoundary::HardwareIsolatedVm,
                AppSurface::CommandLine,
            );
            assert!(admit_runtime(profile).is_ok());
        }
    }

    #[test]
    fn foreign_arm_binaries_require_explicit_translation() {
        for target in [
            CompatibilityTarget::AndroidNativeArm64,
            CompatibilityTarget::LinuxElfArm64,
            CompatibilityTarget::DarwinMachOArm64,
        ] {
            let boundary = if target == CompatibilityTarget::DarwinMachOArm64 {
                RuntimeBoundary::UserModeCompatibility
            } else {
                RuntimeBoundary::HardwareIsolatedVm
            };
            let profile = base_profile(target, boundary, AppSurface::CommandLine);
            assert_eq!(
                admit_runtime(profile),
                Err(RuntimeAdmissionError::ForeignIsaTranslationUnavailable)
            );
        }
    }

    #[test]
    fn darwin_is_userspace_compatibility_not_macos_vm() {
        let profile = base_profile(
            CompatibilityTarget::DarwinMachOX86_64,
            RuntimeBoundary::HardwareIsolatedVm,
            AppSurface::CommandLine,
        );
        assert_eq!(
            admit_runtime(profile),
            Err(RuntimeAdmissionError::DarwinRequiresUserModeCompatibility)
        );
    }

    #[test]
    fn executable_app_is_not_automatically_system_integrated() {
        let profile = base_profile(
            CompatibilityTarget::AndroidDex,
            RuntimeBoundary::HardwareIsolatedVm,
            AppSurface::CommandLine,
        );
        assert_eq!(
            admit_system_integrated_app(profile),
            Err(RuntimeAdmissionError::MissingCapability(
                RuntimeCapability::LauncherRegistration
            ))
        );
    }

    #[test]
    fn graphical_integrated_app_requires_host_window_integration() {
        let mut profile = fully_integrated_profile(
            CompatibilityTarget::LinuxElfX86_64,
            RuntimeBoundary::HardwareIsolatedVm,
            AppSurface::Graphical,
        );
        profile.capabilities &= !RuntimeCapability::WindowIntegration.bit();
        assert_eq!(
            admit_system_integrated_app(profile),
            Err(RuntimeAdmissionError::MissingCapability(
                RuntimeCapability::WindowIntegration
            ))
        );
    }

    #[test]
    fn every_family_can_pass_release_source_admission() {
        for (target, boundary) in [
            (
                CompatibilityTarget::AndroidDex,
                RuntimeBoundary::HardwareIsolatedVm,
            ),
            (
                CompatibilityTarget::LinuxElfX86_64,
                RuntimeBoundary::HardwareIsolatedVm,
            ),
            (
                CompatibilityTarget::DarwinMachOX86_64,
                RuntimeBoundary::UserModeCompatibility,
            ),
        ] {
            let profile = fully_integrated_profile(target, boundary, AppSurface::Graphical);
            let sources = source_proof(target.runtime_family());
            assert!(admit_release_integrated_app(profile, sources).is_ok());
        }
    }

    #[test]
    fn release_admission_rejects_source_proof_from_another_family() {
        let profile = fully_integrated_profile(
            CompatibilityTarget::AndroidDex,
            RuntimeBoundary::HardwareIsolatedVm,
            AppSurface::Graphical,
        );
        let sources = source_proof(RuntimeFamily::Linux);
        assert_eq!(
            admit_release_integrated_app(profile, sources),
            Err(RuntimeAdmissionError::SourceFamilyMismatch {
                expected: RuntimeFamily::Android,
                actual: RuntimeFamily::Linux,
            })
        );
    }

    #[test]
    fn vm_backed_release_requires_matching_guest_family() {
        let profile = fully_integrated_profile(
            CompatibilityTarget::AndroidDex,
            RuntimeBoundary::HardwareIsolatedVm,
            AppSurface::Graphical,
        );
        let sources = source_proof(RuntimeFamily::Android);
        let linux_guest = guest_proof(GuestFamily::Linux);
        assert_eq!(
            admit_release_vm_backed_integrated_app(profile, sources, linux_guest),
            Err(RuntimeAdmissionError::GuestVmFamilyMismatch {
                expected: RuntimeFamily::Android,
                actual: RuntimeFamily::Linux,
            })
        );
    }

    #[test]
    fn linux_and_android_release_require_matching_guest_vm_proof() {
        for (target, guest_family) in [
            (CompatibilityTarget::AndroidDex, GuestFamily::Android),
            (CompatibilityTarget::LinuxElfX86_64, GuestFamily::Linux),
        ] {
            let profile = fully_integrated_profile(
                target,
                RuntimeBoundary::HardwareIsolatedVm,
                AppSurface::Graphical,
            );
            let sources = source_proof(target.runtime_family());
            let guest = guest_proof(guest_family);
            assert!(admit_release_vm_backed_integrated_app(profile, sources, guest).is_ok());
        }
    }

    #[test]
    fn darwin_cannot_be_admitted_through_guest_vm_release_path() {
        let profile = fully_integrated_profile(
            CompatibilityTarget::DarwinMachOX86_64,
            RuntimeBoundary::UserModeCompatibility,
            AppSurface::Graphical,
        );
        let sources = source_proof(RuntimeFamily::Darwin);
        let linux_guest = guest_proof(GuestFamily::Linux);
        assert_eq!(
            admit_release_vm_backed_integrated_app(profile, sources, linux_guest),
            Err(RuntimeAdmissionError::GuestVmNotApplicableToDarwin)
        );
    }

    #[test]
    fn accessibility_bridge_is_mandatory_for_every_runtime() {
        let mut profile = RuntimeProfile::new(
            CompatibilityTarget::AndroidDex,
            RuntimeBoundary::HardwareIsolatedVm,
            AppSurface::CommandLine,
        );
        for capability in [
            RuntimeCapability::PackageIdentity,
            RuntimeCapability::SyscallIsolation,
            RuntimeCapability::IpcBroker,
            RuntimeCapability::FilesystemBroker,
            RuntimeCapability::InputBridge,
        ] {
            profile.mark_capability(capability);
        }
        assert_eq!(
            admit_runtime(profile),
            Err(RuntimeAdmissionError::MissingCapability(
                RuntimeCapability::AccessibilityBridge
            ))
        );
    }
}
