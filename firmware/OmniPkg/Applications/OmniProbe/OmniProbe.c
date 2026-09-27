#include <Uefi.h>
#include <Protocol/HiiDatabase.h>
#include <Protocol/GraphicsOutput.h>
#include <Protocol/BlockIo.h>
#include <Protocol/PciIo.h>
#include <Protocol/LoadedImage.h>
#include <Protocol/SimpleFileSystem.h>
#include <Protocol/Smbios.h>
#include <Uefi/UefiInternalFormRepresentation.h>
#include <Library/IoLib.h>

#define OMNI_DEBUGCON_PORT 0x402
#define OMNI_COM1_BASE     0x3F8
#define OMNI_EVIDENCE_FILE  L"\\OMNI-EVIDENCE.TXT"
#define OMNI_DIAG_FILE      L"\\OMNI-DIAG.TXT"
#define OMNI_TRACE_FILE     L"\\OMNI-TRACE.TXT"
#define OMNI_CHALLENGE_FILE L"\\OMNI-CHALLENGE.TXT"
#define OMNI_SCREENREADER_FILE L"\\EFI\\OMNI\\SCREENREADER.EFI"
#define OMNI_SCREENREADER_MAX_BYTES (16U * 1024U * 1024U)
#define OMNI_CHALLENGE_HEX_LEN 64
#define OMNI_UUID_TEXT_LEN      36
#define OMNI_NVRAM_NAME          L"OmniBootEvidence"
#define OMNI_HDA_DMA_BUFFER_BYTES 4096U
#define OMNI_HDA_DMA_FORMAT       0x0011U /* 48 kHz, 16-bit, stereo */
#define OMNI_HDA_DMA_STREAM_TAG   1U
#define OMNI_HDA_TONE_HZ          750U
#define OMNI_HDA_TONE_RATE        48000U
#define OMNI_HDA_TONE_AMPLITUDE   8192
#define OMNI_HDA_TONE_MILLISECONDS 4000U

STATIC EFI_GUID mOmniEvidenceVariableGuid = {
  0x8d8a7e66, 0x0a4d, 0x4c9f,
  { 0x9d, 0x43, 0x8e, 0x1b, 0x68, 0xd8, 0x4f, 0x06 }
};

typedef struct {
  UINTN Handles;
  UINTN FormPackages;
  UINTN Opcodes;
  UINTN Questions;
  UINTN Passwords;
  UINTN InvalidPackages;
} OMNI_HII_STATS;

typedef struct {
  UINTN Handles;
  UINTN Modes;
  UINTN QueryPass;
  UINTN SetPass;
  UINTN BltPass;
  UINTN Width;
  UINTN Height;
  UINTN PixelFormat;
} OMNI_GOP_STATS;

typedef struct {
  UINTN PciHandles;
  UINTN Controllers;
  UINTN GpuHdmiControllersSkipped;
  UINTN SelectionPass;
  UINTN MmioReads;
  UINTN CodecBitmap;
  UINTN Segment;
  UINTN Bus;
  UINTN Device;
  UINTN Function;
  UINTN VendorId;
  UINTN DeviceId;
  UINTN Gcap;
  UINTN Gctl;
  UINTN Vmaj;
  UINTN Vmin;
  UINTN ImmediateCommands;
  UINTN CodecAddress;
  UINTN CodecVendorId;
  UINTN RootStartNode;
  UINTN RootNodeCount;
  UINTN AudioFunctionGroup;
  UINTN WidgetStartNode;
  UINTN WidgetCount;
  UINTN OutputConverters;
  UINTN PinWidgets;

  /* Read-only codec route evidence. */
  UINTN ConverterNode;
  UINTN ConverterWidgetCaps;
  UINTN ConverterPcmCaps;
  UINTN ConverterStreamFormats;
  UINTN PinNode;
  UINTN PinWidgetCaps;
  UINTN PinCapabilities;
  UINTN PinConfigDefault;
  UINTN PinControl;
  UINTN PinEapd;
  UINTN PinConnectionListLength;
  UINTN PinFirstConnection;
  UINTN RouteResolved;
  UINTN RouteResolvedDepth;
  UINTN RouteIntermediateNode;
  UINTN RouteIntermediateWidgetCaps;
  UINTN RouteIntermediateConnectionIndex;
  UINTN RouteIntermediateAmpProgrammed;
  UINTN RouteEvidence;
  UINTN PinDefaultDevice;
  UINTN PinPortConnectivity;
  UINTN PinSelectionScore;
  UINTN AnalogPinCandidates;

  /* Read-only HDA DMA/stream descriptor capability evidence. */
  UINTN InputStreams;
  UINTN OutputStreams;
  UINTN BidirStreams;
  UINTN Dma64Bit;
  UINTN FirstOutputStreamOffset;
  UINTN FirstOutputStreamCtl;
  UINTN FirstOutputStreamStatus;
  UINTN FirstOutputStreamLpib;
  UINTN FirstOutputStreamCbl;
  UINTN FirstOutputStreamLvi;
  UINTN FirstOutputStreamFormat;
  UINTN FirstOutputStreamBdpl;
  UINTN FirstOutputStreamBdpu;
  UINTN StreamCapabilityEvidence;
  UINTN DmaProgramAttempted;
  UINTN DmaProgrammed;
  UINTN DmaRunObserved;
  UINTN DmaLpibBefore;
  UINTN DmaLpibAfter;
  UINTN DmaProgress;
  UINTN DmaFormat;
  UINTN DmaStreamTag;
  UINTN DmaBufferBytes;
  UINTN DmaStatus;
  UINTN DmaTonePrepared;
  UINTN DmaToneFrames;
  UINTN DmaToneMilliseconds;
  UINTN RouteProgrammed;
  UINTN RoutePinControlAfter;
  UINTN RouteEapdCapable;
  UINTN RouteEapdAfter;
  UINTN ConverterAmpProgrammed;
  UINTN PinAmpProgrammed;

  /* PCI/MMIO validity evidence. */
  UINTN PciAttributesSupported;
  UINTN PciAttributesOriginal;
  UINTN PciAttributesAfter;
  UINTN PciCommandBefore;
  UINTN PciCommandAfter;
  UINTN MmioValid;
  UINTN ControllerResetReady;
  UINTN InvalidMmioReads;
} OMNI_HDA_STATS;

typedef struct {
  UINT64 Address;
  UINT32 Length;
  UINT32 Flags;
} OMNI_HDA_BDL_ENTRY;

STATIC VOID ZeroBytes (
  OUT VOID  *Buffer,
  IN  UINTN Size
  )
{
  UINT8 *Byte;
  UINTN Index;

  if (Buffer == NULL) return;
  Byte = (UINT8 *)Buffer;
  for (Index = 0; Index < Size; ++Index) {
    Byte[Index] = 0;
  }
}

STATIC UINTN AppendAsciiBounded (
  OUT CHAR8       *Buffer,
  IN UINTN        Capacity,
  IN UINTN        Index,
  IN CONST CHAR8  *Text
  )
{
  if ((Buffer == NULL) || (Text == NULL) || (Capacity == 0)) {
    return Index;
  }

  while ((*Text != '\0') && ((Index + 1) < Capacity)) {
    Buffer[Index++] = *Text++;
  }
  Buffer[Index] = '\0';
  return Index;
}

STATIC EFI_STATUS SaveNvramStage (
  EFI_SYSTEM_TABLE *SystemTable,
  CONST CHAR8      *Stage,
  CONST CHAR8      *Challenge
  )
{
  CHAR8 Buffer[192];
  UINTN Index;
  UINT32 Attributes;

  if ((SystemTable == NULL) || (SystemTable->RuntimeServices == NULL) || (Stage == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Buffer[0] = '\0';
  Index = 0;
  Index = AppendAsciiBounded (Buffer, sizeof (Buffer), Index, "OMNI_NVRAM_V1\n");
  Index = AppendAsciiBounded (Buffer, sizeof (Buffer), Index, "STAGE=");
  Index = AppendAsciiBounded (Buffer, sizeof (Buffer), Index, Stage);
  Index = AppendAsciiBounded (Buffer, sizeof (Buffer), Index, "\n");

  if (Challenge != NULL) {
    Index = AppendAsciiBounded (Buffer, sizeof (Buffer), Index, "CHALLENGE=");
    Index = AppendAsciiBounded (Buffer, sizeof (Buffer), Index, Challenge);
    Index = AppendAsciiBounded (Buffer, sizeof (Buffer), Index, "\n");
  }

  Attributes =
    EFI_VARIABLE_NON_VOLATILE |
    EFI_VARIABLE_BOOTSERVICE_ACCESS |
    EFI_VARIABLE_RUNTIME_ACCESS;

  return SystemTable->RuntimeServices->SetVariable (
                                        OMNI_NVRAM_NAME,
                                        &mOmniEvidenceVariableGuid,
                                        Attributes,
                                        Index,
                                        Buffer
                                        );
}

STATIC EFI_STATUS FlushLoadedImageBlockDevice (
  EFI_HANDLE        ImageHandle,
  EFI_SYSTEM_TABLE  *SystemTable
  )
{
  EFI_STATUS Status;
  EFI_LOADED_IMAGE_PROTOCOL *LoadedImage;
  EFI_BLOCK_IO_PROTOCOL *BlockIo;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  LoadedImage = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      ImageHandle,
                                      &gEfiLoadedImageProtocolGuid,
                                      (VOID **)&LoadedImage
                                      );
  if (EFI_ERROR (Status) || (LoadedImage == NULL)) {
    return EFI_NOT_FOUND;
  }

  BlockIo = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      LoadedImage->DeviceHandle,
                                      &gEfiBlockIoProtocolGuid,
                                      (VOID **)&BlockIo
                                      );
  if (EFI_ERROR (Status) || (BlockIo == NULL)) {
    return EFI_UNSUPPORTED;
  }

  if ((BlockIo->Media == NULL) || !BlockIo->Media->MediaPresent) {
    return EFI_NO_MEDIA;
  }

  return BlockIo->FlushBlocks (BlockIo);
}

STATIC EFI_STATUS ApplyPersistenceBarrier (
  EFI_HANDLE        ImageHandle,
  EFI_SYSTEM_TABLE  *SystemTable,
  EFI_STATUS        PriorStatus
  )
{
  EFI_STATUS FlushStatus;

  if (EFI_ERROR (PriorStatus)) {
    return PriorStatus;
  }

  FlushStatus = FlushLoadedImageBlockDevice (ImageHandle, SystemTable);
  if ((FlushStatus == EFI_UNSUPPORTED) || (FlushStatus == EFI_NOT_FOUND)) {
    return PriorStatus;
  }
  if (EFI_ERROR (FlushStatus)) {
    return FlushStatus;
  }

  return PriorStatus;
}

STATIC VOID SerialInit (VOID) {
  IoWrite8 (OMNI_COM1_BASE + 1, 0x00);
  IoWrite8 (OMNI_COM1_BASE + 3, 0x80);
  IoWrite8 (OMNI_COM1_BASE + 0, 0x01);
  IoWrite8 (OMNI_COM1_BASE + 1, 0x00);
  IoWrite8 (OMNI_COM1_BASE + 3, 0x03);
  IoWrite8 (OMNI_COM1_BASE + 2, 0xC7);
  IoWrite8 (OMNI_COM1_BASE + 4, 0x0B);
}

STATIC VOID SerialWriteChar (CHAR8 Ch) {
  UINTN Spin;
  for (Spin = 0; Spin < 100000; ++Spin) {
    if ((IoRead8 (OMNI_COM1_BASE + 5) & 0x20) != 0) {
      IoWrite8 (OMNI_COM1_BASE, (UINT8)Ch);
      return;
    }
  }
}

STATIC VOID WriteChar (CHAR8 Ch) {
  IoWrite8 (OMNI_DEBUGCON_PORT, (UINT8)Ch);
  SerialWriteChar (Ch);
}

STATIC VOID WriteText (CONST CHAR8 *Text) {
  while (*Text != '\0') {
    WriteChar (*Text++);
  }
}

STATIC VOID WriteUint (UINTN Value) {
  CHAR8 Digits[32];
  UINTN Count = 0;
  if (Value == 0) {
    WriteChar ('0');
    return;
  }
  while ((Value != 0) && (Count < sizeof (Digits))) {
    Digits[Count++] = (CHAR8)('0' + (Value % 10));
    Value /= 10;
  }
  while (Count != 0) {
    WriteChar (Digits[--Count]);
  }
}

STATIC VOID WriteStat (CONST CHAR8 *Name, UINTN Value) {
  WriteText (Name);
  WriteChar ('=');
  WriteUint (Value);
  WriteChar ('\n');
}

STATIC CHAR8 HexDigit (UINT8 Value) {
  return (CHAR8)((Value < 10) ? ('0' + Value) : ('a' + (Value - 10)));
}

STATIC VOID FormatHex (
  UINT64 Value,
  UINTN  Digits,
  CHAR8  *Buffer,
  IN OUT UINTN *Index
  )
{
  UINTN Digit;
  UINTN Shift;

  for (Digit = 0; Digit < Digits; ++Digit) {
    Shift = (Digits - Digit - 1) * 4;
    Buffer[(*Index)++] = HexDigit ((UINT8)((Value >> Shift) & 0x0F));
  }
}

STATIC VOID FormatGuidAscii (
  CONST GUID *Uuid,
  OUT CHAR8  Text[OMNI_UUID_TEXT_LEN + 1]
  )
{
  UINTN Index;

  Index = 0;
  FormatHex (Uuid->Data1, 8, Text, &Index);
  Text[Index++] = '-';
  FormatHex (Uuid->Data2, 4, Text, &Index);
  Text[Index++] = '-';
  FormatHex (Uuid->Data3, 4, Text, &Index);
  Text[Index++] = '-';
  FormatHex (Uuid->Data4[0], 2, Text, &Index);
  FormatHex (Uuid->Data4[1], 2, Text, &Index);
  Text[Index++] = '-';
  FormatHex (Uuid->Data4[2], 2, Text, &Index);
  FormatHex (Uuid->Data4[3], 2, Text, &Index);
  FormatHex (Uuid->Data4[4], 2, Text, &Index);
  FormatHex (Uuid->Data4[5], 2, Text, &Index);
  FormatHex (Uuid->Data4[6], 2, Text, &Index);
  FormatHex (Uuid->Data4[7], 2, Text, &Index);
  Text[Index] = '\0';
}

STATIC BOOLEAN GuidIsMeaningful (CONST GUID *Uuid) {
  CONST UINT8 *Bytes;
  UINTN Index;
  BOOLEAN AnyNonZero;
  BOOLEAN AnyNotFf;

  Bytes = (CONST UINT8 *)Uuid;
  AnyNonZero = FALSE;
  AnyNotFf = FALSE;
  for (Index = 0; Index < sizeof (GUID); ++Index) {
    if (Bytes[Index] != 0x00) {
      AnyNonZero = TRUE;
    }
    if (Bytes[Index] != 0xFF) {
      AnyNotFf = TRUE;
    }
  }

  return (BOOLEAN)(AnyNonZero && AnyNotFf);
}

