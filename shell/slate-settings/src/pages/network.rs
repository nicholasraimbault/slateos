/// Network settings page: WiFi scanning, connection, and Bluetooth toggle.
///
/// Uses nmcli for WiFi operations and bluetoothctl for Bluetooth.
/// These are system-level settings not stored in settings.toml.
use iced::widget::{button, column, container, row, scrollable, text, text_input, toggler};
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

impl WifiNetwork {
    /// Whether this network requires a password to connect.
    pub fn needs_password(&self) -> bool {
        !self.security.is_empty() && self.security != "--"
    }
}

// ---------------------------------------------------------------------------
// Connection state machine
// ---------------------------------------------------------------------------

/// Tracks the WiFi connection flow through discrete phases so the UI can
/// show the right prompt at each step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConnectPhase {
    #[default]
    Idle,
    /// Waiting for the user to enter a password for a secured network.
    AwaitingPassword { ssid: String },
    /// nmcli connect is in progress.
    Connecting { ssid: String },
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
    /// Current phase of the WiFi connection flow.
    pub connect_phase: ConnectPhase,
    /// Password input buffer.
    pub password_input: String,
    /// Informational status message (last success).
    pub status_message: String,
    /// Error message from the last failed operation, displayed inline.
    pub error_message: Option<String>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum NetworkMsg {
    ScanWifi,
    WifiScanned(Result<Vec<WifiNetwork>, String>),
    ConnectWifi(String),
    PasswordInput(String),
    SubmitPassword,
    CancelConnect,
    DisconnectWifi(String),
    ConnectionResult(Result<String, String>),
    ToggleBluetooth(bool),
    BluetoothResult(Result<String, String>),
    DismissError,
}

// ---------------------------------------------------------------------------
// System interaction
// ---------------------------------------------------------------------------

