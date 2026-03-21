// View rendering for the launcher UI.
//
// Builds the iced Element tree for the fullscreen launcher overlay, including
// the search bar, optional "Recent" section, and the main app grid. Extracted
// from launcher.rs to keep individual files under the 500-line limit.

use crate::apps::AppEntry;
use crate::feedback::LaunchFeedback;
use crate::launcher::Message;
use crate::recent::RecentApps;
use crate::search;

use iced::widget::{column, container, row, scrollable, text, text_input, Column, Row};
use iced::{Element, Length};
use iced_anim::AnimationBuilder;

use slate_common::layout::LayoutParams;

/// Build the visible launcher view (search bar + recent section + app grid).
///
/// Performs the recent/main split internally so the caller does not need to
/// hold temporary Vec references across the view borrow.
pub fn build_view<'a>(
    search_query: &'a str,
    apps: &'a [AppEntry],
    recent: &'a RecentApps,
    layout: &LayoutParams,
    feedback: &'a LaunchFeedback,
    is_shell_mode: bool,
) -> Element<'a, Message> {
    let search_bar = build_search_bar(search_query, is_shell_mode);

    let columns = (layout.launcher_columns as usize).max(1);
    let icon_size = layout.launcher_icon_size as f32;
    let gap = layout.launcher_gap as f32;

    // Compute filtered list and recent/main split right here so the
    // references borrow from `apps` (which lives in Launcher state).
    let filtered: Vec<&AppEntry> = if is_shell_mode {
        Vec::new()
    } else {
        search::search_apps(search_query, apps)
    };

    let has_query = !search_query.is_empty();

    let (recent_refs, main_refs) = if has_query {
        // During search, hide the recent section entirely.
        (Vec::new(), filtered)
    } else {
        let recent_set = recent.recent_id_set();
        let recent_ids = recent.recent_ids();

        let recent_refs: Vec<&AppEntry> = recent_ids
            .iter()
            .filter_map(|id| apps.iter().find(|a| a.desktop_id == *id))
            .collect();

        let main_refs: Vec<&AppEntry> = filtered
            .into_iter()
            .filter(|a| !recent_set.contains_key(a.desktop_id.as_str()))
            .collect();

        (recent_refs, main_refs)
    };

    let grid_column = build_grid_with_sections(
        apps,
        &recent_refs,
        &main_refs,
        columns,
        icon_size,
        gap,
        feedback,
    );

    let content = column![search_bar, scrollable(grid_column).height(Length::Fill),]
        .width(Length::Fill)
        .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(20)
        .into()
}

/// Build the search bar row.
fn build_search_bar<'a>(query: &'a str, is_shell_mode: bool) -> Row<'a, Message> {
    let placeholder = if is_shell_mode {
        "Run command..."
    } else {
        "Search apps..."
    };

    let prefix = if is_shell_mode {
        text("Run: ").size(18)
    } else {
        text("").size(18)
    };

    row![
        prefix,
        text_input(placeholder, query)
            .on_input(Message::SearchChanged)
            .size(18)
            .padding(12)
            .width(Length::Fill),
    ]
    .spacing(8)
    .padding(20)
    .align_y(iced::Alignment::Center)
}

/// Build the full grid content: optional "Recent" section header + recent row,
/// then an "All Apps" header + the remaining apps grid.
fn build_grid_with_sections<'a>(
    all_apps: &'a [AppEntry],
    recent_apps: &[&'a AppEntry],
    main_apps: &[&'a AppEntry],
    columns: usize,
    icon_size: f32,
    gap: f32,
    feedback: &'a LaunchFeedback,
) -> Column<'a, Message> {
    // Both sections empty: show an appropriate empty-state message.
    if recent_apps.is_empty() && main_apps.is_empty() {
        let label = if all_apps.is_empty() {
            "No apps installed"
        } else {
            "No results"
        };

        return Column::new().spacing(gap).padding(20.0).push(
            container(text(label).size(18))
                .width(Length::Fill)
                .center_x(Length::Fill),
        );
    }

    let mut col = Column::new().spacing(gap).padding(20.0);

    // Recent section (only when there are recent apps and no active search).
    if !recent_apps.is_empty() {
        col = col.push(
            text("Recent")
                .size(14)
                .color(iced::Color::from_rgba8(180, 180, 190, 0.8)),
        );

        col = append_app_rows(col, recent_apps, columns, icon_size, gap, feedback, 0);

        // Spacing before the main grid.
        col = col.push(
            text("All Apps")
                .size(14)
                .color(iced::Color::from_rgba8(180, 180, 190, 0.8)),
        );
    }

    // Main app grid (apps not in the recent section).
    let offset = recent_apps.len();
    col = append_app_rows(col, main_apps, columns, icon_size, gap, feedback, offset);

    col
}