STATIC EFI_STATUS FileWriteAscii (
  EFI_FILE_PROTOCOL *File,
  CONST CHAR8       *Text
  )
{
  UINTN Size;

  if ((File == NULL) || (Text == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Size = 0;
  while (Text[Size] != '\0') {
    Size++;
  }
  if (Size == 0) {
    return EFI_SUCCESS;
  }
  return File->Write (File, &Size, (VOID *)Text);
}

STATIC EFI_STATUS FileWriteUint (
  EFI_FILE_PROTOCOL *File,
  UINTN             Value
  )
{
  CHAR8 Digits[32];
  CHAR8 Forward[32];
  UINTN Count;
  UINTN Index;
  UINTN Size;

  Count = 0;
  if (Value == 0) {
    Forward[0] = '0';
    Size = 1;
    return File->Write (File, &Size, Forward);
  }

  while ((Value != 0) && (Count < sizeof (Digits))) {
    Digits[Count++] = (CHAR8)('0' + (Value % 10));
    Value /= 10;
  }

  for (Index = 0; Index < Count; ++Index) {
    Forward[Index] = Digits[Count - Index - 1];
  }

  Size = Count;
  return File->Write (File, &Size, Forward);
}

STATIC EFI_STATUS FileWriteStat (
  EFI_FILE_PROTOCOL *File,
  CONST CHAR8       *Name,
  UINTN             Value
  )
{
  EFI_STATUS Status;

  Status = FileWriteAscii (File, Name);
  if (EFI_ERROR (Status)) {
    return Status;
  }
  Status = FileWriteAscii (File, "=");
  if (EFI_ERROR (Status)) {
    return Status;
  }
  Status = FileWriteUint (File, Value);
  if (EFI_ERROR (Status)) {
    return Status;
  }
  return FileWriteAscii (File, "\n");
}

STATIC BOOLEAN IsHexChar (CHAR8 Ch) {
  return (BOOLEAN)(
    ((Ch >= '0') && (Ch <= '9')) ||
    ((Ch >= 'a') && (Ch <= 'f')) ||
    ((Ch >= 'A') && (Ch <= 'F'))
    );
}

STATIC EFI_STATUS LoadChallenge (
  EFI_HANDLE       ImageHandle,
  EFI_SYSTEM_TABLE *SystemTable,
  OUT CHAR8        Challenge[OMNI_CHALLENGE_HEX_LEN + 1]
  )
{
  EFI_STATUS Status;
  EFI_LOADED_IMAGE_PROTOCOL *LoadedImage;
  EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *FileSystem;
  EFI_FILE_PROTOCOL *Root;
  EFI_FILE_PROTOCOL *File;
  UINT8 Buffer[OMNI_CHALLENGE_HEX_LEN + 2];
  UINT8 Extra;
  UINTN Size;
  UINTN ExtraSize;
  UINTN Index;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) || (Challenge == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  LoadedImage = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      ImageHandle,
                                      &gEfiLoadedImageProtocolGuid,
                                      (VOID **)&LoadedImage
                                      );
  if (EFI_ERROR (Status) || (LoadedImage == NULL)) {
    return EFI_NOT_FOUND;
  }

  FileSystem = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      LoadedImage->DeviceHandle,
                                      &gEfiSimpleFileSystemProtocolGuid,
                                      (VOID **)&FileSystem
                                      );
  if (EFI_ERROR (Status) || (FileSystem == NULL)) {
    return EFI_NOT_FOUND;
  }

  Root = NULL;
  Status = FileSystem->OpenVolume (FileSystem, &Root);
  if (EFI_ERROR (Status) || (Root == NULL)) {
    return Status;
  }

  File = NULL;
  Status = Root->Open (
                   Root,
                   &File,
                   OMNI_CHALLENGE_FILE,
                   EFI_FILE_MODE_READ,
                   0
                   );
  if (EFI_ERROR (Status) || (File == NULL)) {
    Root->Close (Root);
    return EFI_NOT_FOUND;
  }

  Size = sizeof (Buffer);
  Status = File->Read (File, &Size, Buffer);
  if (!EFI_ERROR (Status)) {
    ExtraSize = 1;
    Status = File->Read (File, &ExtraSize, &Extra);
    if (!EFI_ERROR (Status) && (ExtraSize != 0)) {
      Status = EFI_BAD_BUFFER_SIZE;
    }
  }

  File->Close (File);
  Root->Close (Root);
  if (EFI_ERROR (Status)) {
    return Status;
  }

  while ((Size != 0) && ((Buffer[Size - 1] == '\r') || (Buffer[Size - 1] == '\n'))) {
    Size--;
  }
  if (Size != OMNI_CHALLENGE_HEX_LEN) {
    return EFI_COMPROMISED_DATA;
  }

  for (Index = 0; Index < OMNI_CHALLENGE_HEX_LEN; ++Index) {
    if (!IsHexChar ((CHAR8)Buffer[Index])) {
      return EFI_COMPROMISED_DATA;
    }
    Challenge[Index] = (CHAR8)Buffer[Index];
  }
  Challenge[OMNI_CHALLENGE_HEX_LEN] = '\0';
  return EFI_SUCCESS;
}

STATIC EFI_STATUS LoadPlatformUuid (
  EFI_SYSTEM_TABLE *SystemTable,
  OUT CHAR8        PlatformUuid[OMNI_UUID_TEXT_LEN + 1]
  )
{
  EFI_STATUS Status;
  EFI_SMBIOS_PROTOCOL *Smbios;
  EFI_SMBIOS_HANDLE Handle;
  EFI_SMBIOS_TYPE Type;
  EFI_SMBIOS_TABLE_HEADER *Record;
  SMBIOS_TABLE_TYPE1 *Type1;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) ||
      (PlatformUuid == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Smbios = NULL;
  Status = SystemTable->BootServices->LocateProtocol (
                                      &gEfiSmbiosProtocolGuid,
                                      NULL,
                                      (VOID **)&Smbios
                                      );
  if (EFI_ERROR (Status) || (Smbios == NULL)) {
    return EFI_NOT_FOUND;
  }

  Handle = SMBIOS_HANDLE_PI_RESERVED;
  Type = SMBIOS_TYPE_SYSTEM_INFORMATION;
  Record = NULL;
  Status = Smbios->GetNext (
                     Smbios,
                     &Handle,
                     &Type,
                     &Record,
                     NULL
                     );
  if (EFI_ERROR (Status) || (Record == NULL)) {
    return EFI_NOT_FOUND;
  }

  if (Record->Length < (OFFSET_OF (SMBIOS_TABLE_TYPE1, Uuid) + sizeof (GUID))) {
    return EFI_COMPROMISED_DATA;
  }

  Type1 = (SMBIOS_TABLE_TYPE1 *)Record;
  if (!GuidIsMeaningful (&Type1->Uuid)) {
    return EFI_COMPROMISED_DATA;
  }

  FormatGuidAscii (&Type1->Uuid, PlatformUuid);
  return EFI_SUCCESS;
}


STATIC EFI_STATUS SaveTraceStage (
  EFI_HANDLE       ImageHandle,
  EFI_SYSTEM_TABLE *SystemTable,
  CONST CHAR8      *Stage
  )
{
  EFI_STATUS Status;
  EFI_LOADED_IMAGE_PROTOCOL *LoadedImage;
  EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *FileSystem;
  EFI_FILE_PROTOCOL *Root;
  EFI_FILE_PROTOCOL *File;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) || (Stage == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  LoadedImage = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      ImageHandle,
                                      &gEfiLoadedImageProtocolGuid,
                                      (VOID **)&LoadedImage
                                      );
  if (EFI_ERROR (Status) || (LoadedImage == NULL)) {
    return EFI_NOT_FOUND;
  }

  FileSystem = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      LoadedImage->DeviceHandle,
                                      &gEfiSimpleFileSystemProtocolGuid,
                                      (VOID **)&FileSystem
                                      );
  if (EFI_ERROR (Status) || (FileSystem == NULL)) {
    return EFI_NOT_FOUND;
  }

  Root = NULL;
  Status = FileSystem->OpenVolume (FileSystem, &Root);
  if (EFI_ERROR (Status) || (Root == NULL)) {
    return Status;
  }

  File = NULL;
  Status = Root->Open (
                   Root,
                   &File,
                   OMNI_TRACE_FILE,
                   EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE,
                   0
                   );
  if (!EFI_ERROR (Status) && (File != NULL)) {
    Status = File->Delete (File);
    File = NULL;
    if (EFI_ERROR (Status)) {
      Root->Close (Root);
      return Status;
    }
  }

  Status = Root->Open (
                   Root,
                   &File,
                   OMNI_TRACE_FILE,
                   EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE | EFI_FILE_MODE_CREATE,
                   0
                   );
  if (EFI_ERROR (Status) || (File == NULL)) {
    Root->Close (Root);
    return Status;
  }

  Status = FileWriteAscii (File, "OMNI_TRACE_V1\n");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, "STAGE=");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, Stage);
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, "\n");
  if (!EFI_ERROR (Status)) Status = File->Flush (File);

  File->Close (File);
  Root->Close (Root);
  return ApplyPersistenceBarrier (ImageHandle, SystemTable, Status);
}

STATIC EFI_STATUS ProbeGop (
  EFI_SYSTEM_TABLE *SystemTable,
  OUT OMNI_GOP_STATS *Stats
  )
{
  EFI_STATUS Status;
  EFI_STATUS FirstError;
  EFI_HANDLE *Handles;
  UINTN HandleCount;
  UINTN Index;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) || (Stats == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Handles = NULL;
  HandleCount = 0;
  FirstError = EFI_NOT_FOUND;

  Status = SystemTable->BootServices->LocateHandleBuffer (
                                      ByProtocol,
                                      &gEfiGraphicsOutputProtocolGuid,
                                      NULL,
                                      &HandleCount,
                                      &Handles
                                      );
  if (EFI_ERROR (Status) || (HandleCount == 0) || (Handles == NULL)) {
    return Status;
  }

  for (Index = 0; Index < HandleCount; ++Index) {
    EFI_GRAPHICS_OUTPUT_PROTOCOL *Gop;
    UINT32 ModeIndex;

    Gop = NULL;
    Status = SystemTable->BootServices->HandleProtocol (
                                        Handles[Index],
                                        &gEfiGraphicsOutputProtocolGuid,
                                        (VOID **)&Gop
                                        );
    if (EFI_ERROR (Status) || (Gop == NULL)) {
      if (FirstError == EFI_NOT_FOUND) {
        FirstError = Status;
      }
      continue;
    }

    Stats->Handles++;
    FirstError = EFI_SUCCESS;

    if ((Gop->Mode != NULL) && (Gop->Mode->Info != NULL)) {
      if (Stats->Width == 0) {
        Stats->Width = Gop->Mode->Info->HorizontalResolution;
        Stats->Height = Gop->Mode->Info->VerticalResolution;
        Stats->PixelFormat = Gop->Mode->Info->PixelFormat;
      }

      Status = Gop->SetMode (Gop, Gop->Mode->Mode);
      if (!EFI_ERROR (Status)) {
        EFI_GRAPHICS_OUTPUT_BLT_PIXEL Pixel;

        Stats->SetPass++;
        Pixel.Blue = 0x30;
        Pixel.Green = 0x20;
        Pixel.Red = 0x10;
        Pixel.Reserved = 0;

        if ((Gop->Mode->Info != NULL) &&
            (Gop->Mode->Info->HorizontalResolution != 0) &&
            (Gop->Mode->Info->VerticalResolution != 0)) {
          Status = Gop->Blt (
                          Gop,
                          &Pixel,
                          EfiBltVideoFill,
                          0,
                          0,
                          0,
                          0,
                          Gop->Mode->Info->HorizontalResolution,
                          Gop->Mode->Info->VerticalResolution,
                          0
                          );
          if (!EFI_ERROR (Status)) {
            Stats->BltPass++;
          }
        }
      }
    }

    if ((Gop->Mode == NULL) || (Gop->Mode->MaxMode == 0)) {
      continue;
    }

    for (ModeIndex = 0; ModeIndex < Gop->Mode->MaxMode; ++ModeIndex) {
      EFI_GRAPHICS_OUTPUT_MODE_INFORMATION *Info;
      UINTN InfoSize;

      Info = NULL;
      InfoSize = 0;
      Status = Gop->QueryMode (Gop, ModeIndex, &InfoSize, &Info);
      if (!EFI_ERROR (Status) && (Info != NULL)) {
        Stats->QueryPass++;
        Stats->Modes++;
      }
      if (Info != NULL) {
        SystemTable->BootServices->FreePool (Info);
      }
    }
  }

  SystemTable->BootServices->FreePool (Handles);
  return FirstError;
}

STATIC UINT32 HdaVerb (
  UINTN Codec,
  UINTN Node,
  UINTN Verb,
  UINTN Payload
  )
{
  return (UINT32)(
           ((Codec & 0x0F) << 28) |
           ((Node & 0xFF) << 20) |
           ((Verb & 0x0FFF) << 8) |
           (Payload & 0xFF)
           );
}

STATIC UINT32 HdaVerb16 (
  UINTN Codec,
  UINTN Node,
  UINTN Verb,
  UINTN Payload
  )
{
  return (UINT32)(
           ((Codec & 0x0F) << 28) |
           ((Node & 0xFF) << 20) |
           ((Verb & 0x0F) << 16) |
           (Payload & 0xFFFF)
           );
}

STATIC EFI_STATUS HdaImmediateCommand (
  EFI_PCI_IO_PROTOCOL *PciIo,
  UINT32              Command,
  OUT UINT32          *Response
  )
{
  EFI_STATUS Status;
  UINTN Spin;
  UINT16 Icis;
  UINT32 Value;

  if ((PciIo == NULL) || (Response == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  for (Spin = 0; Spin < 100000; ++Spin) {
    Icis = 0;
    Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint16, 0, 0x68, 1, &Icis);
    if (EFI_ERROR (Status)) return Status;
    if ((Icis & 0x0001) == 0) break;
  }
  if (Spin == 100000) return EFI_TIMEOUT;

  Icis = 0x0002;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, 0x68, 1, &Icis);
  if (EFI_ERROR (Status)) return Status;

  Value = Command;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint32, 0, 0x60, 1, &Value);
  if (EFI_ERROR (Status)) return Status;

  Icis = 0x0001;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, 0x68, 1, &Icis);
  if (EFI_ERROR (Status)) return Status;

  for (Spin = 0; Spin < 200000; ++Spin) {
    Icis = 0;
    Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint16, 0, 0x68, 1, &Icis);
    if (EFI_ERROR (Status)) return Status;
    if (((Icis & 0x0001) == 0) && ((Icis & 0x0002) != 0)) break;
  }
  if (Spin == 200000) return EFI_TIMEOUT;

  Value = 0;
  Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint32, 0, 0x64, 1, &Value);
  if (EFI_ERROR (Status)) return Status;
  *Response = Value;

  Icis = 0x0002;
  PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, 0x68, 1, &Icis);
  return EFI_SUCCESS;
}

STATIC EFI_STATUS HdaGetParameter (
  EFI_PCI_IO_PROTOCOL *PciIo,
  UINTN               Codec,
  UINTN               Node,
  UINTN               Parameter,
  OUT UINT32           *Response
  )
{
  return HdaImmediateCommand (
           PciIo,
           HdaVerb (Codec, Node, 0xF00, Parameter),
           Response
           );
}

