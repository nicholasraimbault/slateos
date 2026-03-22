// Update logic for slate-shade.
//
// All message-handling functions live here so that main.rs stays focused on
// wiring (types, iced Application impl, entry point) and stays under 500 lines.

use iced::Task;

use crate::actions;
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
        Message::NotifAction(action) => return handle_notif_action(app, action),
        Message::QsAction(action) => handle_qs_action(app, action),
        Message::HeadsUpAction(action) => return handle_hun_action(app, action),
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
        // D-Bus call results — local state was already updated optimistically,
        // so we only need to log errors here.
        Message::DismissResult(Err(e)) => {
            tracing::warn!("slate-shade: Dismiss D-Bus call failed: {e}");
        }
        Message::DismissResult(Ok(())) => {}
        Message::DismissGroupResult(Err(e)) => {
            tracing::warn!("slate-shade: DismissGroup D-Bus call failed: {e}");
        }
        Message::DismissGroupResult(Ok(())) => {}
        Message::InvokeActionResult(Err(e)) => {
            tracing::warn!("slate-shade: InvokeAction D-Bus call failed: {e}");
        }
        Message::InvokeActionResult(Ok(())) => {}
        Message::SmartRepliesResult(uuid, replies) => {
            if !replies.is_empty() {
                app.smart_replies.insert(uuid, replies);
            }
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

fn handle_notif_action(app: &mut SlateShade, action: NotifAction) -> Task<Message> {
    match action {
        NotifAction::Dismiss(uuid) => {
            // Optimistically remove from local state so the UI feels instant.
            // The D-Bus call confirms the dismissal to notifyd; if it fails,
            // notifyd's next signal will re-sync the shade.
            remove_notification(&mut app.groups, uuid);
            app.smart_replies.remove(&uuid);
            Task::perform(
                actions::dbus_dismiss(uuid.to_string()),
                Message::DismissResult,
            )
        }
        NotifAction::DismissGroup(app_name) => {
            // Optimistically remove the whole group so the shade updates
            // immediately, then tell notifyd to persist the dismissal.
            remove_group(&mut app.groups, &app_name);
            Task::perform(
                actions::dbus_dismiss_group(app_name),
                Message::DismissGroupResult,
            )
        }
        NotifAction::ToggleGroup(app_name) => {
            toggle_group(&mut app.groups, &app_name);
            Task::none()
        }
        NotifAction::InvokeAction(uuid, key) => {
            // notifyd forwards the action to the originating app via the
            // freedesktop ActionInvoked signal; no local state change needed.
            Task::perform(
                actions::dbus_invoke_action(uuid.to_string(), key),
                Message::InvokeActionResult,
            )
        }
        NotifAction::SendReply(uuid, reply) => {
            // Dispatch the reply text to Rhea.SuggestReplies so we can
            // pre-populate the reply chips for the conversation thread.
            Task::perform(
                actions::dbus_suggest_replies(uuid, reply),
                |(id, replies)| Message::SmartRepliesResult(id, replies),
            )
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

fn handle_hun_action(app: &mut SlateShade, action: HeadsUpAction) -> Task<Message> {
    match action {
        HeadsUpAction::Dismiss => {
            app.hun.dismiss();
            Task::none()
        }
        HeadsUpAction::InvokeAction(uuid, key) => {
            // Dismiss the banner immediately so the UI feels responsive,
            // then forward the action invocation to notifyd.
            app.hun.dismiss();
            Task::perform(
                actions::dbus_invoke_action(uuid.to_string(), key),
                Message::InvokeActionResult,
            )
        }
    }
}
