// Unit tests for the ClawPanel application logic.
//
// These live in a separate file to keep main.rs under the 500-line limit
// while still being able to test all the update_app branches.

use super::*;

#[test]
fn default_panel_is_hidden() {
    let panel = ClawPanel::default();
    assert!(!panel.visible);
}

#[test]
fn update_show_sets_visible() {
    let mut panel = ClawPanel::default();
    let _ = update_app(&mut panel, Message::Show);
    assert!(panel.visible);
}

#[test]
fn update_hide_clears_visible() {
    let mut panel = ClawPanel::default();
    panel.visible = true;
    let _ = update_app(&mut panel, Message::Hide);
    assert!(!panel.visible);
}

#[test]
fn update_toggle_flips_visibility() {
    let mut panel = ClawPanel::default();
    assert!(!panel.visible);

    let _ = update_app(&mut panel, Message::ToggleVisibility);
    assert!(panel.visible);

    let _ = update_app(&mut panel, Message::ToggleVisibility);
    assert!(!panel.visible);
}

#[test]
fn update_palette_changed_stores_palette() {
    let mut panel = ClawPanel::default();
    let new_palette = Palette {
        primary: [255, 0, 0, 255],
        ..Palette::default()
    };
    let _ = update_app(&mut panel, Message::PaletteChanged(new_palette.clone()));
    assert_eq!(panel.palette, new_palette);
}

#[test]
fn update_input_changed_stores_text() {
    let mut panel = ClawPanel::default();
    let _ = update_app(&mut panel, Message::InputChanged("hello".to_string()));
    assert_eq!(panel.input_text, "hello");
}

#[test]
fn update_send_on_empty_input_does_nothing() {
    let mut panel = ClawPanel::default();
    panel.input_text = "   ".to_string();
    let _ = update_app(&mut panel, Message::Send);
    assert!(panel.conversation.is_empty());
    assert!(!panel.is_streaming);
}

#[test]
fn update_send_dispatches_rhea_call_and_clears_input() {
    let mut panel = ClawPanel::default();
    panel.input_text = "test query".to_string();
    // The task returned is a D-Bus call; we cannot execute it without a
    // live session bus, but we verify that send begins streaming and clears
    // the input field.
    let _task = update_app(&mut panel, Message::Send);

    // One user message added, streaming started, input cleared.
    assert_eq!(panel.conversation.len(), 1);
    assert!(panel.is_streaming);
    assert!(panel.input_text.is_empty());
}

#[test]
fn update_rhea_send_result_err_stops_streaming_and_shows_error() {
    let mut panel = ClawPanel::default();
    panel.is_streaming = true;
    let _ = update_app(
        &mut panel,
        Message::RheaSendResult(Err("connection refused".to_string())),
    );
    assert!(!panel.is_streaming);
    assert_eq!(panel.conversation.len(), 1);
    assert!(panel.conversation.messages()[0]
        .content
        .contains("Rhea unavailable"));
}

#[test]
fn update_rhea_send_result_ok_keeps_streaming() {
    let mut panel = ClawPanel::default();
    panel.is_streaming = true;
    let _ = update_app(&mut panel, Message::RheaSendResult(Ok(())));
    // On Ok, streaming stays true until RheaDone arrives via DbusEvent.
    assert!(panel.is_streaming);
}

#[test]
fn update_resize_clamps_width() {
    let mut panel = ClawPanel::default();
    let _ = update_app(&mut panel, Message::Resize(100));
    assert_eq!(panel.panel_width, panel::MIN_WIDTH);

    let _ = update_app(&mut panel, Message::Resize(9999));
    assert_eq!(panel.panel_width, panel::MAX_WIDTH);

    let _ = update_app(&mut panel, Message::Resize(500));
    assert_eq!(panel.panel_width, 500);
}

#[test]
fn update_dbus_show_sets_visible() {
    let mut panel = ClawPanel::default();
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::Show),
    );
    assert!(panel.visible);
}

#[test]
fn update_dbus_toggle_flips_visibility() {
    let mut panel = ClawPanel::default();
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::Toggle),
    );
    assert!(panel.visible);

    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::Toggle),
    );
    assert!(!panel.visible);
}

#[test]
fn update_dbus_palette_updates_theme() {
    let mut panel = ClawPanel::default();
    let custom = Palette {
        primary: [0, 255, 0, 255],
        ..Palette::default()
    };
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::PaletteChanged(custom.clone())),
    );
    assert_eq!(panel.palette, custom);
}

