/// `slate services` — list and inspect arkhe service definitions.
///
/// Reads the `services/` directory tree in the slateos repo to display
/// service names, dependencies, sandbox modes, and environment variables.
/// Useful for debugging the boot cascade and verifying service configs.
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;

use crate::device::Device;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate services`.
#[derive(Debug, Args)]
pub struct ServicesArgs {
    /// Target device (shows device-specific overrides merged with base).
    #[arg(short, long, default_value = "generic-x86")]
    pub device: Device,

    /// Show a single service in detail instead of listing all.
    #[arg(short, long)]
    pub inspect: Option<String>,
}

// ---------------------------------------------------------------------------
// Service info
// ---------------------------------------------------------------------------

/// Parsed service definition.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub source: ServiceSource,
    pub depends: Vec<String>,
    pub sandbox: String,
    pub env: BTreeMap<String, String>,
    pub run_script: String,
}

/// Where a service definition comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceSource {
    Base,
    Device(String),
}

impl std::fmt::Display for ServiceSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceSource::Base => write!(f, "base"),
            ServiceSource::Device(d) => write!(f, "device:{d}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate services`.
pub fn run(args: ServicesArgs) -> Result<()> {
    let repo_root = crate::workspace::find_repo_root().context("could not find slateos repo root")?;
    let services = discover_services(&repo_root, &args.device)?;

    if let Some(name) = &args.inspect {
        inspect_service(&services, name);
    } else {
        list_services(&services);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Discover all services for the given device (base + device overrides).
fn discover_services(repo_root: &Path, device: &Device) -> Result<Vec<ServiceInfo>> {
    let base_dir = repo_root.join("services").join("base");
    let device_dir = repo_root
        .join("services")
        .join("devices")
        .join(device_dir_name(device));

    let mut services = BTreeMap::new();

    // Load base services.
    if base_dir.is_dir() {
        for entry in read_service_dir(&base_dir, ServiceSource::Base)? {
            services.insert(entry.name.clone(), entry);
        }
    }

    // Layer device-specific overrides on top.
    if device_dir.is_dir() {
        let source = ServiceSource::Device(device.to_string());
        for entry in read_service_dir(&device_dir, source)? {
            services.insert(entry.name.clone(), entry);
        }
    }

    Ok(services.into_values().collect())
}

/// Read all service directories under `parent`.
fn read_service_dir(parent: &Path, source: ServiceSource) -> Result<Vec<ServiceInfo>> {
    let mut services = Vec::new();

    let entries = std::fs::read_dir(parent)
        .with_context(|| format!("failed to read {}", parent.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry
            .file_name()
            .to_string_lossy()
            .to_string();

        let info = parse_service(&path, name, source.clone());
        services.push(info);
    }

    Ok(services)
}

/// Parse a single service directory into ServiceInfo.
fn parse_service(dir: &Path, name: String, source: ServiceSource) -> ServiceInfo {
    let depends = read_lines(&dir.join("depends"));
    let sandbox = read_first_line(&dir.join("sandbox"));
    let run_script = read_file_or_empty(&dir.join("run"));
    let env = read_env_dir(&dir.join("env"));

    ServiceInfo {
        name,
        source,
        depends,
        sandbox,
        env,
        run_script,
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

fn list_services(services: &[ServiceInfo]) {
    println!();
    println!("  slate services");
    println!("  --------------");
    println!();
    let header = format!(
        "  {:<22} {:<8} {:<14} {}",
        "NAME", "SOURCE", "SANDBOX", "DEPENDS"
    );
    println!("{header}");
    println!("  {}", "-".repeat(70));

    for svc in services {
        let deps = if svc.depends.is_empty() {
            "(none)".to_string()
        } else {
            svc.depends.join(", ")
        };

        println!(
            "  {:<22} {:<8} {:<14} {}",
            svc.name, svc.source, svc.sandbox, deps
        );
    }

    println!();
    println!("  {} services total", services.len());
    println!();
}

fn inspect_service(services: &[ServiceInfo], name: &str) {
    let Some(svc) = services.iter().find(|s| s.name == name) else {
        println!("  Service '{name}' not found.");
        return;
    };

    println!();
    println!("  slate services --inspect {name}");
    println!("  {}", "-".repeat(35 + name.len()));
    println!();
    println!("  name    : {}", svc.name);
    println!("  source  : {}", svc.source);
    println!("  sandbox : {}", svc.sandbox);
    println!(
        "  depends : {}",
        if svc.depends.is_empty() {
            "(none)".to_string()
        } else {
            svc.depends.join(", ")
        }
    );
    println!();

    if !svc.env.is_empty() {
        println!("  environment:");
        for (k, v) in &svc.env {
            println!("    {k} = {v}");
        }
        println!();
    }

    println!("  run script:");
    for line in svc.run_script.lines() {
        println!("    {line}");
    }
    println!();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn device_dir_name(device: &Device) -> &'static str {
    match device {
        Device::PixelTablet => "pixel-tablet",
        Device::PixelPhone => "pixel-phone",
        Device::PixelFold => "pixel-fold",
        Device::Framework12 | Device::GenericX86 => "generic",
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect()
}

fn read_first_line(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or("(none)")
        .trim()
        .to_string()
}

fn read_file_or_empty(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Read all files in an `env/` subdirectory as key=value pairs.
fn read_env_dir(dir: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return env;
    };

    for entry in entries.flatten() {
        let key = entry.file_name().to_string_lossy().to_string();
        let value = std::fs::read_to_string(entry.path())
            .unwrap_or_default()
            .trim()
            .to_string();
        env.insert(key, value);
    }

    env
}



// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_service_reads_depends() {
        let dir = tempdir("depends");
        fs::write(dir.join("depends"), "seatd\ndbus-session\n").unwrap();
        fs::write(dir.join("run"), "#!/bin/sh\nexec /usr/bin/test").unwrap();
        fs::write(dir.join("sandbox"), "sandbox = permissive").unwrap();

        let info = parse_service(&dir, "test-svc".to_string(), ServiceSource::Base);
        assert_eq!(info.depends, vec!["seatd", "dbus-session"]);
        assert_eq!(info.name, "test-svc");
    }

    #[test]
    fn parse_service_missing_files_returns_defaults() {
        let dir = tempdir("missing");
        let info = parse_service(&dir, "empty".to_string(), ServiceSource::Base);
        assert!(info.depends.is_empty());
        assert!(info.run_script.is_empty());
    }

    #[test]
    fn read_env_dir_parses_files() {
        let dir = tempdir("env");
        let env_dir = dir.join("env");
        fs::create_dir(&env_dir).unwrap();
        fs::write(env_dir.join("HOME"), "/home/slate\n").unwrap();
        fs::write(env_dir.join("LANG"), "en_US.UTF-8\n").unwrap();

        let env = read_env_dir(&env_dir);
        assert_eq!(env.get("HOME").unwrap(), "/home/slate");
        assert_eq!(env.get("LANG").unwrap(), "en_US.UTF-8");
    }

    #[test]
    fn service_source_display() {
        assert_eq!(ServiceSource::Base.to_string(), "base");
        assert_eq!(
            ServiceSource::Device("pixel-tablet".to_string()).to_string(),
            "device:pixel-tablet"
        );
    }

    #[test]
    fn device_dir_name_pixel_tablet() {
        assert_eq!(device_dir_name(&Device::PixelTablet), "pixel-tablet");
    }

    fn tempdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "slate-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }
}
