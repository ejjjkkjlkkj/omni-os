#include <Uefi.h>
#include <Guid/GlobalVariable.h>
#include <Guid/MemoryAttributesTable.h>
#include <Protocol/LoadedImage.h>
#include <Protocol/SimpleFileSystem.h>
#include <Protocol/SimpleNetwork.h>
#include <Protocol/ManagedNetwork.h>
#include <Protocol/Ip4.h>
#include <Protocol/Ip6.h>
#include <Protocol/Dhcp4.h>
#include <Protocol/Dhcp6.h>
#include <Protocol/Dns4.h>
#include <Protocol/Dns6.h>
#include <Protocol/Tcp4.h>
#include <Protocol/Tcp6.h>
#include <Protocol/Udp4.h>
#include <Protocol/Udp6.h>
#include <Protocol/Http.h>
#include <Protocol/Tls.h>
#include <Protocol/IpSec.h>
#include <Protocol/Rest.h>
#include <Protocol/Rng.h>
#include <Protocol/Tcg2Protocol.h>
#include <Protocol/FirmwareManagement.h>

#define OMNI_GUARDIAN_EVIDENCE_FILE L"\\\\OMNI-GUARDIAN.TXT"
#define OMNI_GUARDIAN_SCHEMA "omni.guardian.capabilities.v1"

typedef struct {
  CONST CHAR8 *Name;
  EFI_GUID    *Guid;
} OMNI_PROTOCOL_PROBE;

STATIC UINTN
AsciiLength (
  IN CONST CHAR8 *Text
  )
{
  UINTN Size;

  if (Text == NULL) {
    return 0;
  }

  Size = 0;
  while (Text[Size] != '\0') {
    ++Size;
  }

  return Size;
}

STATIC EFI_STATUS
WriteAscii (
  IN EFI_FILE_PROTOCOL *File,
  IN CONST CHAR8       *Text
  )
{
  UINTN Size;

  if ((File == NULL) || (Text == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Size = AsciiLength (Text);
  if (Size == 0) {
    return EFI_SUCCESS;
  }

  return File->Write (File, &Size, (VOID *)Text);
}

STATIC EFI_STATUS
WriteUint (
  IN EFI_FILE_PROTOCOL *File,
  IN UINTN             Value
  )
{
  CHAR8 Reverse[32];
  CHAR8 Forward[32];
  UINTN Count;
  UINTN Index;
  UINTN Size;

  if (File == NULL) {
    return EFI_INVALID_PARAMETER;
  }

  if (Value == 0) {
    Forward[0] = '0';
    Size = 1;
    return File->Write (File, &Size, Forward);
  }

  Count = 0;
  while ((Value != 0) && (Count < sizeof (Reverse))) {
    Reverse[Count++] = (CHAR8)('0' + (Value % 10));
    Value /= 10;
  }

  for (Index = 0; Index < Count; ++Index) {
    Forward[Index] = Reverse[Count - Index - 1];
  }

  Size = Count;
  return File->Write (File, &Size, Forward);
}

STATIC EFI_STATUS
WriteStat (
  IN EFI_FILE_PROTOCOL *File,
  IN CONST CHAR8       *Name,
  IN UINTN             Value
  )
{
  EFI_STATUS Status;

  Status = WriteAscii (File, Name);
  if (EFI_ERROR (Status)) {
    return Status;
  }

  Status = WriteAscii (File, "=");
  if (EFI_ERROR (Status)) {
    return Status;
  }

  Status = WriteUint (File, Value);
  if (EFI_ERROR (Status)) {
    return Status;
  }

  return WriteAscii (File, "\n");
}

STATIC BOOLEAN
GuidEqual (
  IN CONST EFI_GUID *A,
  IN CONST EFI_GUID *B
  )
{
  CONST UINT8 *Left;
  CONST UINT8 *Right;
  UINTN Index;

  if ((A == NULL) || (B == NULL)) {
    return FALSE;
  }

  Left = (CONST UINT8 *)A;
  Right = (CONST UINT8 *)B;
  for (Index = 0; Index < sizeof (EFI_GUID); ++Index) {
    if (Left[Index] != Right[Index]) {
      return FALSE;
    }
  }

  return TRUE;
}

STATIC BOOLEAN
HasConfigurationTable (
  IN EFI_SYSTEM_TABLE *SystemTable,
  IN EFI_GUID         *Guid
  )
{
  UINTN Index;

  if ((SystemTable == NULL) || (Guid == NULL)) {
    return FALSE;
  }

  for (Index = 0; Index < SystemTable->NumberOfTableEntries; ++Index) {
    if (GuidEqual (&SystemTable->ConfigurationTable[Index].VendorGuid, Guid)) {
      return TRUE;
    }
  }

  return FALSE;
}

STATIC UINTN
CountProtocolHandles (
  IN EFI_SYSTEM_TABLE *SystemTable,
  IN EFI_GUID         *Guid
  )
{
  EFI_STATUS Status;
  EFI_HANDLE *Handles;
  UINTN Count;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) || (Guid == NULL)) {
    return 0;
  }

  Handles = NULL;
  Count = 0;
  Status = SystemTable->BootServices->LocateHandleBuffer (
                                       ByProtocol,
                                       Guid,
                                       NULL,
                                       &Count,
                                       &Handles
                                       );
  if (EFI_ERROR (Status)) {
    return 0;
  }

  if (Handles != NULL) {
    SystemTable->BootServices->FreePool (Handles);
  }

  return Count;
}

