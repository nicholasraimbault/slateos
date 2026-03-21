/// About page: device info, OS version, hardware details.
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
    pub kernel: String,
}

impl Default for AboutInfo {
    fn default() -> Self {
        Self {
            device: "ONN 11 Tablet Pro 2024".to_string(),
            soc: "Snapdragon 685".to_string(),
            ram: "Unknown".to_string(),
            storage: "Unknown".to_string(),
            os_version: SLATE_VERSION.to_string(),
            kernel: "Unknown".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// System info gathering
// ---------------------------------------------------------------------------

/// Collect system info from /proc/meminfo, df, uname.
/// Failures produce "Unknown" rather than errors.
pub async fn gather_info() -> AboutInfo {
    let ram = read_ram().await;
    let storage = read_storage().await;
    let kernel = read_kernel().await;

    AboutInfo {
        device: "ONN 11 Tablet Pro 2024".to_string(),
        soc: "Snapdragon 685".to_string(),
        ram,
        storage,
        os_version: SLATE_VERSION.to_string(),
        kernel,
    }
}

async fn read_ram() -> String {
    match tokio::fs::read_to_string("/proc/meminfo").await {
        Ok(content) => parse_meminfo(&content),
        Err(_) => "Unknown".to_string(),
    }
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

async fn read_storage() -> String {
    let output = tokio::process::Command::new("df")
        .args(["-h", "/"])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            parse_df_output(&stdout)
        }
        _ => "Unknown".to_string(),
    }
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

async fn read_kernel() -> String {
    let output = tokio::process::Command::new("uname")
        .arg("-r")
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "Unknown".to_string(),
    }
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
    let content = column![
        text("About").size(24),
        text(format!("Device: {}", info.device)).size(16),
        text(format!("SoC: {}", info.soc)).size(16),
        text(format!("RAM: {}", info.ram)).size(16),
        text(format!("Storage: {}", info.storage)).size(16),
        text(format!("Slate OS: {}", info.os_version)).size(16),
        text(format!("Kernel: {}", info.kernel)).size(16),
    ]
    .spacing(12)
    .padding(20)
    .width(Length::Fill);

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
        let output = "Filesystem      Size  Used Avail Use% Mounted on\n/dev/sda1        50G   20G   28G  42% /\n";
        let result = parse_df_output(output);
        assert!(result.contains("50G"), "expected 50G in: {result}");
        assert!(result.contains("20G"), "expected 20G in: {result}");
    }

    #[test]
    fn default_about_info_has_device() {
        let info = AboutInfo::default();
        assert_eq!(info.device, "ONN 11 Tablet Pro 2024");
        assert_eq!(info.os_version, SLATE_VERSION);
    }
}