STATIC EFI_STATUS HdaGetConnectionNodeAt (
  EFI_PCI_IO_PROTOCOL *PciIo,
  UINTN               Codec,
  UINTN               Node,
  UINT32              ConnectionLength,
  UINTN               Index,
  OUT UINTN            *ConnectionNode
  )
{
  EFI_STATUS Status;
  UINT32 Response;
  UINTN Count;
  UINTN BaseIndex;
  UINTN Shift;
  UINTN Entry;
  UINTN RangeMask;
  UINTN NodeMask;
  BOOLEAN LongForm;

  if ((PciIo == NULL) || (ConnectionNode == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Count = ConnectionLength & 0x7FU;
  LongForm = ((ConnectionLength & 0x80U) != 0);
  if ((Count == 0) || (Index >= Count)) {
    return EFI_NOT_FOUND;
  }

  BaseIndex = LongForm ? (Index & ~(UINTN)1U) : (Index & ~(UINTN)3U);
  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb (Codec, Node, 0xF02, BaseIndex),
             &Response
             );
  if (EFI_ERROR (Status)) {
    return Status;
  }

  Shift = LongForm ? ((Index - BaseIndex) * 16U) : ((Index - BaseIndex) * 8U);
  Entry = (Response >> Shift) & (LongForm ? 0xFFFFU : 0xFFU);
  RangeMask = LongForm ? 0x8000U : 0x80U;
  NodeMask = LongForm ? 0x7FFFU : 0x7FU;

  /*
   * Range-encoded connection lists need expansion against the previous entry.
   * Do not guess: fail closed. Realtek ALC256 speaker paths use independent
   * short-form NIDs, including the common 0x14 -> 0x0c -> 0x02 route.
   */
  if ((Entry & RangeMask) != 0) {
    return EFI_UNSUPPORTED;
  }

  *ConnectionNode = Entry & NodeMask;
  return (*ConnectionNode != 0) ? EFI_SUCCESS : EFI_NOT_FOUND;
}

STATIC EFI_STATUS ResolveHdaOutputConverter (
  EFI_PCI_IO_PROTOCOL *PciIo,
  IN OUT OMNI_HDA_STATS *Stats
  )
{
  EFI_STATUS Status;
  UINTN CurrentNode;
  UINTN Depth;
  UINTN TryIndex;
  UINTN Index;
  UINTN PreferredIndex;
  UINTN Count;
  UINTN ScanCount;
  UINTN NextNode;
  UINTN NextIndex;
  UINTN ChildNode;
  UINTN WidgetType;
  UINTN ChildWidgetType;
  UINT32 CurrentCaps;
  UINT32 ChildCaps;
  UINT32 ConnectionLength;
  UINT32 Response;

  if ((PciIo == NULL) || (Stats == NULL) || (Stats->PinNode == 0)) {
    return EFI_INVALID_PARAMETER;
  }

  CurrentNode = Stats->PinNode;

  /*
   * Resolve a bounded sink->source route. The common ALC256 speaker topology
   * is pin 0x14 -> mixer 0x0c -> DAC 0x02, so a direct pin->DAC assumption is
   * not sufficient. Prefer the current selector index, then inspect remaining
   * connections for a directly reachable output converter.
   */
  for (Depth = 0; Depth < 4; ++Depth) {
    CurrentCaps = 0;
    Status = HdaGetParameter (
               PciIo,
               Stats->CodecAddress,
               CurrentNode,
               0x09,
               &CurrentCaps
               );
    if (EFI_ERROR (Status)) {
      return Status;
    }
    Stats->ImmediateCommands++;
    WidgetType = (CurrentCaps >> 20) & 0x0FU;

    if (WidgetType == 0x00U) {
      Stats->ConverterNode = CurrentNode;
      Stats->ConverterWidgetCaps = CurrentCaps;
      Stats->RouteResolved = 1;
      Stats->RouteResolvedDepth = Depth;

      if (!EFI_ERROR (HdaGetParameter (
                       PciIo,
                       Stats->CodecAddress,
                       CurrentNode,
                       0x0A,
                       &Response
                       ))) {
        Stats->ImmediateCommands++;
        Stats->ConverterPcmCaps = Response;
        Stats->StreamCapabilityEvidence++;
      }

      if (!EFI_ERROR (HdaGetParameter (
                       PciIo,
                       Stats->CodecAddress,
                       CurrentNode,
                       0x0B,
                       &Response
                       ))) {
        Stats->ImmediateCommands++;
        Stats->ConverterStreamFormats = Response;
        Stats->StreamCapabilityEvidence++;
      }
      return EFI_SUCCESS;
    }

    ConnectionLength = 0;
    Status = HdaGetParameter (
               PciIo,
               Stats->CodecAddress,
               CurrentNode,
               0x0E,
               &ConnectionLength
               );
    if (EFI_ERROR (Status)) {
      return Status;
    }
    Stats->ImmediateCommands++;
    Count = ConnectionLength & 0x7FU;
    if (Count == 0) {
      return EFI_NOT_FOUND;
    }

    PreferredIndex = 0;
    ScanCount = Count;
    if ((Count > 1) && ((WidgetType == 0x03U) || (WidgetType == 0x04U))) {
      /*
       * A selector/pin exposes exactly one active connection.  Resolve only
       * that connection unless we explicitly program a new selection; scanning
       * alternate entries would create false route evidence.
       */
      Response = 0;
      Status = HdaImmediateCommand (
                 PciIo,
                 HdaVerb (Stats->CodecAddress, CurrentNode, 0xF01, 0),
                 &Response
                 );
      if (EFI_ERROR (Status)) {
        return Status;
      }
      Stats->ImmediateCommands++;
      PreferredIndex = Response & 0xFFU;
      if (PreferredIndex >= Count) {
        return EFI_COMPROMISED_DATA;
      }
      ScanCount = 1;
    }

    NextNode = 0;
    NextIndex = 0;
    for (TryIndex = 0; TryIndex < ScanCount; ++TryIndex) {
      Index = (ScanCount == 1) ? PreferredIndex : TryIndex;

      ChildNode = 0;
      Status = HdaGetConnectionNodeAt (
                 PciIo,
                 Stats->CodecAddress,
                 CurrentNode,
                 ConnectionLength,
                 Index,
                 &ChildNode
                 );
      if (EFI_ERROR (Status)) {
        continue;
      }
      Stats->ImmediateCommands++;

      ChildCaps = 0;
      Status = HdaGetParameter (
                 PciIo,
                 Stats->CodecAddress,
                 ChildNode,
                 0x09,
                 &ChildCaps
                 );
      if (EFI_ERROR (Status)) {
        continue;
      }
      Stats->ImmediateCommands++;
      ChildWidgetType = (ChildCaps >> 20) & 0x0FU;

      if (ChildWidgetType == 0x00U) {
        if (CurrentNode != Stats->PinNode) {
          Stats->RouteIntermediateNode = CurrentNode;
          Stats->RouteIntermediateWidgetCaps = CurrentCaps;
          Stats->RouteIntermediateConnectionIndex = Index;
        }

        Stats->ConverterNode = ChildNode;
        Stats->ConverterWidgetCaps = ChildCaps;
        Stats->RouteResolved = 1;
        Stats->RouteResolvedDepth = Depth + 1U;

        if (!EFI_ERROR (HdaGetParameter (
                         PciIo,
                         Stats->CodecAddress,
                         ChildNode,
                         0x0A,
                         &Response
                         ))) {
          Stats->ImmediateCommands++;
          Stats->ConverterPcmCaps = Response;
          Stats->StreamCapabilityEvidence++;
        }

        if (!EFI_ERROR (HdaGetParameter (
                         PciIo,
                         Stats->CodecAddress,
                         ChildNode,
                         0x0B,
                         &Response
                         ))) {
          Stats->ImmediateCommands++;
          Stats->ConverterStreamFormats = Response;
          Stats->StreamCapabilityEvidence++;
        }
        return EFI_SUCCESS;
      }

      if (NextNode == 0) {
        NextNode = ChildNode;
        NextIndex = Index;
      }
    }

    if (NextNode == 0) {
      return EFI_NOT_FOUND;
    }

    if (CurrentNode != Stats->PinNode) {
      Stats->RouteIntermediateNode = CurrentNode;
      Stats->RouteIntermediateWidgetCaps = CurrentCaps;
      Stats->RouteIntermediateConnectionIndex = NextIndex;
    }
    CurrentNode = NextNode;
  }

  return EFI_NOT_FOUND;
}

STATIC VOID ProbeHdaCodecTopology (
  EFI_PCI_IO_PROTOCOL *PciIo,
  IN OUT OMNI_HDA_STATS *Stats
  )
{
  UINTN Codec;
  UINT32 Response;
  UINTN StartNode;
  UINTN NodeCount;
  UINTN Index;
  UINTN BestPinScore;

  if ((PciIo == NULL) || (Stats == NULL) || (Stats->CodecBitmap == 0)) return;

  for (Codec = 0; Codec < 15; ++Codec) {
    if ((Stats->CodecBitmap & (1U << Codec)) != 0) {
      Stats->CodecAddress = Codec;
      break;
    }
  }
  if (Codec == 15) return;

  if (!EFI_ERROR (HdaGetParameter (PciIo, Codec, 0, 0x00, &Response))) {
    Stats->ImmediateCommands++;
    Stats->CodecVendorId = Response;
  }
  if (EFI_ERROR (HdaGetParameter (PciIo, Codec, 0, 0x04, &Response))) return;
  Stats->ImmediateCommands++;
  StartNode = (Response >> 16) & 0xFF;
  NodeCount = Response & 0xFF;
  Stats->RootStartNode = StartNode;
  Stats->RootNodeCount = NodeCount;

  for (Index = 0; Index < NodeCount; ++Index) {
    UINTN Node;
    Node = StartNode + Index;
    if (EFI_ERROR (HdaGetParameter (PciIo, Codec, Node, 0x05, &Response))) continue;
    Stats->ImmediateCommands++;
    if ((Response & 0xFF) == 0x01) {
      Stats->AudioFunctionGroup = Node;
      break;
    }
  }
  if (Stats->AudioFunctionGroup == 0) return;

  if (EFI_ERROR (HdaGetParameter (
                   PciIo,
                   Codec,
                   Stats->AudioFunctionGroup,
                   0x04,
                   &Response
                   ))) {
    return;
  }
  Stats->ImmediateCommands++;
  StartNode = (Response >> 16) & 0xFF;
  NodeCount = Response & 0xFF;
  Stats->WidgetStartNode = StartNode;
  Stats->WidgetCount = NodeCount;
  BestPinScore = 0;

  for (Index = 0; Index < NodeCount; ++Index) {
    UINTN Node;
    UINTN WidgetType;

    Node = StartNode + Index;
    if (EFI_ERROR (HdaGetParameter (PciIo, Codec, Node, 0x09, &Response))) continue;
    Stats->ImmediateCommands++;
    WidgetType = (Response >> 20) & 0x0F;

    if (WidgetType == 0x00) {
      Stats->OutputConverters++;

      if (Stats->ConverterNode == 0) {
        Stats->ConverterNode = Node;
        Stats->ConverterWidgetCaps = Response;

        if (!EFI_ERROR (HdaGetParameter (PciIo, Codec, Node, 0x0A, &Response))) {
          Stats->ImmediateCommands++;
          Stats->ConverterPcmCaps = Response;
          Stats->StreamCapabilityEvidence++;
        }

        if (!EFI_ERROR (HdaGetParameter (PciIo, Codec, Node, 0x0B, &Response))) {
          Stats->ImmediateCommands++;
          Stats->ConverterStreamFormats = Response;
          Stats->StreamCapabilityEvidence++;
        }
      }
    }

    if (WidgetType == 0x04) {
      UINT32 CandidateWidgetCaps;
      UINT32 CandidatePinCaps;
      UINT32 CandidateConfig;
      UINT32 CandidatePinControl;
      UINT32 CandidateEapd;
      UINT32 CandidateConnectionLength;
      UINT32 CandidateFirstConnection;
      UINTN CandidateEvidence;
      UINTN CandidateDefaultDevice;
      UINTN CandidatePortConnectivity;
      UINTN CandidateScore;

      Stats->PinWidgets++;
      CandidateWidgetCaps = Response;
      CandidatePinCaps = 0;
      CandidateConfig = 0;
      CandidatePinControl = 0;
      CandidateEapd = 0;
      CandidateConnectionLength = 0;
      CandidateFirstConnection = 0;
      CandidateEvidence = 0;
      CandidateScore = 0;

      if (!EFI_ERROR (HdaGetParameter (PciIo, Codec, Node, 0x0C, &Response))) {
        Stats->ImmediateCommands++;
        CandidatePinCaps = Response;
        CandidateEvidence++;
      }

      if (!EFI_ERROR (HdaImmediateCommand (
                        PciIo,
                        HdaVerb (Codec, Node, 0xF1C, 0),
                        &Response
                        ))) {
        Stats->ImmediateCommands++;
        CandidateConfig = Response;
        CandidateEvidence++;
      }

      if (!EFI_ERROR (HdaImmediateCommand (
                        PciIo,
                        HdaVerb (Codec, Node, 0xF07, 0),
                        &Response
                        ))) {
        Stats->ImmediateCommands++;
        CandidatePinControl = Response & 0xFF;
        CandidateEvidence++;
      }

      if (!EFI_ERROR (HdaImmediateCommand (
                        PciIo,
                        HdaVerb (Codec, Node, 0xF0C, 0),
                        &Response
                        ))) {
        Stats->ImmediateCommands++;
        CandidateEapd = Response & 0xFF;
        CandidateEvidence++;
      }

      if (!EFI_ERROR (HdaGetParameter (PciIo, Codec, Node, 0x0E, &Response))) {
        Stats->ImmediateCommands++;
        CandidateConnectionLength = Response;
        CandidateEvidence++;
      }

      if ((CandidateConnectionLength & 0x7F) != 0) {
        if (!EFI_ERROR (HdaImmediateCommand (
                          PciIo,
                          HdaVerb (Codec, Node, 0xF02, 0),
                          &Response
                          ))) {
          Stats->ImmediateCommands++;
          CandidateFirstConnection = Response & 0xFF;
          CandidateEvidence++;
        }
      }

      CandidateDefaultDevice = (CandidateConfig >> 20) & 0x0F;
      CandidatePortConnectivity = (CandidateConfig >> 30) & 0x03;

      /*
       * Select only physically connected analog outputs.
       * HDA default-device priority for the laptop path:
       *   Speaker > Headphone Out > Line Out.
       * Prefer fixed/built-in pins over jacks for equal device class.
       */
      if (CandidatePortConnectivity != 0x01) {
        if (CandidateDefaultDevice == 0x01) {
          CandidateScore = 300;
        } else if (CandidateDefaultDevice == 0x02) {
          CandidateScore = 200;
        } else if (CandidateDefaultDevice == 0x00) {
          CandidateScore = 100;
        }

        if (CandidateScore != 0) {
          Stats->AnalogPinCandidates++;
          if (CandidatePortConnectivity == 0x02) {
            CandidateScore += 20;
          } else if (CandidatePortConnectivity == 0x03) {
            CandidateScore += 10;
          }
        }
      }

      if ((CandidateScore != 0) && (CandidateScore > BestPinScore)) {
        UINT32 ConnectedCaps;
        UINTN ConnectedWidgetType;

        BestPinScore = CandidateScore;
        Stats->PinNode = Node;
        Stats->PinWidgetCaps = CandidateWidgetCaps;
        Stats->PinCapabilities = CandidatePinCaps;
        Stats->PinConfigDefault = CandidateConfig;
        Stats->PinControl = CandidatePinControl;
        Stats->PinEapd = CandidateEapd;
        Stats->PinConnectionListLength = CandidateConnectionLength;
        Stats->PinFirstConnection = CandidateFirstConnection;
        Stats->RouteEvidence = CandidateEvidence;
        Stats->PinDefaultDevice = CandidateDefaultDevice;
        Stats->PinPortConnectivity = CandidatePortConnectivity;
        Stats->PinSelectionScore = CandidateScore;

        /*
         * If the selected pin directly names an output converter, bind the
         * converter evidence to that route instead of keeping an unrelated
         * first converter discovered earlier.
         */
        if (CandidateFirstConnection != 0) {
          ConnectedCaps = 0;
          if (!EFI_ERROR (HdaGetParameter (
                           PciIo,
                           Codec,
                           CandidateFirstConnection,
                           0x09,
                           &ConnectedCaps
                           ))) {
            Stats->ImmediateCommands++;
            ConnectedWidgetType = (ConnectedCaps >> 20) & 0x0F;
            if (ConnectedWidgetType == 0x00) {
              Stats->ConverterNode = CandidateFirstConnection;
              Stats->ConverterWidgetCaps = ConnectedCaps;

              if (!EFI_ERROR (HdaGetParameter (
                               PciIo,
                               Codec,
                               CandidateFirstConnection,
                               0x0A,
                               &Response
                               ))) {
                Stats->ImmediateCommands++;
                Stats->ConverterPcmCaps = Response;
                Stats->StreamCapabilityEvidence++;
              }

              if (!EFI_ERROR (HdaGetParameter (
                               PciIo,
                               Codec,
                               CandidateFirstConnection,
                               0x0B,
                               &Response
                               ))) {
                Stats->ImmediateCommands++;
                Stats->ConverterStreamFormats = Response;
                Stats->StreamCapabilityEvidence++;
              }
            }
          }
        }
      }
    }
  }

  if (Stats->PinNode != 0) {
    ResolveHdaOutputConverter (PciIo, Stats);
  }
}

STATIC VOID FillHdaTone (
  OUT VOID  *Buffer,
  IN  UINTN Bytes
  )
{
  INT16 *Samples;
  UINTN Frames;
  UINTN Frame;
  UINTN PeriodFrames;
  INT16 Sample;

  if ((Buffer == NULL) || (Bytes < 4)) return;

  Samples = (INT16 *)Buffer;
  Frames = Bytes / (sizeof (INT16) * 2U);
  PeriodFrames = OMNI_HDA_TONE_RATE / OMNI_HDA_TONE_HZ;
  if (PeriodFrames < 2) PeriodFrames = 2;

  for (Frame = 0; Frame < Frames; ++Frame) {
    Sample = ((Frame % PeriodFrames) < (PeriodFrames / 2U))
      ? (INT16)OMNI_HDA_TONE_AMPLITUDE
      : (INT16)(-OMNI_HDA_TONE_AMPLITUDE);

    Samples[(Frame * 2U) + 0U] = Sample;
    Samples[(Frame * 2U) + 1U] = Sample;
  }
}

STATIC EFI_STATUS ProgramHdaOutputRoute (
  EFI_PCI_IO_PROTOCOL *PciIo,
  IN OUT OMNI_HDA_STATS *Stats
  )
{
  EFI_STATUS Status;
  UINT32 Response;
  UINT32 AmpCaps;
  UINTN PinControl;
  UINTN Eapd;
  UINTN Gain;

  if ((PciIo == NULL) || (Stats == NULL) ||
      (Stats->ConverterNode == 0) || (Stats->PinNode == 0) ||
      (Stats->RouteResolved == 0)) {
    return EFI_INVALID_PARAMETER;
  }

  /*
   * Put the audio function group, converter and selected physical pin in D0.
   * These are standard HDA power-state verbs and are harmless when already D0.
   */
  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb (Stats->CodecAddress, Stats->AudioFunctionGroup, 0x705, 0),
             &Response
             );
  if (EFI_ERROR (Status)) return Status;
  Stats->ImmediateCommands++;

  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb (Stats->CodecAddress, Stats->ConverterNode, 0x705, 0),
             &Response
             );
  if (EFI_ERROR (Status)) return Status;
  Stats->ImmediateCommands++;

  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb (Stats->CodecAddress, Stats->PinNode, 0x705, 0),
             &Response
             );
  if (EFI_ERROR (Status)) return Status;
  Stats->ImmediateCommands++;

  /*
   * ALC256 speaker routes commonly include an Audio Mixer between the pin and
   * DAC. If that widget exposes an input amplifier, unmute the selected input
   * at its declared 0 dB index. The amplifier index is the connection-list
   * index per the HDA specification.
   */
  if ((Stats->RouteIntermediateNode != 0) &&
      ((Stats->RouteIntermediateWidgetCaps & (1U << 1)) != 0) &&
      (Stats->RouteIntermediateConnectionIndex <= 0x0FU)) {
    AmpCaps = 0;
    if (!EFI_ERROR (HdaGetParameter (
                     PciIo,
                     Stats->CodecAddress,
                     Stats->RouteIntermediateNode,
                     0x0D,
                     &AmpCaps
                     ))) {
      Stats->ImmediateCommands++;
      Gain = AmpCaps & 0x7FU;
      Response = 0;
      Status = HdaImmediateCommand (
                 PciIo,
                 HdaVerb16 (
                   Stats->CodecAddress,
                   Stats->RouteIntermediateNode,
                   0x3,
                   0x7000U |
                     ((Stats->RouteIntermediateConnectionIndex & 0x0FU) << 8) |
                     Gain
                   ),
                 &Response
                 );
      if (!EFI_ERROR (Status)) {
        Stats->ImmediateCommands++;
        Stats->RouteIntermediateAmpProgrammed = 1;
      }
    }
  }

  /*
   * Enable the physical output pin. For a headphone pin, also assert HP enable.
   */
  PinControl = (Stats->PinControl | 0x40U) & 0xFFU;
  if (Stats->PinDefaultDevice == 0x02U) {
    PinControl |= 0x80U;
  }

  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb (Stats->CodecAddress, Stats->PinNode, 0x707, PinControl),
             &Response
             );
  if (EFI_ERROR (Status)) return Status;
  Stats->ImmediateCommands++;

  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb (Stats->CodecAddress, Stats->PinNode, 0xF07, 0),
             &Response
             );
  if (EFI_ERROR (Status)) return Status;
  Stats->ImmediateCommands++;
  Stats->RoutePinControlAfter = Response & 0xFFU;

  /*
   * If the pin advertises EAPD support, enable the external amplifier.
   */
  Stats->RouteEapdCapable = ((Stats->PinCapabilities & (1U << 16)) != 0) ? 1U : 0U;
  if (Stats->RouteEapdCapable != 0) {
    Eapd = (Stats->PinEapd | 0x02U) & 0xFFU;

    Response = 0;
    Status = HdaImmediateCommand (
               PciIo,
               HdaVerb (Stats->CodecAddress, Stats->PinNode, 0x70C, Eapd),
               &Response
               );
    if (EFI_ERROR (Status)) return Status;
    Stats->ImmediateCommands++;

    Response = 0;
    Status = HdaImmediateCommand (
               PciIo,
               HdaVerb (Stats->CodecAddress, Stats->PinNode, 0xF0C, 0),
               &Response
               );
    if (EFI_ERROR (Status)) return Status;
    Stats->ImmediateCommands++;
    Stats->RouteEapdAfter = Response & 0xFFU;
  }

  /*
   * Unmute output amplifiers at their codec-declared 0 dB index.
   * AC_AMPCAP_OFFSET is the 0 dB gain index.
   */
  if ((Stats->ConverterWidgetCaps & (1U << 2)) != 0) {
    AmpCaps = 0;
    if (!EFI_ERROR (HdaGetParameter (
                     PciIo,
                     Stats->CodecAddress,
                     Stats->ConverterNode,
                     0x12,
                     &AmpCaps
                     ))) {
      Stats->ImmediateCommands++;
      Gain = AmpCaps & 0x7FU;
      Response = 0;
      Status = HdaImmediateCommand (
                 PciIo,
                 HdaVerb16 (
                   Stats->CodecAddress,
                   Stats->ConverterNode,
                   0x3,
                   0xB000U | Gain
                   ),
                 &Response
                 );
      if (!EFI_ERROR (Status)) {
        Stats->ImmediateCommands++;
        Stats->ConverterAmpProgrammed = 1;
      }
    }
  }

  if ((Stats->PinWidgetCaps & (1U << 2)) != 0) {
    AmpCaps = 0;
    if (!EFI_ERROR (HdaGetParameter (
                     PciIo,
                     Stats->CodecAddress,
                     Stats->PinNode,
                     0x12,
                     &AmpCaps
                     ))) {
      Stats->ImmediateCommands++;
      Gain = AmpCaps & 0x7FU;
      Response = 0;
      Status = HdaImmediateCommand (
                 PciIo,
                 HdaVerb16 (
                   Stats->CodecAddress,
                   Stats->PinNode,
                   0x3,
                   0xB000U | Gain
                   ),
                 &Response
                 );
      if (!EFI_ERROR (Status)) {
        Stats->ImmediateCommands++;
        Stats->PinAmpProgrammed = 1;
      }
    }
  }

  if ((Stats->RoutePinControlAfter & 0x40U) == 0) {
    return EFI_DEVICE_ERROR;
  }

  if ((Stats->RouteEapdCapable != 0) &&
      ((Stats->RouteEapdAfter & 0x02U) == 0)) {
    return EFI_DEVICE_ERROR;
  }

  Stats->RouteProgrammed = 1;
  return EFI_SUCCESS;
}