#[test]
fn update_rhea_chunk_appends_text() {
    let mut panel = ClawPanel::default();
    panel.is_streaming = true;
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::RheaChunk("hello ".to_string())),
    );
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::RheaChunk("world".to_string())),
    );
    assert_eq!(panel.conversation.len(), 1);
    assert_eq!(panel.conversation.messages()[0].content, "hello world");
}

#[test]
fn update_rhea_done_stops_streaming() {
    let mut panel = ClawPanel::default();
    panel.is_streaming = true;
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::RheaDone),
    );
    assert!(!panel.is_streaming);
}

#[test]
fn update_rhea_error_stops_streaming_and_shows_error() {
    let mut panel = ClawPanel::default();
    panel.is_streaming = true;
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::RheaError("timeout".to_string())),
    );
    assert!(!panel.is_streaming);
    assert_eq!(panel.conversation.len(), 1);
}

#[test]
fn update_rhea_backend_changed_stores_name() {
    let mut panel = ClawPanel::default();
    assert!(panel.rhea_backend.is_empty());
    let _ = update_app(
        &mut panel,
        Message::DbusEvent(dbus_listener::DbusEvent::RheaBackendChanged(
            "local".to_string(),
        )),
    );
    assert_eq!(panel.rhea_backend, "local");
}

#[test]
fn update_context_updated_stores_context() {
    let mut panel = ClawPanel::default();
    let ctx = WindowContext {
        app_id: "firefox".to_string(),
        title: "GitHub".to_string(),
    };
    let _ = update_app(&mut panel, Message::ContextUpdated(Some(ctx.clone())));
    assert_eq!(panel.current_context, Some(ctx));
}

#[test]
fn update_panel_action_close_hides() {
    let mut panel = ClawPanel::default();
    panel.visible = true;
    let _ = update_app(&mut panel, Message::PanelAction(PanelAction::Close));
    assert!(!panel.visible);
}

#[test]
fn context_poll_interval_is_reasonable() {
    assert!(
        CONTEXT_POLL_SECS >= 1 && CONTEXT_POLL_SECS <= 10,
        "context poll interval should be between 1 and 10 seconds"
    );
}

#[test]
fn update_clipboard_result_ok_shows_success_toast() {
    let mut panel = ClawPanel::default();
    let _ = update_app(
        &mut panel,
        Message::ClipboardResult(Ok("Copied!".to_string())),
    );
    assert!(panel.toast_state.is_visible());
    let toast = panel.toast_state.current().expect("should have toast");
    assert_eq!(toast.message, "Copied!");
    assert_eq!(toast.kind, toast::ToastKind::Success);
}

#[test]
fn update_clipboard_result_err_shows_error_toast() {
    let mut panel = ClawPanel::default();
    let _ = update_app(
        &mut panel,
        Message::ClipboardResult(Err("wl-copy not found".to_string())),
    );
    assert!(panel.toast_state.is_visible());
    let toast = panel.toast_state.current().expect("should have toast");
    assert_eq!(toast.message, "wl-copy not found");
    assert_eq!(toast.kind, toast::ToastKind::Error);
}

#[test]
fn update_toast_tick_calls_tick() {
    let mut panel = ClawPanel::default();
    // Set an already-expired toast
    panel.toast_state.show_success("Gone".to_string());
    panel.toast_state = {
        let mut ts = ToastState::new();
        ts.show_success("test".to_string());
        ts
    };
    // Toast is visible right after creation
    assert!(panel.toast_state.is_visible());

    // ToastTick should not remove a non-expired toast
    let _ = update_app(&mut panel, Message::ToastTick);
    assert!(panel.toast_state.is_visible());
}

#[test]
fn update_apply_code_block_returns_task() {
    let mut panel = ClawPanel::default();
    let task = update_app(
        &mut panel,
        Message::PanelAction(PanelAction::ApplyCodeBlock("echo hello".to_string())),
    );
    // The task should be non-trivial (it performs clipboard copy).
    // We cannot easily inspect iced::Task internals, but we can
    // verify the function does not panic.
    let _ = task;
}

#[test]
fn default_panel_has_no_toast() {
    let panel = ClawPanel::default();
    assert!(!panel.toast_state.is_visible());
}
