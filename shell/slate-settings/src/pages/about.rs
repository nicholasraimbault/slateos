/// About page: device info, OS version, hardware details.
///
/// All system info is gathered asynchronously; missing commands or
/// inaccessible files produce "Unknown" rather than errors, so this
/// page works on any platform even if `/proc` is not mounted.
use iced::widget::{column, container, text};
use iced::{Element, Length};

/// Slate OS version string.
pub const SLATE_VERSION: &str = "0.1.0";

/// Device info collected at startup.
#[derive(Debug, Clone)]
pub struct AboutInfo {
    pub device: String,
    pub soc: String,
    pub ram: String,
    pub storage: String,
    pub os_version: String,
    pub os_release: String,
    pub kernel: String,
    /// Errors encountered while gathering info (shown inline so the user
    /// knows why a field says "Unknown").
    pub gather_errors: Vec<String>,
}

impl Default for AboutInfo {
    fn default() -> Self {
        Self {
            device: "Unknown".to_string(),
            soc: "Unknown".to_string(),
            ram: "Unknown".to_string(),
            storage: "Unknown".to_string(),
            os_version: SLATE_VERSION.to_string(),
            os_release: "Unknown".to_string(),
            kernel: "Unknown".to_string(),
            gather_errors: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// System info gathering
// ---------------------------------------------------------------------------

/// Collect system info from /proc/meminfo, df, uname, and os-release.
/// Each field is gathered independently so a single failure does not
/// prevent other fields from populating.
pub async fn gather_info() -> AboutInfo {
    let mut errors = Vec::new();

    let ram = match read_ram().await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("could not read RAM info: {e}");
            errors.push(format!("RAM: {e}"));
            "Unknown".to_string()
        }
    };

    let storage = match read_storage().await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("could not read storage info: {e}");
            errors.push(format!("Storage: {e}"));
            "Unknown".to_string()
        }
    };

    let kernel = match read_kernel().await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("could not read kernel version: {e}");
            errors.push(format!("Kernel: {e}"));
            "Unknown".to_string()
        }
    };

    let os_release = match read_os_release().await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("could not read os-release: {e}");
            errors.push(format!("OS release: {e}"));
            "Unknown".to_string()
        }
    };

    let (device, soc) = read_device_info().await;

    AboutInfo {
        device,
        soc,
        ram,
        storage,
        os_version: SLATE_VERSION.to_string(),
        os_release,
        kernel,
        gather_errors: errors,
    }
}

async fn read_ram() -> Result<String, String> {
    let content = tokio::fs::read_to_string("/proc/meminfo")
        .await
        .map_err(|e| format!("/proc/meminfo not readable: {e}"))?;
    Ok(parse_meminfo(&content))
}

/// Parse MemTotal from /proc/meminfo.
fn parse_meminfo(content: &str) -> String {
    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(kb) = parts[1].parse::<u64>() {
                    let gb = kb as f64 / 1024.0 / 1024.0;
                    return format!("{gb:.1} GB");
                }
            }
        }
    }
    "Unknown".to_string()
}

async fn read_storage() -> Result<String, String> {
    let output = tokio::process::Command::new("df")
        .args(["-h", "/"])
        .output()
        .await
        .map_err(|e| format!("df command not available: {e}"))?;

    if !output.status.success() {
        return Err("df command returned an error".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_df_output(&stdout))
}

/// Parse the second line of `df -h /` for size/used/avail.
fn parse_df_output(output: &str) -> String {
    if let Some(line) = output.lines().nth(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            return format!(
                "{} total, {} used, {} available",
                parts[1], parts[2], parts[3]
            );
        }
    }
    "Unknown".to_string()
}