STATIC EFI_STATUS ProgramHdaDmaProof (
  EFI_PCI_IO_PROTOCOL *PciIo,
  EFI_SYSTEM_TABLE    *SystemTable,
  IN OUT OMNI_HDA_STATS *Stats
  )
{
  EFI_STATUS Status;
  EFI_STATUS FinalStatus;
  VOID *AudioHost;
  VOID *BdlHost;
  VOID *AudioMapping;
  VOID *BdlMapping;
  EFI_PHYSICAL_ADDRESS AudioDevice;
  EFI_PHYSICAL_ADDRESS BdlDevice;
  UINTN AudioBytes;
  UINTN BdlBytes;
  OMNI_HDA_BDL_ENTRY *Bdl;
  UINTN StreamOffset;
  UINT16 StreamCtlLow;
  UINT8 StreamCtlHigh;
  UINT8 StreamStatus;
  UINT32 Value32;
  UINT16 Value16;
  UINT32 Response;
  UINT32 PreviousLpib;
  UINT32 CurrentLpib;
  UINTN Spin;
  BOOLEAN StreamStarted;

  if ((PciIo == NULL) || (SystemTable == NULL) ||
      (SystemTable->BootServices == NULL) || (Stats == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Stats->DmaProgramAttempted = 1;
  Stats->DmaFormat = OMNI_HDA_DMA_FORMAT;
  Stats->DmaStreamTag = OMNI_HDA_DMA_STREAM_TAG;
  Stats->DmaBufferBytes = OMNI_HDA_DMA_BUFFER_BYTES;

  if ((Stats->MmioValid == 0) || (Stats->ControllerResetReady == 0) ||
      (Stats->OutputStreams == 0) || (Stats->FirstOutputStreamOffset == 0) ||
      (Stats->ConverterNode == 0)) {
    Stats->DmaStatus = (UINTN)EFI_NOT_READY;
    return EFI_NOT_READY;
  }

  AudioHost = NULL;
  BdlHost = NULL;
  AudioMapping = NULL;
  BdlMapping = NULL;
  AudioDevice = 0;
  BdlDevice = 0;
  AudioBytes = OMNI_HDA_DMA_BUFFER_BYTES;
  BdlBytes = 4096;
  StreamStarted = FALSE;
  FinalStatus = EFI_SUCCESS;
  StreamOffset = Stats->FirstOutputStreamOffset;

  Status = PciIo->AllocateBuffer (PciIo, AllocateAnyPages, EfiBootServicesData, 1, &AudioHost, 0);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Status = PciIo->AllocateBuffer (PciIo, AllocateAnyPages, EfiBootServicesData, 1, &BdlHost, 0);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  FillHdaTone (AudioHost, OMNI_HDA_DMA_BUFFER_BYTES);
  Stats->DmaTonePrepared = 1;
  Stats->DmaToneFrames = OMNI_HDA_DMA_BUFFER_BYTES / (sizeof (INT16) * 2U);
  Stats->DmaToneMilliseconds = OMNI_HDA_TONE_MILLISECONDS;
  ZeroBytes (BdlHost, 4096);

  Status = PciIo->Map (
                    PciIo,
                    EfiPciIoOperationBusMasterCommonBuffer,
                    AudioHost,
                    &AudioBytes,
                    &AudioDevice,
                    &AudioMapping
                    );
  if (EFI_ERROR (Status) || (AudioBytes < OMNI_HDA_DMA_BUFFER_BYTES)) {
    FinalStatus = EFI_ERROR (Status) ? Status : EFI_BAD_BUFFER_SIZE;
    goto Cleanup;
  }

  Status = PciIo->Map (
                    PciIo,
                    EfiPciIoOperationBusMasterCommonBuffer,
                    BdlHost,
                    &BdlBytes,
                    &BdlDevice,
                    &BdlMapping
                    );
  if (EFI_ERROR (Status) || (BdlBytes < sizeof (OMNI_HDA_BDL_ENTRY))) {
    FinalStatus = EFI_ERROR (Status) ? Status : EFI_BAD_BUFFER_SIZE;
    goto Cleanup;
  }

  Bdl = (OMNI_HDA_BDL_ENTRY *)BdlHost;
  Bdl[0].Address = (UINT64)AudioDevice;
  Bdl[0].Length = OMNI_HDA_DMA_BUFFER_BYTES;
  Bdl[0].Flags = 0;

  Status = PciIo->Flush (PciIo);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  StreamCtlLow = 0;
  Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  StreamCtlLow &= (UINT16)~0x0002U;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  StreamCtlLow |= 0x0001U;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  for (Spin = 0; Spin < 1000; ++Spin) {
    SystemTable->BootServices->Stall (10);
    StreamCtlLow = 0;
    Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
    if (!EFI_ERROR (Status) && ((StreamCtlLow & 0x0001U) != 0)) break;
  }
  if (Spin == 1000) { FinalStatus = EFI_TIMEOUT; goto Cleanup; }

  StreamCtlLow &= (UINT16)~0x0001U;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  for (Spin = 0; Spin < 1000; ++Spin) {
    SystemTable->BootServices->Stall (10);
    StreamCtlLow = 0;
    Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
    if (!EFI_ERROR (Status) && ((StreamCtlLow & 0x0001U) == 0)) break;
  }
  if (Spin == 1000) { FinalStatus = EFI_TIMEOUT; goto Cleanup; }

  StreamStatus = 0x1CU;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint8, 0, StreamOffset + 0x03, 1, &StreamStatus);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Value32 = OMNI_HDA_DMA_BUFFER_BYTES;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint32, 0, StreamOffset + 0x08, 1, &Value32);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Value16 = 0;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x0C, 1, &Value16);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Value16 = OMNI_HDA_DMA_FORMAT;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x12, 1, &Value16);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Value32 = (UINT32)(BdlDevice & 0xFFFFFFFFULL);
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint32, 0, StreamOffset + 0x18, 1, &Value32);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Value32 = (UINT32)(BdlDevice >> 32);
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint32, 0, StreamOffset + 0x1C, 1, &Value32);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  /*
   * Keep the analog codec route as an independent hardware gate.
   *
   * A pin/EAPD/amplifier failure must not prevent us from proving the
   * controller-side DMA transport itself.  The stream descriptor, converter
   * stream tag, RUN bit and LPIB are valid independent observations even when
   * the selected physical pin is not yet routable.
   */
  Status = ProgramHdaOutputRoute (PciIo, Stats);
  if (EFI_ERROR (Status)) {
    Stats->RouteProgrammed = 0;
    Status = EFI_SUCCESS;
  }

  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb16 (Stats->CodecAddress, Stats->ConverterNode, 0x2, OMNI_HDA_DMA_FORMAT),
             &Response
             );
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }
  Stats->ImmediateCommands++;

  Response = 0;
  Status = HdaImmediateCommand (
             PciIo,
             HdaVerb (
               Stats->CodecAddress,
               Stats->ConverterNode,
               0x706,
               (OMNI_HDA_DMA_STREAM_TAG << 4)
               ),
             &Response
             );
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }
  Stats->ImmediateCommands++;

  StreamCtlHigh = 0;
  Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint8, 0, StreamOffset + 0x02, 1, &StreamCtlHigh);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  StreamCtlHigh = (UINT8)((StreamCtlHigh & 0x0FU) | (OMNI_HDA_DMA_STREAM_TAG << 4));
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint8, 0, StreamOffset + 0x02, 1, &StreamCtlHigh);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Stats->FirstOutputStreamCbl = OMNI_HDA_DMA_BUFFER_BYTES;
  Stats->FirstOutputStreamLvi = 0;
  Stats->FirstOutputStreamFormat = OMNI_HDA_DMA_FORMAT;
  Stats->FirstOutputStreamBdpl = (UINTN)(BdlDevice & 0xFFFFFFFFULL);
  Stats->FirstOutputStreamBdpu = (UINTN)(BdlDevice >> 32);
  Stats->DmaProgrammed = 1;

  Status = PciIo->Flush (PciIo);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  PreviousLpib = 0;
  Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint32, 0, StreamOffset + 0x04, 1, &PreviousLpib);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }
  Stats->DmaLpibBefore = PreviousLpib;

  StreamCtlLow = 0;
  Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  StreamCtlLow |= 0x0002U;
  Status = PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  /*
   * Make the posted MMIO write visible before judging RUN.  Then poll the
   * controller rather than treating a single immediate read as authoritative.
   */
  Status = PciIo->Flush (PciIo);
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }
  StreamStarted = TRUE;

  for (Spin = 0; Spin < 1000; ++Spin) {
    StreamCtlLow = 0;
    Status = PciIo->Mem.Read (
                         PciIo,
                         EfiPciIoWidthUint16,
                         0,
                         StreamOffset + 0x00,
                         1,
                         &StreamCtlLow
                         );
    if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

    if ((StreamCtlLow & 0x0002U) != 0) {
      Stats->DmaRunObserved = 1;
      break;
    }

    SystemTable->BootServices->Stall (10);
  }

  StreamCtlHigh = 0;
  Status = PciIo->Mem.Read (
                       PciIo,
                       EfiPciIoWidthUint8,
                       0,
                       StreamOffset + 0x02,
                       1,
                       &StreamCtlHigh
                       );
  if (EFI_ERROR (Status)) { FinalStatus = Status; goto Cleanup; }

  Stats->FirstOutputStreamCtl = StreamCtlLow | ((UINTN)StreamCtlHigh << 16);

  if (Stats->DmaRunObserved == 0) {
    FinalStatus = EFI_TIMEOUT;
    goto Cleanup;
  }

  CurrentLpib = PreviousLpib;
  for (Spin = 0; Spin < OMNI_HDA_TONE_MILLISECONDS; ++Spin) {
    UINT32 SampledLpib;

    SystemTable->BootServices->Stall (1000);
    SampledLpib = 0;
    Status = PciIo->Mem.Read (PciIo, EfiPciIoWidthUint32, 0, StreamOffset + 0x04, 1, &SampledLpib);
    if (EFI_ERROR (Status)) { FinalStatus = Status; break; }

    if (SampledLpib != CurrentLpib) {
      Stats->DmaProgress = 1;
    }
    CurrentLpib = SampledLpib;
  }

  Stats->DmaLpibAfter = CurrentLpib;
  Stats->FirstOutputStreamLpib = CurrentLpib;
  if (!EFI_ERROR (FinalStatus) && (Stats->DmaProgress == 0)) { FinalStatus = EFI_TIMEOUT; }

