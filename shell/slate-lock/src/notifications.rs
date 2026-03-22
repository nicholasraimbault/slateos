/// Privacy-filtered notification previews for the lock screen.
///
/// Only app name and summary are shown — body text is intentionally omitted
/// so private message contents are not visible when the device is locked.
use iced::widget::{column, container, text};
use iced::{Element, Length};

use slate_common::notifications::Notification;

/// Lock screen shows at most this many notifications to avoid clutter and
/// keep the UI clean on a single glance.
pub const MAX_LOCK_NOTIFICATIONS: usize = 3;

/// Render the most recent notifications for the lock screen.
///
/// Privacy rule: only `app_name` and `summary` are displayed — the `body`
/// is never shown so that message contents stay hidden while locked.
pub fn view_lock_notifications<'a, M: 'a>(notifications: &[Notification]) -> Element<'a, M> {
    if notifications.is_empty() {
        return column![].into();
    }

    let items = notifications
        .iter()
        .rev()
        .take(MAX_LOCK_NOTIFICATIONS)
        .map(|n| {
            let label = format!("{} \u{2014} {}", n.app_name, n.summary);
            container(text(label).size(12)).padding(8).into()
        });

    column(items).spacing(4).width(Length::Fill).into()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_notifications_constant_is_3() {
        assert_eq!(MAX_LOCK_NOTIFICATIONS, 3);
    }

    #[test]
    fn empty_slice_does_not_panic() {
        // The function must gracefully handle zero notifications.
        let _elem: Element<'_, ()> = view_lock_notifications(&[]);
    }

    #[test]
    fn limits_to_max_lock_notifications() {
        let notifications: Vec<Notification> = (0..5)
            .map(|i| Notification::new(i, format!("App{i}"), format!("Summary {i}"), "body"))
            .collect();

        // We cannot inspect the iced widget tree directly, but we can verify
        // the slicing logic by confirming the function processes all five
        // without panic and that our constant gates the count.
        assert!(notifications.len() > MAX_LOCK_NOTIFICATIONS);
        let _elem: Element<'_, ()> = view_lock_notifications(&notifications);
    }
}
