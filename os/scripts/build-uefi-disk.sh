#!/usr/bin/env bash
set -euo pipefail

EFI_BINARY="${1:-boot/uefi/target/x86_64-unknown-uefi/release/aw-uefi-boot.efi}"
KERNEL_BINARY="${2:-kernel/x86_64/target/x86_64-unknown-none/release/aw-kernel-x86_64.bin}"
OUTPUT_IMAGE="${3:-build/accessible-windows-uefi-x86_64.img}"
IMAGE_SIZE_MIB="${IMAGE_SIZE_MIB:-64}"
MOUNT_DIR="$(mktemp -d)"
LOOP_DEVICE=""
MOUNTED=0

cleanup() {
  set +e
  if [ "$MOUNTED" -eq 1 ]; then
    sudo umount "$MOUNT_DIR"
  fi
  if [ -n "$LOOP_DEVICE" ]; then
    sudo losetup -d "$LOOP_DEVICE"
  fi
  rmdir "$MOUNT_DIR" 2>/dev/null || true
}
trap cleanup EXIT

if [ ! -f "$EFI_BINARY" ]; then
  echo "EFI binary not found: $EFI_BINARY" >&2
  exit 1
fi

if [ ! -f "$KERNEL_BINARY" ]; then
  echo "Native kernel binary not found: $KERNEL_BINARY" >&2
  exit 1
fi

for tool in sgdisk losetup mkfs.vfat mount cmp; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Required tool not found: $tool" >&2
    exit 1
  fi
done

if [ "$IMAGE_SIZE_MIB" -lt 8 ]; then
  echo "IMAGE_SIZE_MIB must be at least 8" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT_IMAGE")"
rm -f "$OUTPUT_IMAGE"
truncate -s "${IMAGE_SIZE_MIB}M" "$OUTPUT_IMAGE"

# Keep both the start and the first sector after the ESP aligned to 1 MiB,
# while reserving space for the secondary GPT structures at the end of disk.
TOTAL_SECTORS=$((IMAGE_SIZE_MIB * 2048))
PARTITION_END=$(( ((TOTAL_SECTORS - 34) / 2048) * 2048 - 1 ))
if [ "$PARTITION_END" -le 2048 ]; then
  echo "Image is too small for an aligned EFI System Partition" >&2
  exit 1
fi

sgdisk --clear \
  --new=1:2048:"$PARTITION_END" \
  --typecode=1:EF00 \
  --change-name=1:"Accessible Windows EFI" \
  "$OUTPUT_IMAGE"

LOOP_DEVICE="$(sudo losetup --find --show --partscan "$OUTPUT_IMAGE")"
PARTITION="${LOOP_DEVICE}p1"

for _ in $(seq 1 50); do
  if [ -b "$PARTITION" ]; then
    break
  fi
  sleep 0.1
done

if [ ! -b "$PARTITION" ]; then
  echo "EFI partition device did not appear: $PARTITION" >&2
  exit 1
fi

sudo mkfs.vfat -F 32 -n AWBOOT "$PARTITION"
sudo mount "$PARTITION" "$MOUNT_DIR"
MOUNTED=1
sudo mkdir -p "$MOUNT_DIR/EFI/BOOT"
sudo cp "$EFI_BINARY" "$MOUNT_DIR/EFI/BOOT/BOOTX64.EFI"
sudo cp "$KERNEL_BINARY" "$MOUNT_DIR/KERNEL.BIN"
sync

if [ ! -s "$MOUNT_DIR/EFI/BOOT/BOOTX64.EFI" ] || [ ! -s "$MOUNT_DIR/KERNEL.BIN" ]; then
  echo "ESP verification failed: required boot files are missing" >&2
  exit 1
fi
sudo cmp "$KERNEL_BINARY" "$MOUNT_DIR/KERNEL.BIN"
echo "AW_ESP_KERNEL_VERIFY_OK path=\\KERNEL.BIN bytes=$(stat -c%s "$KERNEL_BINARY")"

sudo umount "$MOUNT_DIR"
MOUNTED=0
sudo losetup -d "$LOOP_DEVICE"
LOOP_DEVICE=""

sgdisk --verify "$OUTPUT_IMAGE"

echo "AW_DISK_IMAGE_OK path=$OUTPUT_IMAGE size_mib=$IMAGE_SIZE_MIB partition_end=$PARTITION_END kernel=$KERNEL_BINARY"
