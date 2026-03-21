# Supported Devices

## Pixel Tablet (Primary)

- **SoC:** Google Tensor G2
- **Display:** 2560x1600 @ 60Hz
- **Status:** In development
- **Boot:** Fastboot, unlockable bootloader
- **Kernel:** TBD (ChromeOS kernel tree as starting point)
- **Notes:** Primary development target. Tensor G2 has upstream Linux support via ChromeOS kernel trees. Google is actively upstreaming Tensor support (GS101/Pixel 6 landed in Linux 6.8).

## Pixel Phones

- **SoC:** Google Tensor (various generations)
- **Status:** Planned
- **Boot:** Fastboot, unlockable bootloader
- **Notes:** Same Tensor platform as Pixel Tablet, shared kernel and driver work.

## Generic x86 Desktop/Laptop

- **Status:** Active (development target)
- **Boot:** Standard UEFI
- **Kernel:** Mainline Linux
- **Notes:** Standard Chimera Linux install with SlateOS shell overlay. Simplest target — no vendor blobs, standard drivers. Primary development environment before real hardware is available.
