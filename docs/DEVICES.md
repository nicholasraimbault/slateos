# Supported Devices

## Pixel Tablet (Primary)

- **SoC:** Google Tensor G2
- **Status:** In development
- **Boot:** Fastboot, unlockable bootloader
- **Kernel:** TBD
- **Notes:** Primary development target. Tensor G2 has good upstream Linux support via ChromeOS kernel trees.

## Pixel Phones

- **SoC:** Google Tensor (various generations)
- **Status:** Planned
- **Boot:** Fastboot, unlockable bootloader
- **Notes:** Same Tensor platform as Pixel Tablet, shared kernel and driver work.

## Generic x86 Desktop/Laptop

- **Status:** Planned
- **Boot:** Standard UEFI
- **Kernel:** Mainline Linux
- **Notes:** Standard Chimera Linux install with SlateOS shell overlay. Simplest target — no vendor blobs, standard drivers.

## ONN 11 Tablet Pro 2024 (Legacy)

- **SoC:** Qualcomm SM6225 (Snapdragon 685), Adreno 610
- **RAM:** 4GB
- **Display:** 1280x1840 @ 90Hz (DSI)
- **Status:** Experimental / legacy
- **Boot:** Fastboot (A/B slots), boot image v4 header
- **Kernel:** GKI 5.15 + vendor modules from vendor_boot
- **Notes:** Original development device. Requires vendor blobs and kernel modules from stock firmware. Hardware setup service handles mknod for DRM/input devices. Device-specific services in `services/devices/onn-tablet/`.
