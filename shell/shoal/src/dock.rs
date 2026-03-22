/// Dock UI for Shoal.
///
/// Renders the horizontal dock bar with pinned and running app icons,
/// magnification effects, running/focused indicators, and notification badges.
/// Uses iced widgets for layout and styling.
use std::collections::HashMap;

use iced::widget::{button, column, container, stack, text, Row, Space};
use iced::{Alignment, Color, Element, Length, Padding};

use crate::desktop::DesktopEntry;
use crate::magnification;
use crate::windows::RunningApp;

/// Base icon size in logical pixels.
pub const BASE_ICON_SIZE: f64 = 44.0;

/// Maximum magnification scale.
pub const MAX_SCALE: f64 = 1.5;

/// Gaussian spread for magnification falloff.
pub const SPREAD: f64 = 80.0;

/// Gap between icons in logical pixels.
pub const ICON_GAP: f64 = 4.0;

/// Height of the dock exclusive zone.
pub const DOCK_HEIGHT: u32 = 56;

/// Separator width in logical pixels.
const SEPARATOR_WIDTH: f32 = 1.0;

/// An icon in the dock with its metadata and visual state.
#[derive(Debug, Clone)]
#[allow(dead_code)] // is_pinned is used by context_menu logic and tests
pub struct DockIcon {
    pub entry: DesktopEntry,
    pub is_running: bool,
    pub is_focused: bool,
    pub is_pinned: bool,
    pub scale: f64,
}

/// Build the list of dock icons by merging pinned apps, running apps, and
/// magnification state.
pub fn build_dock_icons(
    pinned: &[DesktopEntry],
    running: &[RunningApp],
    all_entries: &[DesktopEntry],
    touch_x: Option<f64>,
) -> (Vec<DockIcon>, Vec<DockIcon>) {
    // Build pinned icons with running/focused state
    let mut pinned_icons: Vec<DockIcon> = pinned
        .iter()
        .map(|entry| {
            let running_match = running.iter().find(|r| app_ids_match(&r.app_id, entry));
            DockIcon {
                entry: entry.clone(),
                is_running: running_match.is_some(),
                is_focused: running_match.map(|r| r.is_focused).unwrap_or(false),
                is_pinned: true,
                scale: 1.0,
            }
        })
        .collect();

    // Running apps not in the pinned list
    let mut dynamic_icons: Vec<DockIcon> = running
        .iter()
        .filter(|r| !pinned.iter().any(|p| app_ids_match(&r.app_id, p)))
        .map(|r| {
            // Try to find a desktop entry for this running app
            let entry = all_entries
                .iter()
                .find(|e| app_ids_match(&r.app_id, e))
                .cloned()
                .unwrap_or_else(|| DesktopEntry {
                    name: r.app_id.clone(),
                    exec: String::new(),
                    icon: String::new(),
                    desktop_id: r.app_id.clone(),
                });

            DockIcon {
                entry,
                is_running: true,
                is_focused: r.is_focused,
                is_pinned: false,
                scale: 1.0,
            }
        })
        .collect();

    // Calculate magnification scales across all icons
    let all_count = pinned_icons.len() + dynamic_icons.len();
    if all_count > 0 {
        let icon_centers: Vec<f64> = (0..all_count)
            .map(|i| BASE_ICON_SIZE / 2.0 + i as f64 * (BASE_ICON_SIZE + ICON_GAP))
            .collect();

        let scales = magnification::calculate_scales(
            touch_x,
            &icon_centers,
            BASE_ICON_SIZE,
            MAX_SCALE,
            SPREAD,
        );

        for (i, icon) in pinned_icons.iter_mut().enumerate() {
            icon.scale = scales[i];
        }
        for (i, icon) in dynamic_icons.iter_mut().enumerate() {
            icon.scale = scales[pinned_icons.len() + i];
        }
    }

    (pinned_icons, dynamic_icons)
}

/// Check whether a Wayland app_id matches a desktop entry.
///
/// Comparison is case-insensitive because Wayland app_ids and desktop
/// filenames don't always agree on casing.
fn app_ids_match(app_id: &str, entry: &DesktopEntry) -> bool {
    app_id.eq_ignore_ascii_case(&entry.desktop_id)
}

/// Determine the badge count for a dock icon by matching against notification counts.
///
/// First tries an exact match on `desktop_id`, then falls back to a
/// case-insensitive match on the last dot-separated component (the "stem"),
/// so that `org.gnome.Nautilus` matches a notification from app "Nautilus".
pub fn get_badge_count(icon: &DockIcon, counts: &HashMap<String, u32>) -> u32 {
    if let Some(&count) = counts.get(&icon.entry.desktop_id) {
        return count;
    }
    let stem = icon
        .entry
        .desktop_id
        .rsplit('.')
        .next()
        .unwrap_or(&icon.entry.desktop_id);
    for (app_name, &count) in counts {
        if app_name.eq_ignore_ascii_case(stem) {
            return count;
        }
    }
    0
}