Cleanup:
  if (StreamStarted) {
    StreamCtlLow = 0;
    if (!EFI_ERROR (PciIo->Mem.Read (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow))) {
      StreamCtlLow &= (UINT16)~0x0002U;
      PciIo->Mem.Write (PciIo, EfiPciIoWidthUint16, 0, StreamOffset + 0x00, 1, &StreamCtlLow);
    }
    Response = 0;
    HdaImmediateCommand (
      PciIo,
      HdaVerb (Stats->CodecAddress, Stats->ConverterNode, 0x706, 0),
      &Response
      );
  }

  PciIo->Flush (PciIo);
  if (BdlMapping != NULL) { PciIo->Unmap (PciIo, BdlMapping); }
  if (AudioMapping != NULL) { PciIo->Unmap (PciIo, AudioMapping); }
  if (BdlHost != NULL) { PciIo->FreeBuffer (PciIo, 1, BdlHost); }
  if (AudioHost != NULL) { PciIo->FreeBuffer (PciIo, 1, AudioHost); }

  Stats->DmaStatus = (UINTN)FinalStatus;
  return FinalStatus;
}

STATIC EFI_STATUS ProbeHda (
  EFI_SYSTEM_TABLE *SystemTable,
  OUT OMNI_HDA_STATS *Stats
  )
{
  EFI_STATUS Status;
  EFI_HANDLE *Handles;
  UINTN HandleCount;
  UINTN Index;
  UINTN Pass;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) || (Stats == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Handles = NULL;
  HandleCount = 0;
  Status = SystemTable->BootServices->LocateHandleBuffer (
                                      ByProtocol,
                                      &gEfiPciIoProtocolGuid,
                                      NULL,
                                      &HandleCount,
                                      &Handles
                                      );
  if (EFI_ERROR (Status) || (Handles == NULL)) {
    return Status;
  }

  /*
   * Two passes. An APU laptop exposes the GPU's HDMI/DP audio controller
   * (PCI vendor 0x1002; 0x10DE on NVIDIA) next to the platform HDA controller
   * that carries the analog codec wired to the speaker. On the ASUS M1603QA
   * PCI order puts the HDMI one first (bus 3 fn 1, 1002:1637), so the probe
   * never saw an analog pin. Pass 0 skips GPU HDMI controllers; pass 1 takes
   * them only when no other controller produced a valid MMIO window.
   */
  for (Pass = 0; (Pass < 2) && (Stats->MmioValid == 0); ++Pass) {
  Stats->SelectionPass = Pass;
  for (Index = 0; Index < HandleCount; ++Index) {
    EFI_PCI_IO_PROTOCOL *PciIo;
    UINT16 VendorId;
    UINT16 DeviceId;
    UINT8 SubClass;
    UINT8 BaseClass;

    PciIo = NULL;
    Status = SystemTable->BootServices->HandleProtocol (
                                        Handles[Index],
                                        &gEfiPciIoProtocolGuid,
                                        (VOID **)&PciIo
                                        );
    if (EFI_ERROR (Status) || (PciIo == NULL)) {
      continue;
    }

    if (Pass == 0) Stats->PciHandles++;
    VendorId = 0xFFFF;
    DeviceId = 0xFFFF;
    SubClass = 0;
    BaseClass = 0;

    Status = PciIo->Pci.Read (PciIo, EfiPciIoWidthUint16, 0x00, 1, &VendorId);
    if (EFI_ERROR (Status) || (VendorId == 0xFFFF)) continue;
    Status = PciIo->Pci.Read (PciIo, EfiPciIoWidthUint16, 0x02, 1, &DeviceId);
    if (EFI_ERROR (Status)) continue;
    Status = PciIo->Pci.Read (PciIo, EfiPciIoWidthUint8, 0x0A, 1, &SubClass);
    if (EFI_ERROR (Status)) continue;
    Status = PciIo->Pci.Read (PciIo, EfiPciIoWidthUint8, 0x0B, 1, &BaseClass);
    if (EFI_ERROR (Status)) continue;

    if ((BaseClass == 0x04) && (SubClass == 0x03)) {
      UINTN Segment;
      UINTN Bus;
      UINTN Device;
      UINTN Function;
      UINT16 Gcap;
      UINT16 StateSts;
      UINT32 Gctl;
      UINT8 Vmin;
      UINT8 Vmaj;
      UINT64 SupportedAttributes;
      UINT64 CurrentAttributes;
      UINT16 PciCommandBefore;
      UINT16 PciCommandAfter;
      EFI_STATUS AttributeStatus;

      if (Pass == 0) Stats->Controllers++;
      if (Stats->MmioValid != 0) continue;
      if ((Pass == 0) && ((VendorId == 0x1002U) || (VendorId == 0x10DEU))) {
        Stats->GpuHdmiControllersSkipped++;
        continue;
      }

      Stats->VendorId = VendorId;
      Stats->DeviceId = DeviceId;
      Segment = 0;
      Bus = 0;
      Device = 0;
      Function = 0;
      if (!EFI_ERROR (PciIo->GetLocation (PciIo, &Segment, &Bus, &Device, &Function))) {
        Stats->Segment = Segment;
        Stats->Bus = Bus;
        Stats->Device = Device;
        Stats->Function = Function;
      }

      SupportedAttributes = 0;
      CurrentAttributes = 0;
      PciCommandBefore = 0xFFFF;
      PciCommandAfter = 0xFFFF;

      if (!EFI_ERROR (PciIo->Attributes (
                               PciIo,
                               EfiPciIoAttributeOperationSupported,
                               0,
                               &SupportedAttributes
                               ))) {
        Stats->PciAttributesSupported = (UINTN)SupportedAttributes;
      }

      if (!EFI_ERROR (PciIo->Attributes (
                               PciIo,
                               EfiPciIoAttributeOperationGet,
                               0,
                               &CurrentAttributes
                               ))) {
        Stats->PciAttributesOriginal = (UINTN)CurrentAttributes;
      }

      PciIo->Pci.Read (
                   PciIo,
                   EfiPciIoWidthUint16,
                   0x04,
                   1,
                   &PciCommandBefore
                   );
      Stats->PciCommandBefore = PciCommandBefore;

      AttributeStatus = PciIo->Attributes (
                                 PciIo,
                                 EfiPciIoAttributeOperationEnable,
                                 EFI_PCI_IO_ATTRIBUTE_MEMORY |
                                 EFI_PCI_IO_ATTRIBUTE_BUS_MASTER,
                                 NULL
                                 );
      if (EFI_ERROR (AttributeStatus)) {
        Stats->InvalidMmioReads++;
        continue;
      }

      CurrentAttributes = 0;
      if (!EFI_ERROR (PciIo->Attributes (
                               PciIo,
                               EfiPciIoAttributeOperationGet,
                               0,
                               &CurrentAttributes
                               ))) {
        Stats->PciAttributesAfter = (UINTN)CurrentAttributes;
      }

      PciIo->Pci.Read (
                   PciIo,
                   EfiPciIoWidthUint16,
                   0x04,
                   1,
                   &PciCommandAfter
                   );
      Stats->PciCommandAfter = PciCommandAfter;

      Gcap = 0xFFFF;
      Vmin = 0xFF;
      Vmaj = 0xFF;
      Gctl = 0xFFFFFFFF;
      StateSts = 0xFFFF;

      Status = PciIo->Mem.Read (
                            PciIo,
                            EfiPciIoWidthUint16,
                            0,
                            0x00,
                            1,
                            &Gcap
                            );
      if (EFI_ERROR (Status) || (Gcap == 0xFFFF) || (Gcap == 0)) {
        Stats->InvalidMmioReads++;
        continue;
      }

      Stats->Gcap = Gcap;
      Stats->MmioReads++;
      Stats->OutputStreams = (Gcap >> 12) & 0x0F;
      Stats->InputStreams = (Gcap >> 8) & 0x0F;
      Stats->BidirStreams = (Gcap >> 3) & 0x1F;
      Stats->Dma64Bit = Gcap & 0x01;

      Status = PciIo->Mem.Read (
                            PciIo,
                            EfiPciIoWidthUint8,
                            0,
                            0x02,
                            1,
                            &Vmin
                            );
      if (EFI_ERROR (Status) || (Vmin == 0xFF)) {
        Stats->InvalidMmioReads++;
        continue;
      }
      Stats->Vmin = Vmin;
      Stats->MmioReads++;

      Status = PciIo->Mem.Read (
                            PciIo,
                            EfiPciIoWidthUint8,
                            0,
                            0x03,
                            1,
                            &Vmaj
                            );
      if (EFI_ERROR (Status) || (Vmaj == 0xFF)) {
        Stats->InvalidMmioReads++;
        continue;
      }
      Stats->Vmaj = Vmaj;
      Stats->MmioReads++;

      Status = PciIo->Mem.Read (
                            PciIo,
                            EfiPciIoWidthUint32,
                            0,
                            0x08,
                            1,
                            &Gctl
                            );
      if (EFI_ERROR (Status) || (Gctl == 0xFFFFFFFF)) {
        Stats->InvalidMmioReads++;
        continue;
      }
      Stats->Gctl = Gctl;
      Stats->MmioReads++;

      if ((Gctl & 0x00000001) == 0) {
        UINT32 NewGctl;
        UINTN ResetSpin;

        NewGctl = Gctl | 0x00000001;
        Status = PciIo->Mem.Write (
                              PciIo,
                              EfiPciIoWidthUint32,
                              0,
                              0x08,
                              1,
                              &NewGctl
                              );
        if (EFI_ERROR (Status)) {
          Stats->InvalidMmioReads++;
          continue;
        }

        for (ResetSpin = 0; ResetSpin < 1000; ++ResetSpin) {
          SystemTable->BootServices->Stall (100);
          Gctl = 0xFFFFFFFF;
          Status = PciIo->Mem.Read (
                                PciIo,
                                EfiPciIoWidthUint32,
                                0,
                                0x08,
                                1,
                                &Gctl
                                );
          if (EFI_ERROR (Status) || (Gctl == 0xFFFFFFFF)) {
            continue;
          }
          if ((Gctl & 0x00000001) != 0) {
            break;
          }
        }
      }

      if ((Gctl == 0xFFFFFFFF) || ((Gctl & 0x00000001) == 0)) {
        Stats->InvalidMmioReads++;
        continue;
      }

      Stats->Gctl = Gctl;
      Stats->ControllerResetReady = 1;
      SystemTable->BootServices->Stall (1000);

      StateSts = 0xFFFF;
      Status = PciIo->Mem.Read (
                            PciIo,
                            EfiPciIoWidthUint16,
                            0,
                            0x0E,
                            1,
                            &StateSts
                            );
      if (EFI_ERROR (Status) || (StateSts == 0xFFFF)) {
        Stats->InvalidMmioReads++;
        continue;
      }
      Stats->MmioReads++;
      Stats->CodecBitmap = StateSts & 0x7FFF;

      if (Stats->CodecBitmap == 0) {
        continue;
      }

      Stats->MmioValid = 1;

      if (Stats->OutputStreams != 0) {
        UINTN StreamOffset;
        UINT16 StreamCtlLow;
        UINT8 StreamCtlHigh;
        UINT8 StreamStatus;
        UINT32 StreamLpib;
        UINT32 StreamCbl;
        UINT16 StreamLvi;
        UINT16 StreamFormat;
        UINT32 StreamBdpl;
        UINT32 StreamBdpu;
        BOOLEAN StreamCtlLowValid;
        BOOLEAN StreamCtlHighValid;

        StreamOffset = 0x80 + (Stats->InputStreams * 0x20);
        Stats->FirstOutputStreamOffset = StreamOffset;

        StreamCtlLow = 0;
        StreamCtlHigh = 0;
        StreamStatus = 0;
        StreamLpib = 0;
        StreamCbl = 0;
        StreamLvi = 0;
        StreamFormat = 0;
        StreamBdpl = 0;
        StreamBdpu = 0;
        StreamCtlLowValid = FALSE;
        StreamCtlHighValid = FALSE;

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint16,
                              0,
                              StreamOffset + 0x00,
                              1,
                              &StreamCtlLow
                              );
        if (!EFI_ERROR (Status) && (StreamCtlLow != 0xFFFF)) {
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
          StreamCtlLowValid = TRUE;
        } else {
          Stats->InvalidMmioReads++;
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint8,
                              0,
                              StreamOffset + 0x02,
                              1,
                              &StreamCtlHigh
                              );
        if (!EFI_ERROR (Status) && (StreamCtlHigh != 0xFF)) {
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
          StreamCtlHighValid = TRUE;
        } else {
          Stats->InvalidMmioReads++;
        }

        if (StreamCtlLowValid && StreamCtlHighValid) {
          Stats->FirstOutputStreamCtl =
            StreamCtlLow | ((UINTN)StreamCtlHigh << 16);
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint8,
                              0,
                              StreamOffset + 0x03,
                              1,
                              &StreamStatus
                              );
        if (!EFI_ERROR (Status) && (StreamStatus != 0xFF)) {
          Stats->FirstOutputStreamStatus = StreamStatus;
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
        } else {
          Stats->InvalidMmioReads++;
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint32,
                              0,
                              StreamOffset + 0x04,
                              1,
                              &StreamLpib
                              );
        if (!EFI_ERROR (Status) && (StreamLpib != 0xFFFFFFFF)) {
          Stats->FirstOutputStreamLpib = StreamLpib;
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
        } else {
          Stats->InvalidMmioReads++;
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint32,
                              0,
                              StreamOffset + 0x08,
                              1,
                              &StreamCbl
                              );
        if (!EFI_ERROR (Status) && (StreamCbl != 0xFFFFFFFF)) {
          Stats->FirstOutputStreamCbl = StreamCbl;
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
        } else {
          Stats->InvalidMmioReads++;
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint16,
                              0,
                              StreamOffset + 0x0C,
                              1,
                              &StreamLvi
                              );
        if (!EFI_ERROR (Status) && (StreamLvi != 0xFFFF)) {
          Stats->FirstOutputStreamLvi = StreamLvi;
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
        } else {
          Stats->InvalidMmioReads++;
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint16,
                              0,
                              StreamOffset + 0x12,
                              1,
                              &StreamFormat
                              );
        if (!EFI_ERROR (Status) && (StreamFormat != 0xFFFF)) {
          Stats->FirstOutputStreamFormat = StreamFormat;
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
        } else {
          Stats->InvalidMmioReads++;
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint32,
                              0,
                              StreamOffset + 0x18,
                              1,
                              &StreamBdpl
                              );
        if (!EFI_ERROR (Status) && (StreamBdpl != 0xFFFFFFFF)) {
          Stats->FirstOutputStreamBdpl = StreamBdpl;
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
        } else {
          Stats->InvalidMmioReads++;
        }

        Status = PciIo->Mem.Read (
                              PciIo,
                              EfiPciIoWidthUint32,
                              0,
                              StreamOffset + 0x1C,
                              1,
                              &StreamBdpu
                              );
        if (!EFI_ERROR (Status) && (StreamBdpu != 0xFFFFFFFF)) {
          Stats->FirstOutputStreamBdpu = StreamBdpu;
          Stats->MmioReads++;
          Stats->StreamCapabilityEvidence++;
        } else {
          Stats->InvalidMmioReads++;
        }
      }

      ProbeHdaCodecTopology (PciIo, Stats);
      ProgramHdaDmaProof (PciIo, SystemTable, Stats);
    }
  }
  }

  SystemTable->BootServices->FreePool (Handles);
  return (Stats->Controllers != 0) ? EFI_SUCCESS : EFI_NOT_FOUND;
}

