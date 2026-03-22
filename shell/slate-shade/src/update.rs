// Update logic for slate-shade.
//
// All message-handling functions live here so that main.rs stays focused on
// wiring (types, iced Application impl, entry point) and stays under 500 lines.

use iced::Task;

use crate::notifications::{
    remove_group, remove_notification, set_ai_summary, toggle_group, upsert_notification,
};
use crate::{
    dbus_listener, HeadsUpAction, Message, NotifAction, QsAction, SlateShade, ANIM_TICK_MS,
};

/// Top-level message dispatcher.
pub(super) fn update_app(app: &mut SlateShade, message: Message) -> Task<Message> {
    match message {
        Message::DbusEvent(event) => handle_dbus_event(app, event),
        Message::Open => {
            app.anim.open();
        }
        Message::Close => {
            app.anim.close();
        }
        Message::NotifAction(action) => handle_notif_action(app, action),
        Message::QsAction(action) => handle_qs_action(app, action),
        Message::HeadsUpAction(action) => handle_hun_action(app, action),
        Message::AnimTick => {
            if !app.anim.is_settled() {
                app.anim.step(ANIM_TICK_MS as f64 / 1000.0);
            }
        }
        Message::HunTick => {
            app.hun.tick();
        }
        Message::LayoutDetected(mode) => {
            app.layout = mode;
        }
    }
    Task::none()
}

fn handle_dbus_event(app: &mut SlateShade, event: dbus_listener::DbusEvent) {
    match event {
        dbus_listener::DbusEvent::NotificationAdded(notif) => {
            if notif.heads_up {
                app.hun.show(notif.clone());
            }
            upsert_notification(&mut app.groups, notif);
        }
        dbus_listener::DbusEvent::NotificationUpdated(notif) => {
            upsert_notification(&mut app.groups, notif);
        }
        dbus_listener::DbusEvent::NotificationDismissed(uuid) => {
            remove_notification(&mut app.groups, uuid);
            app.smart_replies.remove(&uuid);
        }
        dbus_listener::DbusEvent::GroupChanged(app_name, count) => {
            if count == 0 {
                remove_group(&mut app.groups, &app_name);
            }
        }
        dbus_listener::DbusEvent::RheaCompletionChunk(chunk) => {
            // Streaming chunks are not yet surfaced in the UI; log for diagnostics.
            tracing::debug!("slate-shade: Rhea chunk ({} bytes)", chunk.len());
        }
        dbus_listener::DbusEvent::RheaCompletionDone(full_text) => {
            // Route the completed Rhea response to the group that requested it.
            // We pop the oldest pending entry so that concurrent requests are
            // fulfilled in the same order they were sent.
            //
            // TODO: a proper correlation ID in the Rhea protocol would be the
            // ideal long-term fix (avoids all ordering assumptions entirely).
            tracing::debug!("slate-shade: Rhea done ({} bytes)", full_text.len());
            if let Some(app_name) = app.pending_summaries.pop_front() {
                set_ai_summary(&mut app.groups, &app_name, full_text);
            } else {
                tracing::warn!(
                    "slate-shade: RheaCompletionDone arrived but no pending summary request found"
                );
            }
        }
        dbus_listener::DbusEvent::RheaCompletionError(error) => {
            tracing::warn!("slate-shade: Rhea error: {error}");
        }
        dbus_listener::DbusEvent::EdgeGesture {
            phase,
            progress,
            velocity,
        } => match phase.as_str() {
            "begin" | "update" => {
                app.anim.set_from_gesture(progress, velocity);
            }
            "end" => {
                app.anim.set_from_gesture(progress, velocity);
                app.anim.commit_gesture();
            }
            "cancel" => {
                app.anim.close();
            }
            _ => {}
        },
        dbus_listener::DbusEvent::PaletteChanged(palette) => {
            app.palette = palette;
        }
    }
}

fn handle_notif_action(app: &mut SlateShade, action: NotifAction) {
    match action {
        NotifAction::Dismiss(uuid) => {
            remove_notification(&mut app.groups, uuid);
            app.smart_replies.remove(&uuid);
            // TODO: call org.slate.Notifications.Dismiss(uuid) via D-Bus.
        }
        NotifAction::DismissGroup(app_name) => {
            remove_group(&mut app.groups, &app_name);
            // TODO: call org.slate.Notifications.DismissGroup(app_name) via D-Bus.
        }
        NotifAction::ToggleGroup(app_name) => {
            toggle_group(&mut app.groups, &app_name);
        }
        NotifAction::InvokeAction(_uuid, _key) => {
            // TODO: call org.slate.Notifications.InvokeAction(uuid, action_key) via D-Bus.
        }
        NotifAction::SendReply(_uuid, _reply) => {
            // Smart reply dispatch would go here.
        }
    }
}

fn handle_qs_action(app: &mut SlateShade, action: QsAction) {
    match action {
        QsAction::TileToggled(kind) => {
            app.qs.toggle_tile(kind);
        }
        QsAction::BrightnessChanged(v) => {
            app.qs.set_brightness(v);
        }
        QsAction::VolumeChanged(v) => {
            app.qs.set_volume(v);
        }
    }
}

fn handle_hun_action(app: &mut SlateShade, action: HeadsUpAction) {
    match action {
        HeadsUpAction::Dismiss => {
            app.hun.dismiss();
        }
        HeadsUpAction::InvokeAction(_uuid, _key) => {
            app.hun.dismiss();
            // TODO: call org.slate.Notifications.InvokeAction(uuid, action_key) via D-Bus.
        }
    }
}