/// Render a single dock icon as an iced Element.
///
/// `badge_count` is the number of unread notifications to overlay; 0 means no badge.
pub fn view_icon<'a, M: Clone + 'a>(
    icon: &DockIcon,
    badge_count: u32,
    on_tap: M,
    _on_long_press: M,
) -> Element<'a, M> {
    let size = (BASE_ICON_SIZE * icon.scale) as f32;

    // Icon placeholder: first letter of app name in a rounded square
    let label = icon.entry.name.chars().next().unwrap_or('?').to_string();

    let icon_content = container(
        text(label)
            .size(size * 0.5)
            .color(Color::WHITE)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .center_x(Length::Fixed(size))
    .center_y(Length::Fixed(size))
    .style(move |_theme: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.12,
        ))),
        border: iced::Border {
            radius: (size * 0.22).into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Badge overlay: a small red circle in the top-right corner with count text.
    // Rendered only when count > 0 so the layout is unchanged for clean icons.
    let icon_with_badge: Element<'a, M> = if badge_count > 0 {
        let badge_size = (size * 0.38).max(14.0);
        let badge_label = if badge_count > 99 {
            "99+".to_string()
        } else {
            badge_count.to_string()
        };
        let badge = container(
            text(badge_label)
                .size(badge_size * 0.55)
                .color(Color::WHITE)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .width(Length::Fixed(badge_size))
        .height(Length::Fixed(badge_size))
        .center_x(Length::Fixed(badge_size))
        .center_y(Length::Fixed(badge_size))
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.94, 0.19, 0.19))),
            border: iced::Border {
                radius: (badge_size / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        });

        // Stack the badge on top of the icon, aligned to the top-right corner.
        stack![
            icon_content,
            container(badge)
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .align_right(Length::Fixed(size))
                .align_top(Length::Fixed(size))
        ]
        .into()
    } else {
        icon_content.into()
    };

    // Running indicator dot
    let indicator: Element<'a, M> = if icon.is_running {
        let dot_color = if icon.is_focused {
            Color::WHITE
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.5)
        };
        container(Space::new(4, 4))
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(dot_color)),
                border: iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    } else {
        // Invisible spacer to keep alignment consistent
        Space::new(4, 4).into()
    };

    let col = column![icon_with_badge, indicator]
        .spacing(2)
        .align_x(Alignment::Center);

    button(col)
        .on_press(on_tap)
        .padding(Padding::ZERO)
        .style(|_theme: &iced::Theme, _status| button::Style {
            background: None,
            border: iced::Border::default(),
            text_color: Color::WHITE,
            ..Default::default()
        })
        .into()
}

/// Render a thin vertical separator between pinned and dynamic sections.
pub fn view_separator<'a, M: 'a>() -> Element<'a, M> {
    container(Space::new(SEPARATOR_WIDTH, 36.0))
        .style(|_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                1.0, 1.0, 1.0, 0.2,
            ))),
            border: iced::Border {
                radius: 0.5.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .center_y(Length::Shrink)
        .into()
}