/// Scan WiFi networks via nmcli.
pub async fn scan_wifi() -> Result<Vec<WifiNetwork>, String> {
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
        .await
        .map_err(|e| format!("nmcli not available: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("WiFi scan failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_nmcli_output(&stdout))
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
        .map_err(|e| format!("Failed to run nmcli: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(format!("Connected to {ssid}"))
    } else {
        // Provide user-friendly messages for common failure modes
        let combined = format!("{stderr} {stdout}");
        if combined.contains("Secrets were required")
            || combined.contains("secret")
            || combined.contains("password")
        {
            Err(format!("Wrong password for {ssid}"))
        } else if combined.contains("No network with SSID") {
            Err(format!("Network \"{ssid}\" not found"))
        } else if combined.contains("No suitable device") || combined.contains("no Wi-Fi device") {
            Err("No WiFi adapter found on this device".to_string())
        } else if combined.contains("timed out") || combined.contains("Timeout") {
            Err(format!("Connection to {ssid} timed out"))
        } else {
            Err(format!("Connection failed: {}", combined.trim()))
        }
    }
}

/// Disconnect from a WiFi network via nmcli.
async fn disconnect_wifi(ssid: &str) -> Result<String, String> {
    // Find the connection name matching the SSID, then deactivate it
    let output = tokio::process::Command::new("nmcli")
        .args(["connection", "down", ssid])
        .output()
        .await
        .map_err(|e| format!("Failed to run nmcli: {e}"))?;

    if output.status.success() {
        Ok(format!("Disconnected from {ssid}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Disconnect failed: {}", stderr.trim()))
    }
}

/// Toggle Bluetooth via bluetoothctl.
async fn toggle_bluetooth(on: bool) -> Result<String, String> {
    let action = if on { "on" } else { "off" };
    let output = tokio::process::Command::new("bluetoothctl")
        .args(["power", action])
        .output()
        .await
        .map_err(|e| format!("bluetoothctl not available: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        Ok(format!("Bluetooth {action}"))
    } else {
        Err(format!("Bluetooth toggle failed: {}", stdout.trim()))
    }
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(state: &mut NetworkState, msg: NetworkMsg) -> Option<iced::Task<NetworkMsg>> {
    match msg {
        NetworkMsg::ScanWifi => {
            state.scanning = true;
            state.error_message = None;
            let task = iced::Task::perform(async { scan_wifi().await }, NetworkMsg::WifiScanned);
            Some(task)
        }
        NetworkMsg::WifiScanned(result) => {
            state.scanning = false;
            match result {
                Ok(networks) => {
                    state.wifi_networks = networks;
                }
                Err(e) => {
                    tracing::warn!("WiFi scan failed: {e}");
                    state.error_message = Some(e);
                }
            }
            None
        }
        NetworkMsg::ConnectWifi(ssid) => {
            state.error_message = None;
            // Determine whether the network needs a password
            let needs_pw = state
                .wifi_networks
                .iter()
                .any(|n| n.ssid == ssid && n.needs_password());

            if needs_pw {
                state.connect_phase = ConnectPhase::AwaitingPassword { ssid };
                state.password_input.clear();
                None
            } else {
                state.connect_phase = ConnectPhase::Connecting { ssid: ssid.clone() };
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
            if let ConnectPhase::AwaitingPassword { ssid } =
                std::mem::replace(&mut state.connect_phase, ConnectPhase::Idle)
            {
                let password = state.password_input.clone();
                state.password_input.clear();
                state.connect_phase = ConnectPhase::Connecting { ssid: ssid.clone() };
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
            state.connect_phase = ConnectPhase::Idle;
            state.password_input.clear();
            None
        }
        NetworkMsg::DisconnectWifi(ssid) => {
            state.error_message = None;
            let task = iced::Task::perform(
                async move { disconnect_wifi(&ssid).await },
                NetworkMsg::ConnectionResult,
            );
            Some(task)
        }
        NetworkMsg::ConnectionResult(result) => {
            state.connect_phase = ConnectPhase::Idle;
            match result {
                Ok(msg) => {
                    state.status_message = msg;
                    state.error_message = None;
                }
                Err(msg) => {
                    tracing::warn!("WiFi connection error: {msg}");
                    state.error_message = Some(msg);
                }
            }
            // Re-scan to reflect new connection state
            state.scanning = true;
            let task = iced::Task::perform(async { scan_wifi().await }, NetworkMsg::WifiScanned);
            Some(task)
        }
        NetworkMsg::ToggleBluetooth(on) => {
            state.error_message = None;
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
                    state.error_message = None;
                }
                Err(msg) => {
                    tracing::warn!("Bluetooth error: {msg}");
                    state.error_message = Some(msg.clone());
                }
            }
            None
        }
        NetworkMsg::DismissError => {
            state.error_message = None;
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

    // Inline error banner
    if let Some(err) = &state.error_message {
        items.push(
            row![
                text(err)
                    .size(13)
                    .color(iced::Color::from_rgb(0.9, 0.2, 0.2)),
                button(text("Dismiss").size(12))
                    .on_press(NetworkMsg::DismissError)
                    .padding(4)
            ]
            .spacing(8)
            .into(),
        );
    }

    // WiFi scan button
    let scan_label = if state.scanning {
        "Scanning..."
    } else {
        "Scan WiFi"
    };
    let mut scan_btn = button(text(scan_label).size(14)).padding(8);
    // Disable while already scanning or connecting
    if !state.scanning && state.connect_phase == ConnectPhase::Idle {
        scan_btn = scan_btn.on_press(NetworkMsg::ScanWifi);
    }
    items.push(scan_btn.into());

    // Connecting indicator
    if let ConnectPhase::Connecting { ref ssid } = state.connect_phase {
        items.push(
            text(format!("Connecting to {ssid}..."))
                .size(14)
                .color(iced::Color::from_rgb(0.6, 0.6, 0.8))
                .into(),
        );
    }

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

        if network.connected {
            // Show disconnect button for the connected network
            let ssid = network.ssid.clone();
            items.push(
                row![
                    text(label).size(14).width(Length::Fill),
                    button(text("Disconnect").size(12))
                        .on_press(NetworkMsg::DisconnectWifi(ssid))
                        .padding(6)
                ]
                .spacing(8)
                .into(),
            );
        } else {
            let ssid = network.ssid.clone();
            let mut btn = button(text(label).size(14)).width(Length::Fill).padding(8);
            // Disable connect buttons while already connecting
            if state.connect_phase == ConnectPhase::Idle {
                btn = btn.on_press(NetworkMsg::ConnectWifi(ssid));
            }
            items.push(btn.into());
        }
    }

    // Password prompt (only when awaiting password)
    if let ConnectPhase::AwaitingPassword { ref ssid } = state.connect_phase {
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
            row![
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

    // Success status message
    if !state.status_message.is_empty() && state.error_message.is_none() {
        items.push(
            text(&state.status_message)
                .size(12)
                .color(iced::Color::from_rgb(0.3, 0.7, 0.3))
                .into(),
        );
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
        assert_eq!(state.connect_phase, ConnectPhase::Idle);
        assert!(state.error_message.is_none());
    }

    #[test]
    fn needs_password_detects_secured_networks() {
        let open = WifiNetwork {
            ssid: "Open".into(),
            signal: 80,
            security: String::new(),
            connected: false,
        };
        assert!(!open.needs_password());

        let dash = WifiNetwork {
            ssid: "Dash".into(),
            signal: 80,
            security: "--".into(),
            connected: false,
        };
        assert!(!dash.needs_password());

        let wpa = WifiNetwork {
            ssid: "Secure".into(),
            signal: 80,
            security: "WPA2".into(),
            connected: false,
        };
        assert!(wpa.needs_password());
    }

    #[test]
    fn connect_phase_transitions_for_open_network() {
        let mut state = NetworkState {
            wifi_networks: vec![WifiNetwork {
                ssid: "Open".into(),
                signal: 80,
                security: String::new(),
                connected: false,
            }],
            ..Default::default()
        };
        // Connecting to an open network goes straight to Connecting phase
        let task = update(&mut state, NetworkMsg::ConnectWifi("Open".into()));
        assert!(task.is_some());
        assert!(matches!(
            state.connect_phase,
            ConnectPhase::Connecting { .. }
        ));
    }

    #[test]
    fn connect_phase_transitions_for_secured_network() {
        let mut state = NetworkState {
            wifi_networks: vec![WifiNetwork {
                ssid: "Secure".into(),
                signal: 80,
                security: "WPA2".into(),
                connected: false,
            }],
            ..Default::default()
        };
        // Connecting to a secured network enters password prompt phase
        let task = update(&mut state, NetworkMsg::ConnectWifi("Secure".into()));
        assert!(task.is_none());
        assert!(matches!(
            state.connect_phase,
            ConnectPhase::AwaitingPassword { .. }
        ));
    }

    #[test]
    fn cancel_connect_resets_phase() {
        let mut state = NetworkState {
            connect_phase: ConnectPhase::AwaitingPassword { ssid: "Net".into() },
            password_input: "secret".into(),
            ..Default::default()
        };
        update(&mut state, NetworkMsg::CancelConnect);
        assert_eq!(state.connect_phase, ConnectPhase::Idle);
        assert!(state.password_input.is_empty());
    }

    #[test]
    fn connection_error_stored_in_error_message() {
        let mut state = NetworkState::default();
        update(
            &mut state,
            NetworkMsg::ConnectionResult(Err("wrong password".into())),
        );
        assert_eq!(state.error_message.as_deref(), Some("wrong password"));
        assert_eq!(state.connect_phase, ConnectPhase::Idle);
    }

    #[test]
    fn connection_success_clears_error() {
        let mut state = NetworkState {
            error_message: Some("old error".into()),
            ..Default::default()
        };
        update(
            &mut state,
            NetworkMsg::ConnectionResult(Ok("Connected to Net".into())),
        );
        assert!(state.error_message.is_none());
        assert_eq!(state.status_message, "Connected to Net");
    }

    #[test]
    fn dismiss_error_clears_message() {
        let mut state = NetworkState {
            error_message: Some("fail".into()),
            ..Default::default()
        };
        update(&mut state, NetworkMsg::DismissError);
        assert!(state.error_message.is_none());
    }

    #[test]
    fn scan_result_error_sets_error_message() {
        let mut state = NetworkState {
            scanning: true,
            ..Default::default()
        };
        update(
            &mut state,
            NetworkMsg::WifiScanned(Err("nmcli not found".into())),
        );
        assert!(!state.scanning);
        assert_eq!(state.error_message.as_deref(), Some("nmcli not found"));
    }
}