STATIC EFI_STATUS
ReadGlobalByte (
  IN EFI_SYSTEM_TABLE *SystemTable,
  IN CHAR16           *Name,
  OUT UINT8           *Value,
  OUT BOOLEAN         *Present
  )
{
  EFI_STATUS Status;
  UINTN Size;
  UINT32 Attributes;

  if ((SystemTable == NULL) ||
      (SystemTable->RuntimeServices == NULL) ||
      (Name == NULL) ||
      (Value == NULL) ||
      (Present == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  *Value = 0;
  *Present = FALSE;
  Size = sizeof (*Value);
  Attributes = 0;

  Status = SystemTable->RuntimeServices->GetVariable (
                                          Name,
                                          &gEfiGlobalVariableGuid,
                                          &Attributes,
                                          &Size,
                                          Value
                                          );
  if (!EFI_ERROR (Status) && (Size == sizeof (*Value))) {
    *Present = TRUE;
  }

  return Status;
}

STATIC EFI_STATUS
OpenEvidenceFile (
  IN EFI_HANDLE         ImageHandle,
  IN EFI_SYSTEM_TABLE   *SystemTable,
  OUT EFI_FILE_PROTOCOL **Evidence,
  OUT EFI_FILE_PROTOCOL **Root
  )
{
  EFI_STATUS Status;
  EFI_LOADED_IMAGE_PROTOCOL *LoadedImage;
  EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *FileSystem;
  EFI_FILE_PROTOCOL *Existing;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) ||
      (Evidence == NULL) || (Root == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  *Evidence = NULL;
  *Root = NULL;
  LoadedImage = NULL;
  FileSystem = NULL;
  Existing = NULL;

  Status = SystemTable->BootServices->HandleProtocol (
                                      ImageHandle,
                                      &gEfiLoadedImageProtocolGuid,
                                      (VOID **)&LoadedImage
                                      );
  if (EFI_ERROR (Status) || (LoadedImage == NULL)) {
    return EFI_NOT_FOUND;
  }

  Status = SystemTable->BootServices->HandleProtocol (
                                      LoadedImage->DeviceHandle,
                                      &gEfiSimpleFileSystemProtocolGuid,
                                      (VOID **)&FileSystem
                                      );
  if (EFI_ERROR (Status) || (FileSystem == NULL)) {
    return EFI_NOT_FOUND;
  }

  Status = FileSystem->OpenVolume (FileSystem, Root);
  if (EFI_ERROR (Status) || (*Root == NULL)) {
    return Status;
  }

  Status = (*Root)->Open (
                     *Root,
                     &Existing,
                     OMNI_GUARDIAN_EVIDENCE_FILE,
                     EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE,
                     0
                     );
  if (!EFI_ERROR (Status) && (Existing != NULL)) {
    Status = Existing->Delete (Existing);
    Existing = NULL;
    if (EFI_ERROR (Status)) {
      (*Root)->Close (*Root);
      *Root = NULL;
      return Status;
    }
  }

  Status = (*Root)->Open (
                     *Root,
                     Evidence,
                     OMNI_GUARDIAN_EVIDENCE_FILE,
                     EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE | EFI_FILE_MODE_CREATE,
                     0
                     );
  if (EFI_ERROR (Status)) {
    (*Root)->Close (*Root);
    *Root = NULL;
  }

  return Status;
}

STATIC EFI_STATUS
WriteTcg2Evidence (
  IN EFI_SYSTEM_TABLE  *SystemTable,
  IN EFI_FILE_PROTOCOL *Evidence
  )
{
  EFI_STATUS Status;
  EFI_TCG2_PROTOCOL *Tcg2;
  EFI_TCG2_BOOT_SERVICE_CAPABILITY Capability;

  Tcg2 = NULL;
  Status = SystemTable->BootServices->LocateProtocol (
                                       &gEfiTcg2ProtocolGuid,
                                       NULL,
                                       (VOID **)&Tcg2
                                       );
  if (EFI_ERROR (Status) || (Tcg2 == NULL)) {
    WriteStat (Evidence, "TCG2_PRESENT", 0);
    return EFI_SUCCESS;
  }

  WriteStat (Evidence, "TCG2_PRESENT", 1);

  Capability.Size = (UINT8)sizeof (Capability);
  Capability.StructureVersion.Major = 0;
  Capability.StructureVersion.Minor = 0;
  Capability.ProtocolVersion.Major = 0;
  Capability.ProtocolVersion.Minor = 0;
  Capability.HashAlgorithmBitmap = 0;
  Capability.SupportedEventLogs = 0;
  Capability.TPMPresentFlag = FALSE;
  Capability.MaxCommandSize = 0;
  Capability.MaxResponseSize = 0;
  Capability.ManufacturerID = 0;
  Capability.NumberOfPCRBanks = 0;
  Capability.ActivePcrBanks = 0;

  Status = Tcg2->GetCapability (Tcg2, &Capability);
  WriteStat (Evidence, "TCG2_CAPABILITY_OK", EFI_ERROR (Status) ? 0 : 1);
  if (EFI_ERROR (Status)) {
    return EFI_SUCCESS;
  }

  WriteStat (Evidence, "TPM_PRESENT", Capability.TPMPresentFlag ? 1 : 0);
  WriteStat (Evidence, "TCG2_HASH_ALG_BITMAP", Capability.HashAlgorithmBitmap);
  WriteStat (Evidence, "TCG2_SUPPORTED_LOGS", Capability.SupportedEventLogs);
  WriteStat (Evidence, "TCG2_PCR_BANKS", Capability.NumberOfPCRBanks);
  WriteStat (Evidence, "TCG2_ACTIVE_PCR_BANKS", Capability.ActivePcrBanks);
  WriteStat (Evidence, "TCG2_MAX_COMMAND", Capability.MaxCommandSize);
  WriteStat (Evidence, "TCG2_MAX_RESPONSE", Capability.MaxResponseSize);
  WriteStat (Evidence, "TCG2_MANUFACTURER_ID", Capability.ManufacturerID);

  return EFI_SUCCESS;
}

EFI_STATUS
EFIAPI
UefiMain (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  EFI_STATUS Status;
  EFI_FILE_PROTOCOL *Evidence;
  EFI_FILE_PROTOCOL *Root;
  UINTN Index;
  UINT8 Value;
  BOOLEAN Present;
  UINTN TotalNetworkCapabilities;

  OMNI_PROTOCOL_PROBE Probes[] = {
    { "SNP_HANDLES", &gEfiSimpleNetworkProtocolGuid },
    { "MNP_SERVICE_BINDING_HANDLES", &gEfiManagedNetworkServiceBindingProtocolGuid },
    { "IP4_SERVICE_BINDING_HANDLES", &gEfiIp4ServiceBindingProtocolGuid },
    { "IP6_SERVICE_BINDING_HANDLES", &gEfiIp6ServiceBindingProtocolGuid },
    { "DHCP4_SERVICE_BINDING_HANDLES", &gEfiDhcp4ServiceBindingProtocolGuid },
    { "DHCP6_SERVICE_BINDING_HANDLES", &gEfiDhcp6ServiceBindingProtocolGuid },
    { "DNS4_SERVICE_BINDING_HANDLES", &gEfiDns4ServiceBindingProtocolGuid },
    { "DNS6_SERVICE_BINDING_HANDLES", &gEfiDns6ServiceBindingProtocolGuid },
    { "TCP4_SERVICE_BINDING_HANDLES", &gEfiTcp4ServiceBindingProtocolGuid },
    { "TCP6_SERVICE_BINDING_HANDLES", &gEfiTcp6ServiceBindingProtocolGuid },
    { "UDP4_SERVICE_BINDING_HANDLES", &gEfiUdp4ServiceBindingProtocolGuid },
    { "UDP6_SERVICE_BINDING_HANDLES", &gEfiUdp6ServiceBindingProtocolGuid },
    { "HTTP_SERVICE_BINDING_HANDLES", &gEfiHttpServiceBindingProtocolGuid },
    { "TLS_SERVICE_BINDING_HANDLES", &gEfiTlsServiceBindingProtocolGuid },
    { "IPSEC2_HANDLES", &gEfiIpSec2ProtocolGuid },
    { "REST_HANDLES", &gEfiRestProtocolGuid },
    { "RNG_HANDLES", &gEfiRngProtocolGuid },
    { "FMP_HANDLES", &gEfiFirmwareManagementProtocolGuid },
    { "TCG2_HANDLES", &gEfiTcg2ProtocolGuid }
  };

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  if (SystemTable->ConOut != NULL) {
    SystemTable->ConOut->OutputString (
                           SystemTable->ConOut,
                           L"OMNI Guardian capability probe\r\n"
                           );
  }

  Evidence = NULL;
  Root = NULL;
  Status = OpenEvidenceFile (ImageHandle, SystemTable, &Evidence, &Root);
  if (EFI_ERROR (Status) || (Evidence == NULL)) {
    return Status;
  }

  WriteAscii (Evidence, "OMNI_GUARDIAN_V1\n");
  WriteAscii (Evidence, "SCHEMA=" OMNI_GUARDIAN_SCHEMA "\n");
  WriteAscii (Evidence, "MODE=READ_ONLY_CAPABILITY_DISCOVERY\n");

  Present = FALSE;
  Value = 0;
  ReadGlobalByte (SystemTable, EFI_SECURE_BOOT_MODE_NAME, &Value, &Present);
  WriteStat (Evidence, "SECURE_BOOT_PRESENT", Present ? 1 : 0);
  WriteStat (Evidence, "SECURE_BOOT", Present ? Value : 0);

  Present = FALSE;
  Value = 0;
  ReadGlobalByte (SystemTable, EFI_SETUP_MODE_NAME, &Value, &Present);
  WriteStat (Evidence, "SETUP_MODE_PRESENT", Present ? 1 : 0);
  WriteStat (Evidence, "SETUP_MODE", Present ? Value : 0);

  Present = FALSE;
  Value = 0;
  ReadGlobalByte (SystemTable, L"DeployedMode", &Value, &Present);
  WriteStat (Evidence, "DEPLOYED_MODE_PRESENT", Present ? 1 : 0);
  WriteStat (Evidence, "DEPLOYED_MODE", Present ? Value : 0);

  WriteStat (
    Evidence,
    "MEMORY_ATTRIBUTES_TABLE_PRESENT",
    HasConfigurationTable (SystemTable, &gEfiMemoryAttributesTableGuid) ? 1 : 0
    );

  TotalNetworkCapabilities = 0;
  for (Index = 0; Index < (sizeof (Probes) / sizeof (Probes[0])); ++Index) {
    UINTN Count;
    Count = CountProtocolHandles (SystemTable, Probes[Index].Guid);
    WriteStat (Evidence, Probes[Index].Name, Count);
    if (Index < 15) {
      TotalNetworkCapabilities += (Count != 0) ? 1 : 0;
    }
  }

  WriteStat (Evidence, "NETWORK_CAPABILITY_CLASSES_PRESENT", TotalNetworkCapabilities);
  WriteTcg2Evidence (SystemTable, Evidence);

  WriteAscii (Evidence, "VERDICT=CAPABILITY_INVENTORY_COMPLETE\n");
  Evidence->Flush (Evidence);
  Evidence->Close (Evidence);
  Root->Close (Root);

  if (SystemTable->ConOut != NULL) {
    SystemTable->ConOut->OutputString (
                           SystemTable->ConOut,
                           L"OMNI Guardian evidence written to \\OMNI-GUARDIAN.TXT\r\n"
                           );
  }

  return EFI_SUCCESS;
}
