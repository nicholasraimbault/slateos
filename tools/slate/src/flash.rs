/// `slate flash` — flash a built image onto a connected device.
///
/// Scaffold implementation.  Actual fastboot/adb logic will be added per
/// device when cross-compilation is ready.  For now the command prints what
/// it *would* do and exits cleanly so the overall CLI scaffolding is usable.
use anyhow::Result;
use clap::Args;
use tracing::warn;

use crate::device::Device;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate flash`.
#[derive(Debug, Args)]
pub struct FlashArgs {
    /// Target device to flash.
    #[arg(short, long, default_value = "generic-x86")]
    pub device: Device,
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate flash`.
///
/// Flash is not yet implemented — it requires fastboot tooling and a built
/// device image.  This stub prints guidance and exits without error so that
/// the CLI is usable end-to-end.
pub fn run(args: FlashArgs) -> Result<()> {
    warn!("slate flash is not yet implemented");

    println!();
    println!("  slate flash (not yet implemented)");
    println!("  ----------------------------------");
    println!("  device : {}", args.device);
    println!();
    println!("  Flash support requires:");
    println!("    1. A built rootfs image  (run `bash build/build-rootfs.sh`)");
    println!("    2. fastboot (for Pixel targets) or dd (for generic-x86)");
    println!("    3. The device connected and in bootloader mode");
    println!();

    match args.device {
        Device::GenericX86 | Device::Framework12 => {
            println!("  For x86 devices, write the rootfs image with dd:");
            println!("    doas dd if=slate-x86_64.img of=/dev/sdX bs=4M status=progress");
        }
        Device::PixelTablet | Device::PixelPhone | Device::PixelFold => {
            println!("  For Pixel devices, boot to fastboot and run:");
            println!("    fastboot flash boot slate-boot.img");
            println!("    fastboot flash system slate-system.img");
        }
    }

    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_runs_without_error_for_all_devices() {
        // run() must not return an error for any device — it's a scaffold.
        for &device in Device::ALL {
            let args = FlashArgs { device };
            assert!(run(args).is_ok(), "flash returned error for {device}");
        }
    }
}