STATIC EFI_STATUS ProbeKeyboardNavigation (
  EFI_SYSTEM_TABLE *SystemTable,
  OUT BOOLEAN       *KeyboardPassed,
  OUT BOOLEAN       *NavigationPassed
  )
{
  EFI_STATUS Status;
  EFI_INPUT_KEY Key;
  UINTN Tick;
  BOOLEAN SawAny;
  BOOLEAN SawDown;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) ||
      (SystemTable->ConIn == NULL) || (KeyboardPassed == NULL) ||
      (NavigationPassed == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  *KeyboardPassed = FALSE;
  *NavigationPassed = FALSE;
  SawAny = FALSE;
  SawDown = FALSE;

  SystemTable->ConIn->Reset (SystemTable->ConIn, FALSE);
  if (SystemTable->ConOut != NULL) {
    SystemTable->ConOut->OutputString (
                           SystemTable->ConOut,
                           L"INPUT TEST: press DOWN ARROW then ENTER within 30 seconds.\r\n"
                           );
  }
  WriteText ("OMNI_KEYBOARD_WAIT\n");

  for (Tick = 0; Tick < 300; ++Tick) {
    Key.ScanCode = 0;
    Key.UnicodeChar = 0;
    Status = SystemTable->ConIn->ReadKeyStroke (SystemTable->ConIn, &Key);
    if (Status == EFI_NOT_READY) {
      SystemTable->BootServices->Stall (100000);
      continue;
    }
    if (EFI_ERROR (Status)) {
      WriteText ("OMNI_KEYBOARD_READ_ERROR\n");
      return Status;
    }

    SawAny = TRUE;
    *KeyboardPassed = TRUE;
    WriteStat ("OMNI_KEY_SCAN", Key.ScanCode);
    WriteStat ("OMNI_KEY_UNICODE", Key.UnicodeChar);

    if (Key.ScanCode == SCAN_DOWN) {
      SawDown = TRUE;
      if (SystemTable->ConOut != NULL) {
        SystemTable->ConOut->OutputString (
                               SystemTable->ConOut,
                               L"DOWN received. Press ENTER.\r\n"
                               );
      }
      WriteText ("OMNI_NAVIGATION_DOWN_RECEIVED\n");
      continue;
    }

    if (SawDown && (Key.UnicodeChar == CHAR_CARRIAGE_RETURN)) {
      *NavigationPassed = TRUE;
      WriteText ("OMNI_KEYBOARD_PASS\n");
      WriteText ("OMNI_NAVIGATION_INPUT_PASS\n");
      if (SystemTable->ConOut != NULL) {
        SystemTable->ConOut->OutputString (
                               SystemTable->ConOut,
                               L"KEYBOARD: PASS\r\nNAVIGATION INPUT: PASS\r\n"
                               );
      }
      return EFI_SUCCESS;
    }
  }

  if (SawAny) {
    WriteText ("OMNI_KEYBOARD_PASS\n");
    WriteText ("OMNI_NAVIGATION_INPUT_INCOMPLETE\n");
    return EFI_TIMEOUT;
  }

  WriteText ("OMNI_KEYBOARD_TIMEOUT\n");
  return EFI_TIMEOUT;
}

STATIC EFI_STATUS SaveDiag (
  EFI_HANDLE           ImageHandle,
  EFI_SYSTEM_TABLE     *SystemTable,
  CONST CHAR8          *Challenge,
  CONST OMNI_GOP_STATS *Gop,
  EFI_STATUS           GopStatus,
  CONST OMNI_HDA_STATS *Hda,
  EFI_STATUS           HdaStatus,
  EFI_STATUS           KeyboardStatus,
  BOOLEAN              KeyboardPassed,
  BOOLEAN              NavigationPassed
  )
{
  EFI_STATUS Status;
  EFI_LOADED_IMAGE_PROTOCOL *LoadedImage;
  EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *FileSystem;
  EFI_FILE_PROTOCOL *Root;
  EFI_FILE_PROTOCOL *File;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) ||
      (Challenge == NULL) || (Gop == NULL) || (Hda == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  LoadedImage = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      ImageHandle,
                                      &gEfiLoadedImageProtocolGuid,
                                      (VOID **)&LoadedImage
                                      );
  if (EFI_ERROR (Status) || (LoadedImage == NULL)) return EFI_NOT_FOUND;

  FileSystem = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      LoadedImage->DeviceHandle,
                                      &gEfiSimpleFileSystemProtocolGuid,
                                      (VOID **)&FileSystem
                                      );
  if (EFI_ERROR (Status) || (FileSystem == NULL)) return EFI_NOT_FOUND;

  Root = NULL;
  Status = FileSystem->OpenVolume (FileSystem, &Root);
  if (EFI_ERROR (Status) || (Root == NULL)) return Status;

  File = NULL;
  Status = Root->Open (Root, &File, OMNI_DIAG_FILE, EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE, 0);
  if (!EFI_ERROR (Status) && (File != NULL)) {
    Status = File->Delete (File);
    File = NULL;
    if (EFI_ERROR (Status)) { Root->Close (Root); return Status; }
  }

  Status = Root->Open (
                   Root,
                   &File,
                   OMNI_DIAG_FILE,
                   EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE | EFI_FILE_MODE_CREATE,
                   0
                   );
  if (EFI_ERROR (Status) || (File == NULL)) { Root->Close (Root); return Status; }

  Status = FileWriteAscii (File, "OMNI_GOP_DIAG_V1\n");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, "OMNI_CHALLENGE=");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, Challenge);
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, "\n");
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_STATUS", (UINTN)GopStatus);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_HANDLES", Gop->Handles);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_MODES", Gop->Modes);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_QUERY_PASS", Gop->QueryPass);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_SET_PASS", Gop->SetPass);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_BLT_PASS", Gop->BltPass);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_WIDTH", Gop->Width);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_HEIGHT", Gop->Height);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_GOP_PIXEL_FORMAT", Gop->PixelFormat);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_STATUS", (UINTN)HdaStatus);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PCI_HANDLES", Hda->PciHandles);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CONTROLLERS", Hda->Controllers);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_GPU_HDMI_SKIPPED", Hda->GpuHdmiControllersSkipped);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_SELECTION_PASS", Hda->SelectionPass);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_MMIO_READS", Hda->MmioReads);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CODEC_BITMAP", Hda->CodecBitmap);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_SEGMENT", Hda->Segment);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_BUS", Hda->Bus);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DEVICE", Hda->Device);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FUNCTION", Hda->Function);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_VENDOR_ID", Hda->VendorId);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DEVICE_ID", Hda->DeviceId);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_GCAP", Hda->Gcap);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_GCTL", Hda->Gctl);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_VMAJ", Hda->Vmaj);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_VMIN", Hda->Vmin);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_IMMEDIATE_COMMANDS", Hda->ImmediateCommands);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CODEC_ADDRESS", Hda->CodecAddress);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CODEC_VENDOR_ID", Hda->CodecVendorId);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROOT_START_NODE", Hda->RootStartNode);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROOT_NODE_COUNT", Hda->RootNodeCount);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_AUDIO_FUNCTION_GROUP", Hda->AudioFunctionGroup);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_WIDGET_START_NODE", Hda->WidgetStartNode);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_WIDGET_COUNT", Hda->WidgetCount);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_OUTPUT_CONVERTERS", Hda->OutputConverters);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_WIDGETS", Hda->PinWidgets);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CONVERTER_NODE", Hda->ConverterNode);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CONVERTER_WIDGET_CAPS", Hda->ConverterWidgetCaps);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CONVERTER_PCM_CAPS", Hda->ConverterPcmCaps);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CONVERTER_STREAM_FORMATS", Hda->ConverterStreamFormats);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_NODE", Hda->PinNode);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_WIDGET_CAPS", Hda->PinWidgetCaps);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_CAPABILITIES", Hda->PinCapabilities);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_CONFIG_DEFAULT", Hda->PinConfigDefault);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_CONTROL", Hda->PinControl);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_EAPD", Hda->PinEapd);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_CONNECTION_LIST_LENGTH", Hda->PinConnectionListLength);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_FIRST_CONNECTION", Hda->PinFirstConnection);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_RESOLVED", Hda->RouteResolved);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_RESOLVED_DEPTH", Hda->RouteResolvedDepth);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_INTERMEDIATE_NODE", Hda->RouteIntermediateNode);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_INTERMEDIATE_CONNECTION_INDEX", Hda->RouteIntermediateConnectionIndex);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_EVIDENCE", Hda->RouteEvidence);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_DEFAULT_DEVICE", Hda->PinDefaultDevice);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_PORT_CONNECTIVITY", Hda->PinPortConnectivity);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_SELECTION_SCORE", Hda->PinSelectionScore);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ANALOG_PIN_CANDIDATES", Hda->AnalogPinCandidates);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_INPUT_STREAMS", Hda->InputStreams);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_OUTPUT_STREAMS", Hda->OutputStreams);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_BIDIR_STREAMS", Hda->BidirStreams);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA64", Hda->Dma64Bit);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_OFFSET", Hda->FirstOutputStreamOffset);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_CTL", Hda->FirstOutputStreamCtl);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_STATUS", Hda->FirstOutputStreamStatus);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_LPIB", Hda->FirstOutputStreamLpib);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_CBL", Hda->FirstOutputStreamCbl);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_LVI", Hda->FirstOutputStreamLvi);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_FORMAT", Hda->FirstOutputStreamFormat);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_BDPL", Hda->FirstOutputStreamBdpl);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_FIRST_OUTPUT_STREAM_BDPU", Hda->FirstOutputStreamBdpu);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_STREAM_CAPABILITY_EVIDENCE", Hda->StreamCapabilityEvidence);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_PROGRAM_ATTEMPTED", Hda->DmaProgramAttempted);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_PROGRAMMED", Hda->DmaProgrammed);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_RUN_OBSERVED", Hda->DmaRunObserved);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_LPIB_BEFORE", Hda->DmaLpibBefore);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_LPIB_AFTER", Hda->DmaLpibAfter);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_PROGRESS", Hda->DmaProgress);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_FORMAT", Hda->DmaFormat);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_STREAM_TAG", Hda->DmaStreamTag);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_BUFFER_BYTES", Hda->DmaBufferBytes);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_STATUS", Hda->DmaStatus);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_TONE_PREPARED", Hda->DmaTonePrepared);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_TONE_FRAMES", Hda->DmaToneFrames);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_DMA_TONE_MILLISECONDS", Hda->DmaToneMilliseconds);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_PROGRAMMED", Hda->RouteProgrammed);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_PIN_CONTROL_AFTER", Hda->RoutePinControlAfter);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_EAPD_CAPABLE", Hda->RouteEapdCapable);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_EAPD_AFTER", Hda->RouteEapdAfter);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_ROUTE_INTERMEDIATE_AMP_PROGRAMMED", Hda->RouteIntermediateAmpProgrammed);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CONVERTER_AMP_PROGRAMMED", Hda->ConverterAmpProgrammed);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PIN_AMP_PROGRAMMED", Hda->PinAmpProgrammed);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PCI_ATTRIBUTES_SUPPORTED", Hda->PciAttributesSupported);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PCI_ATTRIBUTES_ORIGINAL", Hda->PciAttributesOriginal);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PCI_ATTRIBUTES_AFTER", Hda->PciAttributesAfter);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PCI_COMMAND_BEFORE", Hda->PciCommandBefore);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_PCI_COMMAND_AFTER", Hda->PciCommandAfter);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_MMIO_VALID", Hda->MmioValid);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_CONTROLLER_RESET_READY", Hda->ControllerResetReady);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HDA_INVALID_MMIO_READS", Hda->InvalidMmioReads);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_KEYBOARD_STATUS", (UINTN)KeyboardStatus);
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, KeyboardPassed ? "OMNI_KEYBOARD_PASS\n" : "OMNI_KEYBOARD_UNPROVEN\n");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, NavigationPassed ? "OMNI_NAVIGATION_INPUT_PASS\n" : "OMNI_NAVIGATION_INPUT_UNPROVEN\n");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, "OMNI_DIAG_PASS\n");
  if (!EFI_ERROR (Status)) Status = File->Flush (File);

  File->Close (File);
  Root->Close (Root);
  return ApplyPersistenceBarrier (ImageHandle, SystemTable, Status);
}

STATIC VOID ShowPhysicalScreen (
  EFI_SYSTEM_TABLE     *SystemTable,
  CONST OMNI_GOP_STATS *Gop,
  CONST OMNI_HDA_STATS *Hda,
  BOOLEAN              Passed
  )
{
  if ((SystemTable == NULL) || (SystemTable->ConOut == NULL)) return;

  SystemTable->ConOut->SetAttribute (SystemTable->ConOut, EFI_TEXT_ATTR (EFI_LIGHTGRAY, EFI_BLACK));
  SystemTable->ConOut->SetCursorPosition (SystemTable->ConOut, 0, 0);
  SystemTable->ConOut->OutputString (SystemTable->ConOut, L"OMNI UEFI PHYSICAL PROBE\r\n");
  SystemTable->ConOut->OutputString (SystemTable->ConOut, Passed ? L"CORE EVIDENCE: PASS\r\n" : L"CORE EVIDENCE: FAIL\r\n");
  SystemTable->ConOut->OutputString (SystemTable->ConOut, (Gop->BltPass != 0) ? L"GOP BLT: PASS\r\n" : L"GOP BLT: NOT PROVEN\r\n");
  SystemTable->ConOut->OutputString (SystemTable->ConOut, (Hda->Controllers != 0) ? L"HDA CONTROLLER: FOUND\r\n" : L"HDA CONTROLLER: NOT FOUND\r\n");
  SystemTable->ConOut->OutputString (SystemTable->ConOut, L"Physical input validation follows.\r\n");
  SystemTable->ConOut->OutputString (SystemTable->ConOut, L"Press DOWN ARROW then ENTER.\r\n");
}

