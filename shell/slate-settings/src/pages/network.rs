/// Network settings page: WiFi scanning and Bluetooth toggle.
///
/// Uses nmcli for WiFi operations and bluetoothctl for Bluetooth.
/// These are system-level settings not stored in settings.toml.
use iced::widget::{button, column, container, scrollable, text, text_input, toggler};
use iced::{Element, Length};

// ---------------------------------------------------------------------------
// WiFi network data
// ---------------------------------------------------------------------------

/// Represents a scanned WiFi network.
#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub connected: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// State for the network settings page.
#[derive(Debug, Clone, Default)]
pub struct NetworkState {
    pub wifi_networks: Vec<WifiNetwork>,
    pub bluetooth_on: bool,
    pub scanning: bool,
    /// SSID the user is trying to connect to (prompts for password).
    pub connecting_ssid: Option<String>,
    /// Password input buffer.
    pub password_input: String,
    pub status_message: String,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum NetworkMsg {
    ScanWifi,
    WifiScanned(Vec<WifiNetwork>),
    ConnectWifi(String),
    PasswordInput(String),
    SubmitPassword,
    CancelConnect,
    ConnectionResult(Result<String, String>),
    ToggleBluetooth(bool),
    BluetoothResult(Result<String, String>),
}

// ---------------------------------------------------------------------------
// System interaction
// ---------------------------------------------------------------------------

/// Scan WiFi networks via nmcli.
pub async fn scan_wifi() -> Vec<WifiNetwork> {
    let output = tokio::process::Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "SSID,SIGNAL,SECURITY,ACTIVE",
            "device",
            "wifi",
            "list",
            "--rescan",
            "yes",
        ])
        .output()
        .await;

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("failed to scan wifi: {e}");
            return Vec::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_nmcli_output(&stdout)
}

