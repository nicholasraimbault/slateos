/// Sidebar navigation for the settings app.
///
/// Presents a vertical list of pages. Tapping a page switches the
/// right-hand content pane.
use iced::widget::{button, column, container, scrollable, text};
use iced::{Element, Length};

// ---------------------------------------------------------------------------
// Page enum
// ---------------------------------------------------------------------------

/// Every page in the settings app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Display,
    Wallpaper,
    Dock,
    Gestures,
    Keyboard,
    Ai,
    Fex,
    Network,
    About,
}

impl Page {
    /// Ordered list of all pages for sidebar rendering.
    pub fn all() -> &'static [Page] {
        &[
            Page::Display,
            Page::Wallpaper,
            Page::Dock,
            Page::Gestures,
            Page::Keyboard,
            Page::Ai,
            Page::Fex,
            Page::Network,
            Page::About,
        ]
    }

    /// Human-readable label for the sidebar.
    pub fn label(self) -> &'static str {
        match self {
            Page::Display => "Display",
            Page::Wallpaper => "Wallpaper",
            Page::Dock => "Dock",
            Page::Gestures => "Gestures",
            Page::Keyboard => "Keyboard",
            Page::Ai => "AI & LLM",
            Page::Fex => "x86 Compat",
            Page::Network => "Network",
            Page::About => "About",
        }
    }

    /// Icon label (text placeholder until real icons are used).
    pub fn icon(self) -> &'static str {
        match self {
            Page::Display => "\u{1f4bb}",
            Page::Wallpaper => "\u{1f5bc}",
            Page::Dock => "\u{2693}",
            Page::Gestures => "\u{270b}",
            Page::Keyboard => "\u{2328}",
            Page::Ai => "\u{1f916}",
            Page::Fex => "\u{1f4e6}",
            Page::Network => "\u{1f4f6}",
            Page::About => "\u{2139}",
        }
    }
}

// ---------------------------------------------------------------------------
// Sidebar view
// ---------------------------------------------------------------------------

/// Render the left sidebar. Returns an element that emits `Page` on tap.
pub fn view_sidebar<'a, M: Clone + 'a>(
    active: Page,
    on_select: impl Fn(Page) -> M + 'a,
) -> Element<'a, M> {
    let items = Page::all().iter().map(|&page| {
        let label_text = format!("{}  {}", page.icon(), page.label());
        let style = if page == active {
            button::primary
        } else {
            button::secondary
        };
        button(text(label_text).size(16))
            .on_press(on_select(page))
            .width(Length::Fill)
            .padding(12)
            .style(style)
            .into()
    });

    let col: Element<'a, M> = column(items).spacing(4).width(Length::Fill).into();

    container(scrollable(col))
        .width(Length::Fixed(200.0))
        .height(Length::Fill)
        .padding(8)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_list_has_all_expected_pages() {
        let pages = Page::all();
        assert_eq!(pages.len(), 9);
        assert!(pages.contains(&Page::Display));
        assert!(pages.contains(&Page::Wallpaper));
        assert!(pages.contains(&Page::Dock));
        assert!(pages.contains(&Page::Gestures));
        assert!(pages.contains(&Page::Keyboard));
        assert!(pages.contains(&Page::Ai));
        assert!(pages.contains(&Page::Fex));
        assert!(pages.contains(&Page::Network));
        assert!(pages.contains(&Page::About));
    }

    #[test]
    fn every_page_has_label_and_icon() {
        for &page in Page::all() {
            assert!(!page.label().is_empty(), "{:?} has no label", page);
            assert!(!page.icon().is_empty(), "{:?} has no icon", page);
        }
    }

    #[test]
    fn page_labels_are_unique() {
        let labels: Vec<&str> = Page::all().iter().map(|p| p.label()).collect();
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "duplicate label");
                }
            }
        }
    }
}