/// Append rows of app cells to `col`, starting global indices at `index_offset`.
///
/// `index_offset` ensures that apps in the recent section get indices
/// [0..recent.len()) and main-grid apps get [recent.len()..total), so
/// `Message::LaunchApp(idx)` can unambiguously identify the tapped app.
fn append_app_rows<'a>(
    mut col: Column<'a, Message>,
    apps: &[&'a AppEntry],
    columns: usize,
    icon_size: f32,
    gap: f32,
    feedback: &'a LaunchFeedback,
    index_offset: usize,
) -> Column<'a, Message> {
    for (row_idx, chunk) in apps.chunks(columns).enumerate() {
        let mut row_widget: Row<'a, Message> = Row::new().spacing(gap);

        for (col_idx, app) in chunk.iter().enumerate() {
            let global_idx = index_offset + row_idx * columns + col_idx;
            let target_opacity = feedback.opacity_for(&app.desktop_id);

            let cell = build_app_cell(app, global_idx, icon_size, target_opacity);
            row_widget = row_widget.push(cell);
        }

        // Pad short rows so cells keep consistent width.
        if chunk.len() < columns {
            for _ in 0..(columns - chunk.len()) {
                row_widget = row_widget.push(
                    container(text(""))
                        .width(Length::FillPortion(1))
                        .height(Length::Shrink),
                );
            }
        }

        col = col.push(row_widget);
    }

    col
}

/// Build a single app cell with animated opacity feedback.
///
/// Uses `AnimationBuilder` to spring-animate the opacity from 0.4 back to 1.0
/// when an app is tapped, providing subtle visual confirmation of the launch.
fn build_app_cell<'a>(
    app: &'a AppEntry,
    index: usize,
    icon_size: f32,
    target_opacity: f32,
) -> Element<'a, Message> {
    AnimationBuilder::new(target_opacity, move |opacity| {
        let cell_content = column![
            container(text(app.icon_placeholder()).size(icon_size * 0.5),)
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size))
                .center_x(Length::Fixed(icon_size))
                .center_y(Length::Fixed(icon_size)),
            text(&app.name).size(12).center(),
        ]
        .spacing(4)
        .align_x(iced::Alignment::Center);

        let btn = iced::widget::button(cell_content)
            .on_press(Message::LaunchApp(index))
            .padding(8)
            .width(Length::FillPortion(1));

        container(btn)
            .style(move |_theme: &iced::Theme| container::Style {
                text_color: Some(iced::Color {
                    a: opacity,
                    ..iced::Color::WHITE
                }),
                ..container::Style::default()
            })
            .width(Length::FillPortion(1))
            .into()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::AppEntry;

    fn make_app(name: &str) -> AppEntry {
        AppEntry {
            name: name.to_string(),
            exec: name.to_lowercase(),
            icon: String::new(),
            desktop_id: name.to_lowercase(),
            keywords: Vec::new(),
        }
    }

    #[test]
    fn build_search_bar_normal_mode() {
        let _bar = build_search_bar("", false);
    }

    #[test]
    fn build_search_bar_shell_mode() {
        let _bar = build_search_bar(">ls", true);
    }

    #[test]
    fn build_grid_empty_apps_shows_no_apps() {
        let feedback = LaunchFeedback::new();
        let recent: Vec<&AppEntry> = Vec::new();
        let main: Vec<&AppEntry> = Vec::new();
        let _grid = build_grid_with_sections(&[], &recent, &main, 6, 64.0, 20.0, &feedback);
    }

    #[test]
    fn build_grid_with_recent_and_main() {
        let apps = vec![make_app("A"), make_app("B"), make_app("C")];
        let recent: Vec<&AppEntry> = vec![&apps[0]];
        let main: Vec<&AppEntry> = vec![&apps[1], &apps[2]];
        let feedback = LaunchFeedback::new();
        let _grid =
            build_grid_with_sections(&apps, &recent, &main, 6, 64.0, 20.0, &feedback);
    }
}