/// Render the entire dock bar as a pill-shaped container.
pub fn view_dock<'a, M: Clone + 'a>(
    pinned_icons: &[DockIcon],
    dynamic_icons: &[DockIcon],
    notification_counts: &HashMap<String, u32>,
    make_tap_msg: impl Fn(String) -> M,
    make_long_press_msg: impl Fn(String) -> M,
) -> Element<'a, M> {
    let mut dock_row: Row<'a, M> = Row::new().spacing(ICON_GAP as f32).align_y(Alignment::End);

    // Pinned section
    for icon in pinned_icons {
        let id = icon.entry.desktop_id.clone();
        let id2 = id.clone();
        let badge = get_badge_count(icon, notification_counts);
        dock_row = dock_row.push(view_icon(
            icon,
            badge,
            make_tap_msg(id),
            make_long_press_msg(id2),
        ));
    }

    // Separator (only if there are dynamic icons)
    if !dynamic_icons.is_empty() {
        dock_row = dock_row.push(view_separator());

        for icon in dynamic_icons {
            let id = icon.entry.desktop_id.clone();
            let id2 = id.clone();
            let badge = get_badge_count(icon, notification_counts);
            dock_row = dock_row.push(view_icon(
                icon,
                badge,
                make_tap_msg(id),
                make_long_press_msg(id2),
            ));
        }
    }

    // Pill-shaped background
    container(dock_row)
        .padding(Padding::from([6, 12]))
        .center_x(Length::Shrink)
        .style(|_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.0, 0.0, 0.0, 0.08,
            ))),
            border: iced::Border {
                radius: 20.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned_entries() -> Vec<DesktopEntry> {
        vec![
            DesktopEntry {
                name: "Alacritty".into(),
                exec: "alacritty".into(),
                icon: "alacritty".into(),
                desktop_id: "Alacritty".into(),
            },
            DesktopEntry {
                name: "Firefox".into(),
                exec: "firefox".into(),
                icon: "firefox".into(),
                desktop_id: "firefox".into(),
            },
        ]
    }

    #[test]
    fn build_dock_icons_pinned_only() {
        let pinned = pinned_entries();
        let (p, d) = build_dock_icons(&pinned, &[], &pinned, None);
        assert_eq!(p.len(), 2);
        assert!(d.is_empty());
        assert!(!p[0].is_running);
        assert!(!p[1].is_running);
    }

    #[test]
    fn build_dock_icons_with_running_pinned() {
        let pinned = pinned_entries();
        let running = vec![RunningApp {
            app_id: "firefox".into(),
            title: "Mozilla Firefox".into(),
            is_focused: true,
        }];
        let (p, d) = build_dock_icons(&pinned, &running, &pinned, None);
        assert_eq!(p.len(), 2);
        assert!(d.is_empty());
        assert!(!p[0].is_running);
        assert!(p[1].is_running);
        assert!(p[1].is_focused);
    }

    #[test]
    fn build_dock_icons_with_dynamic_app() {
        let pinned = pinned_entries();
        let running = vec![RunningApp {
            app_id: "nautilus".into(),
            title: "Files".into(),
            is_focused: false,
        }];
        let all_entries = vec![DesktopEntry {
            name: "Files".into(),
            exec: "nautilus".into(),
            icon: "nautilus".into(),
            desktop_id: "nautilus".into(),
        }];
        let (p, d) = build_dock_icons(&pinned, &running, &all_entries, None);
        assert_eq!(p.len(), 2);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].entry.desktop_id, "nautilus");
    }

    #[test]
    fn app_ids_match_case_insensitive() {
        let entry = DesktopEntry {
            name: "Alacritty".into(),
            exec: "alacritty".into(),
            icon: "alacritty".into(),
            desktop_id: "Alacritty".into(),
        };
        assert!(app_ids_match("Alacritty", &entry));
        assert!(app_ids_match("alacritty", &entry));
        assert!(app_ids_match("ALACRITTY", &entry));
        assert!(!app_ids_match("firefox", &entry));
    }

    fn make_icon(desktop_id: &str) -> DockIcon {
        DockIcon {
            entry: DesktopEntry {
                name: desktop_id.to_string(),
                exec: desktop_id.to_lowercase(),
                icon: String::new(),
                desktop_id: desktop_id.to_string(),
            },
            is_running: false,
            is_focused: false,
            is_pinned: true,
            scale: 1.0,
        }
    }

    #[test]
    fn get_badge_count_exact_match() {
        let icon = make_icon("slack");
        let mut counts = HashMap::new();
        counts.insert("slack".to_string(), 7);
        assert_eq!(get_badge_count(&icon, &counts), 7);
    }

    #[test]
    fn get_badge_count_no_match_returns_zero() {
        let icon = make_icon("firefox");
        let mut counts = HashMap::new();
        counts.insert("slack".to_string(), 3);
        assert_eq!(get_badge_count(&icon, &counts), 0);
    }

    #[test]
    fn get_badge_count_empty_counts_returns_zero() {
        let icon = make_icon("firefox");
        let counts = HashMap::new();
        assert_eq!(get_badge_count(&icon, &counts), 0);
    }

    #[test]
    fn get_badge_count_stem_fallback_case_insensitive() {
        // desktop_id "org.gnome.Nautilus" should match notification app "Nautilus"
        let icon = make_icon("org.gnome.Nautilus");
        let mut counts = HashMap::new();
        counts.insert("nautilus".to_string(), 2);
        assert_eq!(get_badge_count(&icon, &counts), 2);
    }

    #[test]
    fn get_badge_count_stem_fallback_mixed_case() {
        let icon = make_icon("com.example.MyApp");
        let mut counts = HashMap::new();
        counts.insert("MYAPP".to_string(), 5);
        assert_eq!(get_badge_count(&icon, &counts), 5);
    }

    #[test]
    fn get_badge_count_exact_match_takes_priority_over_stem() {
        // When both exact and stem match exist, exact should win
        let icon = make_icon("com.slack.Slack");
        let mut counts = HashMap::new();
        counts.insert("com.slack.Slack".to_string(), 10);
        counts.insert("Slack".to_string(), 3);
        assert_eq!(get_badge_count(&icon, &counts), 10);
    }

    #[test]
    fn get_badge_count_simple_id_no_dots() {
        // desktop_id with no dots: stem equals desktop_id itself
        let icon = make_icon("firefox");
        let mut counts = HashMap::new();
        counts.insert("Firefox".to_string(), 4);
        assert_eq!(get_badge_count(&icon, &counts), 4);
    }
}