/// Parse nmcli terse output into WifiNetwork structs.
fn parse_nmcli_output(output: &str) -> Vec<WifiNetwork> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 4 {
                let ssid = parts[0].to_string();
                if ssid.is_empty() {
                    return None;
                }
                let signal = parts[1].parse::<u8>().unwrap_or(0);
                let security = parts[2].to_string();
                let connected = parts[3] == "yes";
                Some(WifiNetwork {
                    ssid,
                    signal,
                    security,
                    connected,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Connect to a WiFi network via nmcli.
async fn connect_wifi(ssid: &str, password: Option<&str>) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("nmcli");
    cmd.args(["device", "wifi", "connect", ssid]);
    if let Some(pw) = password {
        cmd.args(["password", pw]);
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run nmcli: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(format!("Connected to {ssid}"))
    } else {
        Err(format!("Failed to connect: {stderr} {stdout}"))
    }
}

/// Toggle Bluetooth via bluetoothctl.
async fn toggle_bluetooth(on: bool) -> Result<String, String> {
    let action = if on { "on" } else { "off" };
    let output = tokio::process::Command::new("bluetoothctl")
        .args(["power", action])
        .output()
        .await
        .map_err(|e| format!("failed to run bluetoothctl: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        Ok(format!("Bluetooth {action}"))
    } else {
        Err(format!("bluetoothctl failed: {stdout}"))
    }
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(state: &mut NetworkState, msg: NetworkMsg) -> Option<iced::Task<NetworkMsg>> {
    match msg {
        NetworkMsg::ScanWifi => {
            state.scanning = true;
            let task = iced::Task::perform(async { scan_wifi().await }, NetworkMsg::WifiScanned);
            Some(task)
        }
        NetworkMsg::WifiScanned(networks) => {
            state.wifi_networks = networks;
            state.scanning = false;
            None
        }
        NetworkMsg::ConnectWifi(ssid) => {
            // Check if network needs a password
            let needs_password = state
                .wifi_networks
                .iter()
                .any(|n| n.ssid == ssid && !n.security.is_empty() && n.security != "--");

            if needs_password {
                state.connecting_ssid = Some(ssid);
                state.password_input.clear();
                None
            } else {
                let task = iced::Task::perform(
                    async move { connect_wifi(&ssid, None).await },
                    NetworkMsg::ConnectionResult,
                );
                Some(task)
            }
        }
        NetworkMsg::PasswordInput(pw) => {
            state.password_input = pw;
            None
        }
        NetworkMsg::SubmitPassword => {
            if let Some(ssid) = state.connecting_ssid.take() {
                let password = state.password_input.clone();
                state.password_input.clear();
                let task = iced::Task::perform(
                    async move { connect_wifi(&ssid, Some(&password)).await },
                    NetworkMsg::ConnectionResult,
                );
                Some(task)
            } else {
                None
            }
        }
        NetworkMsg::CancelConnect => {
            state.connecting_ssid = None;
            state.password_input.clear();
            None
        }
        NetworkMsg::ConnectionResult(result) => {
            state.status_message = match result {
                Ok(msg) => msg,
                Err(msg) => msg,
            };
            // Re-scan after connecting
            state.scanning = true;
            let task = iced::Task::perform(async { scan_wifi().await }, NetworkMsg::WifiScanned);
            Some(task)
        }
        NetworkMsg::ToggleBluetooth(on) => {
            let task = iced::Task::perform(
                async move { toggle_bluetooth(on).await },
                NetworkMsg::BluetoothResult,
            );
            Some(task)
        }
        NetworkMsg::BluetoothResult(result) => {
            match &result {
                Ok(msg) => {
                    state.status_message = msg.clone();
                    state.bluetooth_on = msg.contains("on");
                }
                Err(msg) => {
                    state.status_message = msg.clone();
                }
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(state: &NetworkState) -> Element<'_, NetworkMsg> {
    let mut items: Vec<Element<'_, NetworkMsg>> = Vec::new();

    items.push(text("Network").size(24).into());

    // WiFi scan button
    let scan_label = if state.scanning {
        "Scanning..."
    } else {
        "Scan WiFi"
    };
    items.push(
        button(text(scan_label).size(14))
            .on_press(NetworkMsg::ScanWifi)
            .padding(8)
            .into(),
    );

    // WiFi network list
    for network in &state.wifi_networks {
        let signal_bars = match network.signal {
            0..=25 => "| ",
            26..=50 => "|| ",
            51..=75 => "||| ",
            _ => "|||| ",
        };
        let security_label = if network.security.is_empty() || network.security == "--" {
            "Open"
        } else {
            &network.security
        };
        let connected_mark = if network.connected {
            " (connected)"
        } else {
            ""
        };
        let label = format!(
            "{signal_bars} {} [{security_label}]{connected_mark}",
            network.ssid
        );

        let ssid = network.ssid.clone();
        items.push(
            button(text(label).size(14))
                .on_press(NetworkMsg::ConnectWifi(ssid))
                .width(Length::Fill)
                .padding(8)
                .into(),
        );
    }

    // Password prompt
    if let Some(ssid) = &state.connecting_ssid {
        items.push(text(format!("Password for {ssid}:")).size(14).into());
        items.push(
            text_input("Password", &state.password_input)
                .on_input(NetworkMsg::PasswordInput)
                .on_submit(NetworkMsg::SubmitPassword)
                .secure(true)
                .padding(8)
                .into(),
        );
        items.push(
            iced::widget::row![
                button(text("Connect").size(14))
                    .on_press(NetworkMsg::SubmitPassword)
                    .padding(6),
                button(text("Cancel").size(14))
                    .on_press(NetworkMsg::CancelConnect)
                    .padding(6),
            ]
            .spacing(8)
            .into(),
        );
    }

    // Bluetooth toggle
    items.push(
        toggler(state.bluetooth_on)
            .label("Bluetooth")
            .on_toggle(NetworkMsg::ToggleBluetooth)
            .into(),
    );

    // Status message
    if !state.status_message.is_empty() {
        items.push(text(&state.status_message).size(12).into());
    }

    let content = scrollable(column(items).spacing(12).padding(20).width(Length::Fill));

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nmcli_output_parses_valid_lines() {
        let input = "MyWifi:85:WPA2:yes\nGuest:40::no\n";
        let networks = parse_nmcli_output(input);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "MyWifi");
        assert_eq!(networks[0].signal, 85);
        assert!(networks[0].connected);
        assert_eq!(networks[1].ssid, "Guest");
        assert!(!networks[1].connected);
    }

    #[test]
    fn parse_nmcli_output_skips_empty_ssid() {
        let input = ":50:WPA2:no\n";
        let networks = parse_nmcli_output(input);
        assert!(networks.is_empty());
    }

    #[test]
    fn default_network_state() {
        let state = NetworkState::default();
        assert!(state.wifi_networks.is_empty());
        assert!(!state.bluetooth_on);
        assert!(!state.scanning);
    }
}
