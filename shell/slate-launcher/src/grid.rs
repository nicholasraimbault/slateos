// Grid layout logic for the app launcher.
//
// Arranges app entries into rows of a fixed number of columns. The actual
// rendering is done in the iced view; this module only computes the layout
// data structure.

use crate::apps::AppEntry;

/// Default number of columns in the app grid.
pub const DEFAULT_COLUMNS: usize = 6;

/// Default icon size in logical pixels.
pub const ICON_SIZE: u32 = 48;

/// Split a flat list of apps into rows of at most `columns` items.
///
/// The last row may have fewer than `columns` entries.
pub fn grid_items<'a>(apps: &'a [&AppEntry], columns: usize) -> Vec<Vec<&'a AppEntry>> {
    if columns == 0 || apps.is_empty() {
        return Vec::new();
    }

    apps.chunks(columns).map(|chunk| chunk.to_vec()).collect()
}

/// Calculate the number of rows needed for `item_count` items in `columns`
/// columns.
pub fn row_count(item_count: usize, columns: usize) -> usize {
    if columns == 0 {
        return 0;
    }
    item_count.div_ceil(columns)
}

/// Calculate the optimal column count given available width and minimum cell
/// width. Falls back to `DEFAULT_COLUMNS` if the calculation would produce
/// fewer than 1 column.
pub fn columns_for_width(available_width: f32, min_cell_width: f32) -> usize {
    if min_cell_width <= 0.0 {
        return DEFAULT_COLUMNS;
    }

    let cols = (available_width / min_cell_width).floor() as usize;
    cols.max(1)
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
    fn seven_apps_in_six_columns_produces_two_rows() {
        let apps_owned: Vec<AppEntry> = (0..7).map(|i| make_app(&format!("App{i}"))).collect();
        let apps_refs: Vec<&AppEntry> = apps_owned.iter().collect();

        let rows = grid_items(&apps_refs, 6);
        assert_eq!(rows.len(), 2, "7 apps in 6 columns should produce 2 rows");
        assert_eq!(rows[0].len(), 6, "first row should be full");
        assert_eq!(rows[1].len(), 1, "second row should have the remainder");
    }

    #[test]
    fn exact_multiple_fills_all_rows() {
        let apps_owned: Vec<AppEntry> = (0..12).map(|i| make_app(&format!("App{i}"))).collect();
        let apps_refs: Vec<&AppEntry> = apps_owned.iter().collect();

        let rows = grid_items(&apps_refs, 6);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 6);
        assert_eq!(rows[1].len(), 6);
    }

    #[test]
    fn empty_apps_returns_empty_grid() {
        let apps: Vec<&AppEntry> = Vec::new();
        let rows = grid_items(&apps, 6);
        assert!(rows.is_empty());
    }

    #[test]
    fn zero_columns_returns_empty_grid() {
        let app = make_app("A");
        let apps: Vec<&AppEntry> = vec![&app];
        let rows = grid_items(&apps, 0);
        assert!(rows.is_empty());
    }

    #[test]
    fn row_count_calculates_correctly() {
        assert_eq!(row_count(7, 6), 2);
        assert_eq!(row_count(6, 6), 1);
        assert_eq!(row_count(0, 6), 0);
        assert_eq!(row_count(13, 6), 3);
        assert_eq!(row_count(5, 0), 0);
    }

    #[test]
    fn columns_for_width_respects_minimum() {
        assert_eq!(columns_for_width(600.0, 100.0), 6);
        assert_eq!(columns_for_width(300.0, 100.0), 3);
        assert_eq!(columns_for_width(50.0, 100.0), 1);
    }

    #[test]
    fn columns_for_width_handles_zero_min() {
        assert_eq!(columns_for_width(600.0, 0.0), DEFAULT_COLUMNS);
        assert_eq!(columns_for_width(600.0, -1.0), DEFAULT_COLUMNS);
    }

    #[test]
    fn single_app_produces_single_row() {
        let app = make_app("Solo");
        let apps: Vec<&AppEntry> = vec![&app];
        let rows = grid_items(&apps, 6);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 1);
    }

    #[test]
    fn grid_preserves_order() {
        let apps_owned = vec![make_app("A"), make_app("B"), make_app("C")];
        let apps_refs: Vec<&AppEntry> = apps_owned.iter().collect();

        let rows = grid_items(&apps_refs, 2);
        assert_eq!(rows[0][0].name, "A");
        assert_eq!(rows[0][1].name, "B");
        assert_eq!(rows[1][0].name, "C");
    }
}