STATIC EFI_STATUS SaveEvidence (
  EFI_HANDLE          ImageHandle,
  EFI_SYSTEM_TABLE    *SystemTable,
  CONST OMNI_HII_STATS *Stats,
  CONST CHAR8          *Challenge,
  CONST CHAR8          *PlatformUuid,
  BOOLEAN              HiiPassed,
  BOOLEAN              Passed
  )
{
  EFI_STATUS Status;
  EFI_LOADED_IMAGE_PROTOCOL *LoadedImage;
  EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *FileSystem;
  EFI_FILE_PROTOCOL *Root;
  EFI_FILE_PROTOCOL *File;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) ||
      (Stats == NULL) || (Challenge == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  LoadedImage = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      ImageHandle,
                                      &gEfiLoadedImageProtocolGuid,
                                      (VOID **)&LoadedImage
                                      );
  if (EFI_ERROR (Status) || (LoadedImage == NULL)) {
    return EFI_NOT_FOUND;
  }

  FileSystem = NULL;
  Status = SystemTable->BootServices->HandleProtocol (
                                      LoadedImage->DeviceHandle,
                                      &gEfiSimpleFileSystemProtocolGuid,
                                      (VOID **)&FileSystem
                                      );
  if (EFI_ERROR (Status) || (FileSystem == NULL)) {
    return EFI_NOT_FOUND;
  }

  Root = NULL;
  Status = FileSystem->OpenVolume (FileSystem, &Root);
  if (EFI_ERROR (Status) || (Root == NULL)) {
    return Status;
  }

  File = NULL;
  Status = Root->Open (
                   Root,
                   &File,
                   OMNI_EVIDENCE_FILE,
                   EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE,
                   0
                   );
  if (!EFI_ERROR (Status) && (File != NULL)) {
    Status = File->Delete (File);
    File = NULL;
    if (EFI_ERROR (Status)) {
      Root->Close (Root);
      return Status;
    }
  }

  Status = Root->Open (
                   Root,
                   &File,
                   OMNI_EVIDENCE_FILE,
                   EFI_FILE_MODE_READ | EFI_FILE_MODE_WRITE | EFI_FILE_MODE_CREATE,
                   0
                   );
  if (EFI_ERROR (Status) || (File == NULL)) {
    Root->Close (Root);
    return Status;
  }

  Status = FileWriteAscii (File, "OMNI_EVIDENCE_V2\n");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, "OMNI_CHALLENGE=");
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, Challenge);
  if (!EFI_ERROR (Status)) Status = FileWriteAscii (File, "\n");
  if (!EFI_ERROR (Status) && (PlatformUuid != NULL)) {
    Status = FileWriteAscii (File, "OMNI_PLATFORM_UUID=");
  }
  if (!EFI_ERROR (Status) && (PlatformUuid != NULL)) {
    Status = FileWriteAscii (File, PlatformUuid);
  }
  if (!EFI_ERROR (Status) && (PlatformUuid != NULL)) {
    Status = FileWriteAscii (File, "\n");
  }
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HII_HANDLES", Stats->Handles);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HII_FORM_PACKAGES", Stats->FormPackages);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HII_OPCODES", Stats->Opcodes);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HII_QUESTIONS", Stats->Questions);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HII_PASSWORDS", Stats->Passwords);
  if (!EFI_ERROR (Status)) Status = FileWriteStat (File, "OMNI_HII_INVALID", Stats->InvalidPackages);
  if (!EFI_ERROR (Status)) {
    Status = FileWriteAscii (File, HiiPassed ? "OMNI_HII_PASS\n" : "OMNI_HII_FAIL\n");
  }
  if (!EFI_ERROR (Status)) {
    Status = FileWriteAscii (File, Passed ? "OMNI_UEFI_PASS\n" : "OMNI_UEFI_FAIL\n");
  }
  if (!EFI_ERROR (Status)) {
    Status = File->Flush (File);
  }

  File->Close (File);
  Root->Close (Root);
  return ApplyPersistenceBarrier (ImageHandle, SystemTable, Status);
}

STATIC BOOLEAN IsQuestionOpcode (UINT8 OpCode) {
  switch (OpCode) {
    case EFI_IFR_ONE_OF_OP:
    case EFI_IFR_CHECKBOX_OP:
    case EFI_IFR_NUMERIC_OP:
    case EFI_IFR_PASSWORD_OP:
    case EFI_IFR_ACTION_OP:
    case EFI_IFR_REF_OP:
    case EFI_IFR_DATE_OP:
    case EFI_IFR_TIME_OP:
    case EFI_IFR_STRING_OP:
    case EFI_IFR_ORDERED_LIST_OP:
      return TRUE;
    default:
      return FALSE;
  }
}

STATIC BOOLEAN ScanFormsPackage (
  CONST EFI_HII_PACKAGE_HEADER *Package,
  IN OUT OMNI_HII_STATS        *Stats
  )
{
  CONST UINT8 *Base;
  UINTN Offset;
  UINTN Depth;

  if ((Package == NULL) || (Stats == NULL) ||
      (Package->Length < sizeof (EFI_HII_PACKAGE_HEADER))) {
    return FALSE;
  }

  Base = (CONST UINT8 *)Package;
  Offset = sizeof (EFI_HII_PACKAGE_HEADER);
  Depth = 0;

  while (Offset < Package->Length) {
    CONST EFI_IFR_OP_HEADER *Op;

    if ((Package->Length - Offset) < sizeof (EFI_IFR_OP_HEADER)) {
      return FALSE;
    }

    Op = (CONST EFI_IFR_OP_HEADER *)(Base + Offset);
    if ((Op->Length < sizeof (EFI_IFR_OP_HEADER)) ||
        (Op->Length > (Package->Length - Offset))) {
      return FALSE;
    }

    Stats->Opcodes++;
    if (IsQuestionOpcode (Op->OpCode)) {
      Stats->Questions++;
    }
    if (Op->OpCode == EFI_IFR_PASSWORD_OP) {
      Stats->Passwords++;
    }

    if (Op->OpCode == EFI_IFR_END_OP) {
      if ((Op->Scope != 0) || (Depth == 0)) {
        return FALSE;
      }
      Depth--;
    } else if (Op->Scope != 0) {
      Depth++;
    }

    Offset += Op->Length;
  }

  return (Offset == Package->Length) && (Depth == 0);
}

STATIC BOOLEAN ScanPackageList (
  CONST EFI_HII_PACKAGE_LIST_HEADER *List,
  UINTN                             BufferSize,
  IN OUT OMNI_HII_STATS             *Stats
  )
{
  CONST UINT8 *Base;
  UINTN Offset;
  UINTN Declared;

  if ((List == NULL) || (Stats == NULL) ||
      (BufferSize < sizeof (EFI_HII_PACKAGE_LIST_HEADER))) {
    return FALSE;
  }

  Declared = List->PackageLength;
  if ((Declared < sizeof (EFI_HII_PACKAGE_LIST_HEADER)) ||
      (Declared > BufferSize)) {
    return FALSE;
  }

  Base = (CONST UINT8 *)List;
  Offset = sizeof (EFI_HII_PACKAGE_LIST_HEADER);

  while (Offset < Declared) {
    CONST EFI_HII_PACKAGE_HEADER *Package;

    if ((Declared - Offset) < sizeof (EFI_HII_PACKAGE_HEADER)) {
      return FALSE;
    }

    Package = (CONST EFI_HII_PACKAGE_HEADER *)(Base + Offset);
    if ((Package->Length < sizeof (EFI_HII_PACKAGE_HEADER)) ||
        (Package->Length > (Declared - Offset))) {
      return FALSE;
    }

    if (Package->Type == EFI_HII_PACKAGE_FORMS) {
      Stats->FormPackages++;
      if (!ScanFormsPackage (Package, Stats)) {
        return FALSE;
      }
    }

    Offset += Package->Length;
  }

  return Offset == Declared;
}

STATIC EFI_STATUS ProbeHii (
  EFI_SYSTEM_TABLE *SystemTable,
  OUT OMNI_HII_STATS *Stats
  )
{
  EFI_STATUS Status;
  EFI_HII_DATABASE_PROTOCOL *Database;
  EFI_HII_HANDLE *Handles;
  UINTN HandleBytes;
  UINTN Index;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL) || (Stats == NULL)) {
    return EFI_INVALID_PARAMETER;
  }

  Database = NULL;
  Status = SystemTable->BootServices->LocateProtocol (
                                      &gEfiHiiDatabaseProtocolGuid,
                                      NULL,
                                      (VOID **)&Database
                                      );
  if (EFI_ERROR (Status) || (Database == NULL)) {
    return EFI_NOT_FOUND;
  }

  Handles = NULL;
  HandleBytes = 0;
  Status = Database->ListPackageLists (
                       Database,
                       EFI_HII_PACKAGE_FORMS,
                       NULL,
                       &HandleBytes,
                       NULL
                       );
  if ((Status != EFI_BUFFER_TOO_SMALL) || (HandleBytes == 0)) {
    return Status;
  }

  Status = SystemTable->BootServices->AllocatePool (
                                      EfiBootServicesData,
                                      HandleBytes,
                                      (VOID **)&Handles
                                      );
  if (EFI_ERROR (Status) || (Handles == NULL)) {
    return EFI_OUT_OF_RESOURCES;
  }

  Status = Database->ListPackageLists (
                       Database,
                       EFI_HII_PACKAGE_FORMS,
                       NULL,
                       &HandleBytes,
                       Handles
                       );
  if (EFI_ERROR (Status)) {
    SystemTable->BootServices->FreePool (Handles);
    return Status;
  }

  Stats->Handles = HandleBytes / sizeof (EFI_HII_HANDLE);

  for (Index = 0; Index < Stats->Handles; ++Index) {
    EFI_HII_PACKAGE_LIST_HEADER *Buffer;
    UINTN BufferSize;

    Buffer = NULL;
    BufferSize = 0;
    Status = Database->ExportPackageLists (
                         Database,
                         Handles[Index],
                         &BufferSize,
                         NULL
                         );
    if ((Status != EFI_BUFFER_TOO_SMALL) && (Status != EFI_OUT_OF_RESOURCES)) {
      Stats->InvalidPackages++;
      continue;
    }
    if (BufferSize < sizeof (EFI_HII_PACKAGE_LIST_HEADER)) {
      Stats->InvalidPackages++;
      continue;
    }

    Status = SystemTable->BootServices->AllocatePool (
                                        EfiBootServicesData,
                                        BufferSize,
                                        (VOID **)&Buffer
                                        );
    if (EFI_ERROR (Status) || (Buffer == NULL)) {
      SystemTable->BootServices->FreePool (Handles);
      return EFI_OUT_OF_RESOURCES;
    }

    Status = Database->ExportPackageLists (
                         Database,
                         Handles[Index],
                         &BufferSize,
                         Buffer
                         );
    if (EFI_ERROR (Status) || !ScanPackageList (Buffer, BufferSize, Stats)) {
      Stats->InvalidPackages++;
    }

    SystemTable->BootServices->FreePool (Buffer);
  }

  SystemTable->BootServices->FreePool (Handles);
  return EFI_SUCCESS;
}

/*
 * Chain-load the UEFI screen reader stored on the boot media at
 * \EFI\OMNI\SCREENREADER.EFI. The image is read into memory (no DevicePathLib
 * dependency) and loaded from that buffer; its LoadedImage DeviceHandle is set to
 * this key so the reader can persist its own evidence next to OmniProbe's.
 * Returns EFI_NOT_FOUND when the reader is not on the media.
 */
STATIC EFI_STATUS StartScreenReader (
  EFI_HANDLE       ImageHandle,
  EFI_SYSTEM_TABLE *SystemTable
  )
{
  EFI_STATUS Status;
  EFI_LOADED_IMAGE_PROTOCOL *LoadedImage;
  EFI_LOADED_IMAGE_PROTOCOL *ChildImage;
  EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *FileSystem;
  EFI_FILE_PROTOCOL *Root;
  EFI_FILE_PROTOCOL *File;
  EFI_HANDLE Child;
  VOID *Buffer;
  UINT64 FileSize;
  UINTN Size;

  if ((SystemTable == NULL) || (SystemTable->BootServices == NULL)) {
    return EFI_INVALID_PARAMETER;
  }
  LoadedImage = NULL;
  Status = SystemTable->BootServices->HandleProtocol (ImageHandle, &gEfiLoadedImageProtocolGuid, (VOID **)&LoadedImage);
  if (EFI_ERROR (Status) || (LoadedImage == NULL)) return EFI_NOT_FOUND;
  FileSystem = NULL;
  Status = SystemTable->BootServices->HandleProtocol (LoadedImage->DeviceHandle, &gEfiSimpleFileSystemProtocolGuid, (VOID **)&FileSystem);
  if (EFI_ERROR (Status) || (FileSystem == NULL)) return EFI_NOT_FOUND;
  Root = NULL;
  Status = FileSystem->OpenVolume (FileSystem, &Root);
  if (EFI_ERROR (Status) || (Root == NULL)) return EFI_NOT_FOUND;
  File = NULL;
  Status = Root->Open (Root, &File, OMNI_SCREENREADER_FILE, EFI_FILE_MODE_READ, 0);
  if (EFI_ERROR (Status) || (File == NULL)) {
    Root->Close (Root);
    return EFI_NOT_FOUND;
  }

  /* Size via end-of-file position: avoids an EFI_FILE_INFO/GUID dependency. */
  FileSize = 0;
  Status = File->SetPosition (File, 0xFFFFFFFFFFFFFFFFULL);
  if (!EFI_ERROR (Status)) Status = File->GetPosition (File, &FileSize);
  if (!EFI_ERROR (Status)) Status = File->SetPosition (File, 0);
  if (!EFI_ERROR (Status) && ((FileSize == 0) || (FileSize > OMNI_SCREENREADER_MAX_BYTES))) Status = EFI_BAD_BUFFER_SIZE;
  Buffer = NULL;
  if (!EFI_ERROR (Status)) Status = SystemTable->BootServices->AllocatePool (EfiLoaderData, (UINTN)FileSize, &Buffer);
  Size = (UINTN)FileSize;
  if (!EFI_ERROR (Status)) Status = File->Read (File, &Size, Buffer);
  if (!EFI_ERROR (Status) && (Size != (UINTN)FileSize)) Status = EFI_END_OF_FILE;
  File->Close (File);
  Root->Close (Root);

  Child = NULL;
  if (!EFI_ERROR (Status)) {
    WriteText ("OMNI_SCREENREADER_LOAD\n");
    Status = SystemTable->BootServices->LoadImage (FALSE, ImageHandle, NULL, Buffer, Size, &Child);
  }
  if (Buffer != NULL) SystemTable->BootServices->FreePool (Buffer);
  if (EFI_ERROR (Status)) return Status;

  ChildImage = NULL;
  Status = SystemTable->BootServices->HandleProtocol (Child, &gEfiLoadedImageProtocolGuid, (VOID **)&ChildImage);
  if (!EFI_ERROR (Status) && (ChildImage != NULL)) {
    ChildImage->DeviceHandle = LoadedImage->DeviceHandle;
  }
  SaveTraceStage (ImageHandle, SystemTable, "SCREENREADER_START");
  WriteText ("OMNI_SCREENREADER_START\n");
  if ((SystemTable->ConOut != NULL)) {
    SystemTable->ConOut->OutputString (SystemTable->ConOut, L"Starting UEFI screen reader...\r\n");
  }
  return SystemTable->BootServices->StartImage (Child, NULL, NULL);
}

