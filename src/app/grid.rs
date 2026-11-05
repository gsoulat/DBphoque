//! Grille de résultats : tri, filtre et rendu des cellules.

use super::*;
use iced::widget::column;


/// Bascule le tri sur une colonne : croissant, puis décroissant, puis aucun.
pub(super) fn toggle_sort(current: Option<(usize, bool)>, col: usize) -> Option<(usize, bool)> {
    match current {
        Some((c, true)) if c == col => Some((col, false)),
        Some((c, false)) if c == col => None,
        _ => Some((col, true)),
    }
}

/// Indices des lignes à afficher : filtrées (sous-chaîne, sans casse, sur
/// toutes les colonnes) puis triées. Les nombres se comparent comme des
/// nombres, NULL en dernier.
pub(super) fn order_rows(rows: &[Vec<connector::Cell>], sort: Option<(usize, bool)>, filter: &str) -> Vec<usize> {
    use connector::Cell;
    let needle = filter.trim().to_lowercase();
    let mut order: Vec<usize> = (0..rows.len())
        .filter(|&i| needle.is_empty() || rows[i].iter().any(|c| c.display().to_lowercase().contains(&needle)))
        .collect();
    let Some((col, ascending)) = sort else { return order };

    let key = |i: usize| rows[i].get(col);
    order.sort_by(|&a, &b| {
        let ordering = match (key(a), key(b)) {
            (Some(Cell::Null) | None, Some(Cell::Null) | None) => std::cmp::Ordering::Equal,
            (Some(Cell::Null) | None, _) => std::cmp::Ordering::Greater,
            (_, Some(Cell::Null) | None) => std::cmp::Ordering::Less,
            (Some(x), Some(y)) => match (number(x), number(y)) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => x.display().to_lowercase().cmp(&y.display().to_lowercase()),
            },
        };
        // NULL reste en dernier quel que soit le sens.
        let is_null = |i| matches!(key(i), Some(Cell::Null) | None);
        if is_null(a) || is_null(b) || ascending { ordering } else { ordering.reverse() }
    });
    order
}

pub(super) fn number(cell: &connector::Cell) -> Option<f64> {
    match cell {
        connector::Cell::Int(i) => Some(*i as f64),
        connector::Cell::Float(f) => Some(*f),
        connector::Cell::Text(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Grille défilable dans les deux sens. Les colonnes prennent la largeur de
/// leur contenu (bornée). Chaque ligne porte son indice d'origine, pour que
/// la sélection survive au tri et au filtre.
pub(super) struct Grid<'a> {
    columns: Vec<String>,
    rows: Vec<(usize, Vec<String>)>,
    selected: Option<usize>,
    action: CellAction,
    sort: Option<(usize, bool)>,
    on_sort: Option<fn(usize) -> Message>,
    /// (ligne, colonne, texte) : cette cellule est un champ de saisie.
    editing: Option<(usize, usize, &'a str)>,
    /// Second clic sur une cellule de la ligne sélectionnée : édition en place.
    editable: bool,
}

impl<'a> Grid<'a> {
    pub(super) fn new(columns: Vec<String>, rows: impl Iterator<Item = (usize, Vec<String>)>) -> Self {
        Self {
            columns,
            rows: rows.collect(),
            selected: None,
            action: CellAction::Copy,
            sort: None,
            on_sort: None,
            editing: None,
            editable: false,
        }
    }

    pub(super) fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub(super) fn action(mut self, action: CellAction) -> Self {
        self.action = action;
        self
    }

    pub(super) fn sort(mut self, sort: Option<(usize, bool)>, on_sort: fn(usize) -> Message) -> Self {
        self.sort = sort;
        self.on_sort = Some(on_sort);
        self
    }

    pub(super) fn editable(mut self, editing: Option<(usize, usize, &'a str)>) -> Self {
        self.editable = true;
        self.editing = editing;
        self
    }

    pub(super) fn build(self) -> Element<'a, Message> {
        let widths: Vec<f32> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let longest = self
                    .rows
                    .iter()
                    .filter_map(|(_, r)| r.get(i))
                    .map(|v| v.chars().count())
                    .max()
                    .unwrap_or(0)
                    .max(name.chars().count() + 2);
                (longest as f32 * CHAR_WIDTH + 18.0).clamp(MIN_CELL, MAX_CELL)
            })
            .collect();

        let header = Row::with_children(self.columns.iter().enumerate().zip(&widths).map(|((i, name), &w)| {
            let arrow = match self.sort {
                Some((c, true)) if c == i => " ▲",
                Some((c, false)) if c == i => " ▼",
                _ => "",
            };
            let label = text(format!("{name}{arrow}")).font(MONO).size(13).wrapping(text::Wrapping::None);
            match self.on_sort {
                Some(on_sort) => button(label)
                    .on_press(on_sort(i))
                    .style(theme::cell)
                    .width(w)
                    .padding([4, 8])
                    .clip(true)
                    .into(),
                None => container(label).width(w).padding([4, 8]).clip(true).into(),
            }
        }));
        let header = container(header).style(theme::header_cell);

        let lines = self.rows.into_iter().enumerate().map(|(n, (i, values))| {
            let is_selected = self.selected == Some(i);
            let cells = values.into_iter().enumerate().zip(&widths).map(|((col, value), &w)| {
                if let Some((er, ec, text_value)) = self.editing
                    && er == i
                    && ec == col
                {
                    return text_input("", text_value)
                        .on_input(Message::CellEditChanged)
                        .on_submit(Message::CellEditSubmit)
                        .font(MONO)
                        .size(13)
                        .padding([1, 6])
                        .width(w)
                        .into();
                }
                let label = text(value.clone()).font(MONO).size(13).wrapping(text::Wrapping::None);
                let message = match self.action {
                    CellAction::Copy => Message::CopyCell(value),
                    CellAction::SelectRow(_) if self.editable && is_selected => Message::CellEditStart(i, col),
                    CellAction::SelectRow(f) => f(i),
                };
                button(label)
                    .on_press(message)
                    .style(theme::cell)
                    .width(w)
                    .padding([3, 8])
                    .clip(true)
                    .into()
            });
            let line = container(Row::with_children(cells));
            if is_selected {
                line.style(theme::grid_row_selected).into()
            } else {
                line.style(theme::grid_row(n % 2 == 1)).into()
            }
        });

        let body = column![header, Column::with_children(lines)].width(Length::Shrink);

        scrollable(body)
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::new(),
                horizontal: scrollable::Scrollbar::new(),
            })
            .width(Fill)
            .height(Fill)
            .into()
    }
}
