/// Context menu for dock icons.
///
/// Displays a popup above a dock icon on right-click (mouse) or long-press
/// (touch) with actions like close window, pin/unpin, and close-all.
/// The menu dismisses when the user taps elsewhere.
use iced::widget::{button, column, container, mouse_area, text, Space};
use iced::{Color, Element, Length, Padding};

/// Which actions are available depends on the icon's running/pinned state.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuState {
    /// Desktop ID of the app whose menu is open.
    pub desktop_id: String,
    /// Human-readable name shown at the top of the menu.
    pub app_name: String,
    /// Whether the app has at least one open window.
    pub is_running: bool,
    /// Whether the app is in the pinned favourites list.
    pub is_pinned: bool,
}

/// Actions the user can trigger from the context menu.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    /// Close the focused window of this app.
    CloseWindow(String),
    /// Close every window belonging to this app.
    CloseAllWindows(String),
    /// Add this running app to the pinned favourites.
    KeepInDock(String),
    /// Remove this app from the pinned favourites.
    RemoveFromDock(String),
}

/// Build the list of menu items for the given state.
///
/// The order matches user expectations: close actions first (most likely
/// intent on a running app), then pin/unpin.
pub fn menu_items(state: &MenuState) -> Vec<MenuAction> {
    let mut items = Vec::new();

    if state.is_running {
        items.push(MenuAction::CloseWindow(state.desktop_id.clone()));
        items.push(MenuAction::CloseAllWindows(state.desktop_id.clone()));
    }

    if state.is_pinned {
        items.push(MenuAction::RemoveFromDock(state.desktop_id.clone()));
    } else if state.is_running {
        // Only non-pinned running apps can be pinned (you can't pin something
        // that isn't visible in the dock).
        items.push(MenuAction::KeepInDock(state.desktop_id.clone()));
    }

    items
}

/// Human-readable label for a menu action.
pub fn action_label(action: &MenuAction) -> &'static str {
    match action {
        MenuAction::CloseWindow(_) => "Close Window",
        MenuAction::CloseAllWindows(_) => "Close All Windows",
        MenuAction::KeepInDock(_) => "Keep in Dock",
        MenuAction::RemoveFromDock(_) => "Remove from Dock",
    }
}

/// Render the context menu popup as an iced Element.
///
/// The entire menu is wrapped in a mouse_area so a tap outside triggers
/// `on_dismiss`. Each item triggers its own message via `on_action`.
pub fn view_menu<'a, M: Clone + 'a>(
    state: &'a MenuState,
    on_action: impl Fn(MenuAction) -> M + 'a,
    on_dismiss: M,
) -> Element<'a, M> {
    let items = menu_items(state);

    if items.is_empty() {
        return Space::new(0, 0).into();
    }

    let mut col = column![].spacing(2).width(Length::Fixed(180.0));

    // App name header
    col = col.push(
        container(
            text(&state.app_name)
                .size(13)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.6)),
        )
        .padding(Padding::from([6, 12])),
    );

    for item in &items {
        let label = action_label(item);
        let action_msg = on_action(item.clone());

        let btn = button(
            text(label)
                .size(14)
                .color(Color::WHITE)
                .align_x(iced::alignment::Horizontal::Left),
        )
        .on_press(action_msg)
        .width(Length::Fill)
        .padding(Padding::from([8, 12]))
        .style(|_theme: &iced::Theme, status| {
            let bg_alpha = match status {
                button::Status::Hovered | button::Status::Pressed => 0.15,
                _ => 0.0,
            };
            button::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    1.0, 1.0, 1.0, bg_alpha,
                ))),
                border: iced::Border::default(),
                text_color: Color::WHITE,
                ..Default::default()
            }
        });

        col = col.push(btn);
    }

    let popup = container(col)
        .padding(Padding::from([4, 0]))
        .style(|_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.12, 0.12, 0.16, 0.95,
            ))),
            border: iced::Border {
                radius: 12.0.into(),
                width: 1.0,
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            },
            ..Default::default()
        });

    // Wrap in a mouse_area to detect clicks outside the menu
    mouse_area(popup).on_press(on_dismiss).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_pinned_state() -> MenuState {
        MenuState {
            desktop_id: "firefox".into(),
            app_name: "Firefox".into(),
            is_running: true,
            is_pinned: true,
        }
    }

    fn running_unpinned_state() -> MenuState {
        MenuState {
            desktop_id: "nautilus".into(),
            app_name: "Files".into(),
            is_running: true,
            is_pinned: false,
        }
    }

    fn pinned_not_running_state() -> MenuState {
        MenuState {
            desktop_id: "Alacritty".into(),
            app_name: "Alacritty".into(),
            is_running: false,
            is_pinned: true,
        }
    }

    #[test]
    fn running_pinned_app_has_close_and_remove() {
        let items = menu_items(&running_pinned_state());
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], MenuAction::CloseWindow(_)));
        assert!(matches!(&items[1], MenuAction::CloseAllWindows(_)));
        assert!(matches!(&items[2], MenuAction::RemoveFromDock(_)));
    }

    #[test]
    fn running_unpinned_app_has_close_and_keep() {
        let items = menu_items(&running_unpinned_state());
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], MenuAction::CloseWindow(_)));
        assert!(matches!(&items[1], MenuAction::CloseAllWindows(_)));
        assert!(matches!(&items[2], MenuAction::KeepInDock(_)));
    }

    #[test]
    fn pinned_not_running_app_has_only_remove() {
        let items = menu_items(&pinned_not_running_state());
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], MenuAction::RemoveFromDock(_)));
    }

    #[test]
    fn menu_items_preserves_desktop_id() {
        let state = running_pinned_state();
        let items = menu_items(&state);
        for item in &items {
            let id = match item {
                MenuAction::CloseWindow(id) => id,
                MenuAction::CloseAllWindows(id) => id,
                MenuAction::KeepInDock(id) => id,
                MenuAction::RemoveFromDock(id) => id,
            };
            assert_eq!(id, "firefox");
        }
    }

    #[test]
    fn action_labels_are_nonempty() {
        let state = running_pinned_state();
        for item in &menu_items(&state) {
            let label = action_label(item);
            assert!(!label.is_empty(), "label should not be empty");
        }
    }

    #[test]
    fn empty_state_yields_no_items() {
        // An app that is neither running nor pinned should not appear in the
        // dock at all, but if it does the menu should be empty.
        let state = MenuState {
            desktop_id: "ghost".into(),
            app_name: "Ghost".into(),
            is_running: false,
            is_pinned: false,
        };
        assert!(menu_items(&state).is_empty());
    }

    #[test]
    fn view_menu_does_not_panic() {
        let state = running_pinned_state();
        // Ensure the view function can build without panicking
        let _: Element<'_, String> = view_menu(
            &state,
            |action| format!("{action:?}"),
            "dismiss".to_string(),
        );
    }
}