async fn read_kernel() -> Result<String, String> {
    let output = tokio::process::Command::new("uname")
        .arg("-r")
        .output()
        .await
        .map_err(|e| format!("uname command not available: {e}"))?;

    if !output.status.success() {
        return Err("uname command returned an error".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Read PRETTY_NAME from /etc/os-release for a human-readable distro string.
async fn read_os_release() -> Result<String, String> {
    let content = tokio::fs::read_to_string("/etc/os-release")
        .await
        .map_err(|e| format!("/etc/os-release not readable: {e}"))?;
    Ok(parse_os_release(&content))
}

/// Parse PRETTY_NAME from os-release format.
fn parse_os_release(content: &str) -> String {
    for line in content.lines() {
        if line.starts_with("PRETTY_NAME=") {
            let value = line.trim_start_matches("PRETTY_NAME=");
            // Strip surrounding quotes if present
            return value.trim_matches('"').to_string();
        }
    }
    "Unknown".to_string()
}

/// Read device model and SoC from devicetree (ARM) or DMI (x86). Falls
/// back to "Unknown" on desktop / non-Linux platforms.
async fn read_device_info() -> (String, String) {
    // Try devicetree first (ARM devices like Pixel Tablet, ONN Tablet)
    let device = match tokio::fs::read_to_string("/proc/device-tree/model").await {
        Ok(model) => model.trim_matches('\0').trim().to_string(),
        Err(_) => {
            // Fall back to DMI product name (x86 laptops/desktops)
            match tokio::fs::read_to_string("/sys/class/dmi/id/product_name").await {
                Ok(name) => name.trim().to_string(),
                Err(_) => "Unknown device".to_string(),
            }
        }
    };

    // SoC is typically only in devicetree; DMI does not expose it
    let soc = match tokio::fs::read_to_string("/proc/device-tree/compatible").await {
        Ok(compat) => {
            // compatible is null-separated; take the first entry
            compat.split('\0').next().unwrap_or("Unknown").to_string()
        }
        Err(_) => {
            // On x86, read the CPU model from /proc/cpuinfo instead
            match tokio::fs::read_to_string("/proc/cpuinfo").await {
                Ok(info) => parse_cpu_model(&info),
                Err(_) => "Unknown".to_string(),
            }
        }
    };

    (device, soc)
}

/// Extract the first "model name" from /proc/cpuinfo on x86.
fn parse_cpu_model(cpuinfo: &str) -> String {
    for line in cpuinfo.lines() {
        if line.starts_with("model name") {
            if let Some(value) = line.split(':').nth(1) {
                return value.trim().to_string();
            }
        }
    }
    "Unknown".to_string()
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum AboutMsg {
    InfoLoaded(AboutInfo),
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(info: &AboutInfo) -> Element<'_, AboutMsg> {
    let mut items: Vec<Element<'_, AboutMsg>> = vec![
        text("About").size(24).into(),
        text(format!("Device: {}", info.device)).size(16).into(),
        text(format!("SoC/CPU: {}", info.soc)).size(16).into(),
        text(format!("RAM: {}", info.ram)).size(16).into(),
        text(format!("Storage: {}", info.storage)).size(16).into(),
        text(format!("Slate OS: {}", info.os_version))
            .size(16)
            .into(),
        text(format!("Base: {}", info.os_release)).size(16).into(),
        text(format!("Kernel: {}", info.kernel)).size(16).into(),
    ];

    // Show any errors encountered during info gathering so the user
    // understands why fields say "Unknown"
    if !info.gather_errors.is_empty() {
        items.push(
            text("Some system information could not be read:")
                .size(13)
                .color(iced::Color::from_rgb(0.7, 0.5, 0.2))
                .into(),
        );
        for err in &info.gather_errors {
            items.push(
                text(format!("  - {err}"))
                    .size(12)
                    .color(iced::Color::from_rgb(0.7, 0.5, 0.2))
                    .into(),
            );
        }
    }

    let content = column(items).spacing(12).padding(20).width(Length::Fill);

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_is_well_formed() {
        // Must be semver-like: X.Y.Z
        let parts: Vec<&str> = SLATE_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version must have 3 parts: {SLATE_VERSION}");
        for part in &parts {
            assert!(
                part.parse::<u32>().is_ok(),
                "version part '{part}' is not a number"
            );
        }
    }

    #[test]
    fn parse_meminfo_extracts_total() {
        let content = "MemTotal:        3907088 kB\nMemFree:          123456 kB\n";
        let result = parse_meminfo(content);
        assert!(result.contains("GB"), "expected GB in: {result}");
        assert!(result.contains("3."), "expected ~3.7 in: {result}");
    }

    #[test]
    fn parse_meminfo_handles_missing() {
        let result = parse_meminfo("nothing useful here");
        assert_eq!(result, "Unknown");
    }

    #[test]
    fn parse_df_output_extracts_sizes() {
        let output =
            "Filesystem      Size  Used Avail Use% Mounted on\n/dev/sda1        50G   20G   28G  42% /\n";
        let result = parse_df_output(output);
        assert!(result.contains("50G"), "expected 50G in: {result}");
        assert!(result.contains("20G"), "expected 20G in: {result}");
    }

    #[test]
    fn default_about_info_has_defaults() {
        let info = AboutInfo::default();
        assert_eq!(info.os_version, SLATE_VERSION);
        assert!(info.gather_errors.is_empty());
    }

    #[test]
    fn parse_os_release_extracts_pretty_name() {
        let content = "ID=chimera\nPRETTY_NAME=\"Chimera Linux\"\nVERSION_ID=1\n";
        let result = parse_os_release(content);
        assert_eq!(result, "Chimera Linux");
    }

    #[test]
    fn parse_os_release_handles_no_quotes() {
        let content = "PRETTY_NAME=Chimera\n";
        let result = parse_os_release(content);
        assert_eq!(result, "Chimera");
    }

    #[test]
    fn parse_os_release_returns_unknown_when_missing() {
        let content = "ID=chimera\nVERSION_ID=1\n";
        let result = parse_os_release(content);
        assert_eq!(result, "Unknown");
    }

    #[test]
    fn parse_cpu_model_extracts_name() {
        let cpuinfo = "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7\nflags\t: avx\n";
        let result = parse_cpu_model(cpuinfo);
        assert_eq!(result, "Intel(R) Core(TM) i7");
    }

    #[test]
    fn parse_cpu_model_handles_missing() {
        let result = parse_cpu_model("no model here");
        assert_eq!(result, "Unknown");
    }
}