EFI_STATUS
EFIAPI
UefiMain (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  EFI_STATUS HiiStatus;
  EFI_STATUS ChallengeStatus;
  EFI_STATUS PlatformStatus;
  EFI_STATUS EvidenceStatus;
  EFI_STATUS GopStatus;
  EFI_STATUS HdaStatus;
  EFI_STATUS KeyboardStatus;
  EFI_STATUS DiagStatus;
  EFI_STATUS NvramStatus;
  EFI_STATUS ScreenReaderStatus;
  BOOLEAN HiiPassed;
  BOOLEAN Passed;
  BOOLEAN KeyboardPassed;
  BOOLEAN NavigationPassed;
  CHAR8 Challenge[OMNI_CHALLENGE_HEX_LEN + 1] = {0};
  CHAR8 PlatformUuid[OMNI_UUID_TEXT_LEN + 1] = {0};
  OMNI_HII_STATS Stats = {0};
  OMNI_GOP_STATS Gop = {0};
  OMNI_HDA_STATS Hda = {0};

  KeyboardPassed = FALSE;
  NavigationPassed = FALSE;

  /*
   * First persistent proof: do this before SerialInit, GOP, HDA, HII and
   * filesystem access.  If FAT writes fail or an early hardware probe faults,
   * the firmware variable can still prove that UefiMain was entered.
   */
  NvramStatus = SaveNvramStage (SystemTable, "ENTRY", NULL);

  SerialInit ();
  WriteText ("OMNI_BOOT_OK\n");
  WriteText (EFI_ERROR (NvramStatus) ? "OMNI_NVRAM_ENTRY_FAIL\n" : "OMNI_NVRAM_ENTRY_PASS\n");
  SaveTraceStage (ImageHandle, SystemTable, "BOOT_START");

  ChallengeStatus = LoadChallenge (ImageHandle, SystemTable, Challenge);
  if (EFI_ERROR (ChallengeStatus)) {
    WriteText ("OMNI_CHALLENGE_FAIL\n");
  } else {
    WriteText ("OMNI_CHALLENGE_PASS\n");
    WriteText ("OMNI_CHALLENGE=");
    WriteText (Challenge);
    WriteText ("\n");

    NvramStatus = SaveNvramStage (SystemTable, "CHALLENGE_BOUND", Challenge);
    WriteText (EFI_ERROR (NvramStatus) ? "OMNI_NVRAM_BIND_FAIL\n" : "OMNI_NVRAM_BIND_PASS\n");
  }

  PlatformStatus = LoadPlatformUuid (SystemTable, PlatformUuid);
  if (EFI_ERROR (PlatformStatus)) {
    WriteText ("OMNI_PLATFORM_UUID_UNAVAILABLE\n");
  } else {
    WriteText ("OMNI_PLATFORM_UUID=");
    WriteText (PlatformUuid);
    WriteText ("\n");
  }

  HiiStatus = ProbeHii (SystemTable, &Stats);
  WriteStat ("OMNI_HII_HANDLES", Stats.Handles);
  WriteStat ("OMNI_HII_FORM_PACKAGES", Stats.FormPackages);
  WriteStat ("OMNI_HII_OPCODES", Stats.Opcodes);
  WriteStat ("OMNI_HII_QUESTIONS", Stats.Questions);
  WriteStat ("OMNI_HII_PASSWORDS", Stats.Passwords);
  WriteStat ("OMNI_HII_INVALID", Stats.InvalidPackages);

  HiiPassed = (BOOLEAN)(
    !EFI_ERROR (HiiStatus) &&
    (Stats.FormPackages != 0) &&
    (Stats.InvalidPackages == 0)
    );
  Passed = (BOOLEAN)(!EFI_ERROR (ChallengeStatus) && HiiPassed);

  if (HiiPassed) {
    WriteText ("OMNI_HII_PASS\n");
  } else {
    WriteText ("OMNI_HII_FAIL\n");
  }

  EvidenceStatus = SaveEvidence (
                     ImageHandle,
                     SystemTable,
                     &Stats,
                     EFI_ERROR (ChallengeStatus) ? "INVALID" : Challenge,
                     EFI_ERROR (PlatformStatus) ? NULL : PlatformUuid,
                     HiiPassed,
                     Passed
                     );
  if (EFI_ERROR (EvidenceStatus)) {
    WriteText ("OMNI_EVIDENCE_FAIL\n");
    Passed = FALSE;
  } else {
    WriteText ("OMNI_EVIDENCE_PASS\n");
  }

  WriteText (Passed ? "OMNI_UEFI_PASS\n" : "OMNI_UEFI_FAIL\n");

  SaveTraceStage (ImageHandle, SystemTable, "CORE_EVIDENCE_SAVED");
  if (!EFI_ERROR (ChallengeStatus)) {
    NvramStatus = SaveNvramStage (SystemTable, "CORE_EVIDENCE_SAVED", Challenge);
    WriteText (EFI_ERROR (NvramStatus) ? "OMNI_NVRAM_CORE_FAIL\n" : "OMNI_NVRAM_CORE_PASS\n");
  }

  SaveTraceStage (ImageHandle, SystemTable, "BEFORE_GOP");
  GopStatus = ProbeGop (SystemTable, &Gop);
  WriteStat ("OMNI_GOP_HANDLES", Gop.Handles);
  WriteStat ("OMNI_GOP_MODES", Gop.Modes);
  WriteStat ("OMNI_GOP_QUERY_PASS", Gop.QueryPass);
  WriteStat ("OMNI_GOP_SET_PASS", Gop.SetPass);
  WriteStat ("OMNI_GOP_BLT_PASS", Gop.BltPass);
  WriteStat ("OMNI_GOP_WIDTH", Gop.Width);
  WriteStat ("OMNI_GOP_HEIGHT", Gop.Height);
  WriteText (EFI_ERROR (GopStatus) ? "OMNI_GOP_PROBE_FAIL\n" : "OMNI_GOP_PROBE_PASS\n");
  SaveTraceStage (ImageHandle, SystemTable, "AFTER_GOP");

  SaveTraceStage (ImageHandle, SystemTable, "BEFORE_HDA");
  HdaStatus = ProbeHda (SystemTable, &Hda);
  WriteStat ("OMNI_HDA_PCI_HANDLES", Hda.PciHandles);
  WriteStat ("OMNI_HDA_CONTROLLERS", Hda.Controllers);
  WriteStat ("OMNI_HDA_GPU_HDMI_SKIPPED", Hda.GpuHdmiControllersSkipped);
  WriteStat ("OMNI_HDA_SELECTION_PASS", Hda.SelectionPass);
  WriteStat ("OMNI_HDA_MMIO_READS", Hda.MmioReads);
  WriteStat ("OMNI_HDA_CODEC_BITMAP", Hda.CodecBitmap);
  WriteStat ("OMNI_HDA_VENDOR_ID", Hda.VendorId);
  WriteStat ("OMNI_HDA_DEVICE_ID", Hda.DeviceId);
  WriteStat ("OMNI_HDA_IMMEDIATE_COMMANDS", Hda.ImmediateCommands);
  WriteStat ("OMNI_HDA_CODEC_ADDRESS", Hda.CodecAddress);
  WriteStat ("OMNI_HDA_CODEC_VENDOR_ID", Hda.CodecVendorId);
  WriteStat ("OMNI_HDA_AUDIO_FUNCTION_GROUP", Hda.AudioFunctionGroup);
  WriteStat ("OMNI_HDA_WIDGET_START_NODE", Hda.WidgetStartNode);
  WriteStat ("OMNI_HDA_WIDGET_COUNT", Hda.WidgetCount);
  WriteStat ("OMNI_HDA_OUTPUT_CONVERTERS", Hda.OutputConverters);
  WriteStat ("OMNI_HDA_PIN_WIDGETS", Hda.PinWidgets);
  WriteStat ("OMNI_HDA_CONVERTER_NODE", Hda.ConverterNode);
  WriteStat ("OMNI_HDA_CONVERTER_PCM_CAPS", Hda.ConverterPcmCaps);
  WriteStat ("OMNI_HDA_CONVERTER_STREAM_FORMATS", Hda.ConverterStreamFormats);
  WriteStat ("OMNI_HDA_PIN_NODE", Hda.PinNode);
  WriteStat ("OMNI_HDA_PIN_CAPABILITIES", Hda.PinCapabilities);
  WriteStat ("OMNI_HDA_PIN_CONFIG_DEFAULT", Hda.PinConfigDefault);
  WriteStat ("OMNI_HDA_PIN_CONTROL", Hda.PinControl);
  WriteStat ("OMNI_HDA_PIN_EAPD", Hda.PinEapd);
  WriteStat ("OMNI_HDA_PIN_CONNECTION_LIST_LENGTH", Hda.PinConnectionListLength);
  WriteStat ("OMNI_HDA_PIN_FIRST_CONNECTION", Hda.PinFirstConnection);
  WriteStat ("OMNI_HDA_ROUTE_RESOLVED", Hda.RouteResolved);
  WriteStat ("OMNI_HDA_ROUTE_RESOLVED_DEPTH", Hda.RouteResolvedDepth);
  WriteStat ("OMNI_HDA_ROUTE_INTERMEDIATE_NODE", Hda.RouteIntermediateNode);
  WriteStat ("OMNI_HDA_ROUTE_INTERMEDIATE_CONNECTION_INDEX", Hda.RouteIntermediateConnectionIndex);
  WriteStat ("OMNI_HDA_ROUTE_EVIDENCE", Hda.RouteEvidence);
  WriteStat ("OMNI_HDA_PIN_DEFAULT_DEVICE", Hda.PinDefaultDevice);
  WriteStat ("OMNI_HDA_PIN_PORT_CONNECTIVITY", Hda.PinPortConnectivity);
  WriteStat ("OMNI_HDA_PIN_SELECTION_SCORE", Hda.PinSelectionScore);
  WriteStat ("OMNI_HDA_ANALOG_PIN_CANDIDATES", Hda.AnalogPinCandidates);
  WriteStat ("OMNI_HDA_INPUT_STREAMS", Hda.InputStreams);
  WriteStat ("OMNI_HDA_OUTPUT_STREAMS", Hda.OutputStreams);
  WriteStat ("OMNI_HDA_BIDIR_STREAMS", Hda.BidirStreams);
  WriteStat ("OMNI_HDA_DMA64", Hda.Dma64Bit);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_OFFSET", Hda.FirstOutputStreamOffset);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_CTL", Hda.FirstOutputStreamCtl);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_STATUS", Hda.FirstOutputStreamStatus);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_LPIB", Hda.FirstOutputStreamLpib);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_CBL", Hda.FirstOutputStreamCbl);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_LVI", Hda.FirstOutputStreamLvi);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_FORMAT", Hda.FirstOutputStreamFormat);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_BDPL", Hda.FirstOutputStreamBdpl);
  WriteStat ("OMNI_HDA_FIRST_OUTPUT_STREAM_BDPU", Hda.FirstOutputStreamBdpu);
  WriteStat ("OMNI_HDA_STREAM_CAPABILITY_EVIDENCE", Hda.StreamCapabilityEvidence);
  WriteStat ("OMNI_HDA_DMA_PROGRAM_ATTEMPTED", Hda.DmaProgramAttempted);
  WriteStat ("OMNI_HDA_DMA_PROGRAMMED", Hda.DmaProgrammed);
  WriteStat ("OMNI_HDA_DMA_RUN_OBSERVED", Hda.DmaRunObserved);
  WriteStat ("OMNI_HDA_DMA_LPIB_BEFORE", Hda.DmaLpibBefore);
  WriteStat ("OMNI_HDA_DMA_LPIB_AFTER", Hda.DmaLpibAfter);
  WriteStat ("OMNI_HDA_DMA_PROGRESS", Hda.DmaProgress);
  WriteStat ("OMNI_HDA_DMA_FORMAT", Hda.DmaFormat);
  WriteStat ("OMNI_HDA_DMA_STREAM_TAG", Hda.DmaStreamTag);
  WriteStat ("OMNI_HDA_DMA_BUFFER_BYTES", Hda.DmaBufferBytes);
  WriteStat ("OMNI_HDA_DMA_STATUS", Hda.DmaStatus);
  WriteStat ("OMNI_HDA_ROUTE_PROGRAMMED", Hda.RouteProgrammed);
  WriteStat ("OMNI_HDA_ROUTE_INTERMEDIATE_AMP_PROGRAMMED", Hda.RouteIntermediateAmpProgrammed);
  WriteText ((Hda.DmaProgrammed != 0) ? "OMNI_HDA_DMA_PROGRAM_PASS\n" : "OMNI_HDA_DMA_PROGRAM_MISS\n");
  WriteText ((Hda.DmaRunObserved != 0) ? "OMNI_HDA_DMA_RUN_PASS\n" : "OMNI_HDA_DMA_RUN_MISS\n");
  WriteText ((Hda.DmaProgress != 0) ? "OMNI_HDA_DMA_LPIB_PROGRESS_PASS\n" : "OMNI_HDA_DMA_LPIB_PROGRESS_MISS\n");
  WriteStat ("OMNI_HDA_PCI_ATTRIBUTES_SUPPORTED", Hda.PciAttributesSupported);
  WriteStat ("OMNI_HDA_PCI_ATTRIBUTES_ORIGINAL", Hda.PciAttributesOriginal);
  WriteStat ("OMNI_HDA_PCI_ATTRIBUTES_AFTER", Hda.PciAttributesAfter);
  WriteStat ("OMNI_HDA_PCI_COMMAND_BEFORE", Hda.PciCommandBefore);
  WriteStat ("OMNI_HDA_PCI_COMMAND_AFTER", Hda.PciCommandAfter);
  WriteStat ("OMNI_HDA_MMIO_VALID", Hda.MmioValid);
  WriteStat ("OMNI_HDA_CONTROLLER_RESET_READY", Hda.ControllerResetReady);
  WriteStat ("OMNI_HDA_INVALID_MMIO_READS", Hda.InvalidMmioReads);
  WriteText ((Hda.RouteEvidence != 0) ? "OMNI_HDA_ROUTE_CAPS_PASS\n" : "OMNI_HDA_ROUTE_CAPS_MISS\n");
  WriteText (((Hda.OutputStreams != 0) && (Hda.StreamCapabilityEvidence != 0)) ?
             "OMNI_HDA_STREAM_CAPS_PASS\n" : "OMNI_HDA_STREAM_CAPS_MISS\n");
  WriteText (EFI_ERROR (HdaStatus) ? "OMNI_HDA_PROBE_MISS\n" : "OMNI_HDA_PROBE_PASS\n");
  SaveTraceStage (ImageHandle, SystemTable, "AFTER_HDA");

  ShowPhysicalScreen (SystemTable, &Gop, &Hda, Passed);
  SaveTraceStage (ImageHandle, SystemTable, "BEFORE_INPUT");
  KeyboardStatus = ProbeKeyboardNavigation (
                     SystemTable,
                     &KeyboardPassed,
                     &NavigationPassed
                     );
  SaveTraceStage (ImageHandle, SystemTable, "AFTER_INPUT");

  DiagStatus = SaveDiag (
                 ImageHandle,
                 SystemTable,
                 EFI_ERROR (ChallengeStatus) ? "INVALID" : Challenge,
                 &Gop,
                 GopStatus,
                 &Hda,
                 HdaStatus,
                 KeyboardStatus,
                 KeyboardPassed,
                 NavigationPassed
                 );
  WriteText (EFI_ERROR (DiagStatus) ? "OMNI_DIAG_FAIL\n" : "OMNI_DIAG_PASS\n");
  SaveTraceStage (
    ImageHandle,
    SystemTable,
    EFI_ERROR (DiagStatus) ? "DIAG_WRITE_FAIL" : "COMPLETE"
    );

  if (!EFI_ERROR (ChallengeStatus)) {
    NvramStatus = SaveNvramStage (
                    SystemTable,
                    EFI_ERROR (DiagStatus) ? "DIAG_WRITE_FAIL" : "COMPLETE",
                    Challenge
                    );
    WriteText (EFI_ERROR (NvramStatus) ? "OMNI_NVRAM_FINAL_FAIL\n" : "OMNI_NVRAM_FINAL_PASS\n");
  }

  if ((SystemTable != NULL) && (SystemTable->ConOut != NULL)) {
    SystemTable->ConOut->OutputString (
                           SystemTable->ConOut,
                           KeyboardPassed ? L"KEYBOARD EVIDENCE: PASS\r\n" : L"KEYBOARD EVIDENCE: UNPROVEN\r\n"
                           );
    SystemTable->ConOut->OutputString (
                           SystemTable->ConOut,
                           NavigationPassed ? L"NAVIGATION INPUT: PASS\r\n" : L"NAVIGATION INPUT: UNPROVEN\r\n"
                           );
  }

  /*
   * Evidence is persisted; hand the machine to the UEFI screen reader shipped on
   * the same key. Absent reader keeps the previous behaviour (return to firmware).
   */
  ScreenReaderStatus = StartScreenReader (ImageHandle, SystemTable);
  if (ScreenReaderStatus == EFI_NOT_FOUND) {
    WriteText ("OMNI_SCREENREADER_ABSENT\n");
  } else {
    WriteText (EFI_ERROR (ScreenReaderStatus) ? "OMNI_SCREENREADER_FAIL\n" : "OMNI_SCREENREADER_RETURNED\n");
    SaveTraceStage (
      ImageHandle,
      SystemTable,
      EFI_ERROR (ScreenReaderStatus) ? "SCREENREADER_FAIL" : "SCREENREADER_RETURNED"
      );
  }

  if ((SystemTable != NULL) && (SystemTable->ConOut != NULL)) {
    SystemTable->ConOut->OutputString (SystemTable->ConOut, L"Returning to firmware in 3 seconds...\r\n");
  }
  if ((SystemTable != NULL) && (SystemTable->BootServices != NULL)) {
    SystemTable->BootServices->Stall (3000000);
  }

  WriteText ("OMNI_RETURN_TO_FIRMWARE\n");
  return Passed ? EFI_SUCCESS : EFI_DEVICE_ERROR;
}
