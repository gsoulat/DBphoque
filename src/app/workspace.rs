//! Espace de travail : onglets de requêtes, éditeur, résultats, historique et plan.

use super::*;
use iced::widget::column;

impl App {
    /// Messages de la section « Espace de travail ».
    pub(super) fn update_workspace(&mut self, message: Message) -> Task<Message> {
        match message {

            Message::Edit(action) => {
                let typing = matches!(action, Action::Edit(Edit::Insert(_) | Edit::Backspace));
                self.tab_mut().editor.perform(action);
                self.completion = if typing { self.compute_completion() } else { None };
            }
            Message::Explain => {
                let Some(c) = self.connector() else { return Task::none() };
                self.persist_tabs();
                self.status = Status::Busy("Analyse du plan…".into());
                let sql = self.current_sql();
                let id = self.tab().id;
                return Task::perform(
                    async move {
                        let plan = c.explain(&sql).await;
                        (sql, plan)
                    },
                    move |(sql, plan)| Message::Explained(id, sql, plan),
                );
            }
            Message::Explained(id, sql, Ok(plan)) => {
                self.status = Status::Idle;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                    tab.plan = Some(plan);
                    tab.plan_sql = Some(sql);
                }
                self.tab = Tab::Plan;
            }
            Message::Run => {
                let Some(c) = self.connector() else { return Task::none() };
                self.persist_tabs();
                self.status = Status::Busy("Exécution…".into());
                let sql = self.current_sql();
                let id = self.tab().id;
                return Task::perform(
                    async move {
                        let start = Instant::now();
                        let result = c.query(&sql).await;
                        (sql, result, start.elapsed())
                    },
                    move |(sql, result, elapsed)| Message::QueryDone(id, sql, result, elapsed),
                );
            }
            Message::QueryDone(id, sql, Ok(result), elapsed) => {
                let truncated = if result.rows.len() >= connector::MAX_ROWS {
                    format!(" (affichage limité à {})", connector::MAX_ROWS)
                } else {
                    String::new()
                };
                self.status = Status::Info(format!(
                    "{} ligne(s) en {} ms{truncated}",
                    result.rows.len(),
                    elapsed.as_millis()
                ));
                if let Some(conn) = self.session.as_ref().map(|s| s.id) {
                    let entry = HistoryEntry {
                        sql: sql.clone(),
                        at: chrono::Local::now().format("%d/%m %H:%M:%S").to_string(),
                        rows: result.rows.len(),
                        ms: elapsed.as_millis(),
                    };
                    match store::push_history(conn, entry) {
                        Ok(history) => self.history = history,
                        Err(e) => self.status = Status::Error(e),
                    }
                }
                let is_ddl = is_ddl(&sql);
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                    tab.result = Some(result);
                    tab.result_sql = Some(sql);
                    tab.page = 0;
                }
                self.tab = Tab::Results;
                if is_ddl {
                    // Une instruction de structure tapée à la main : la barre
                    // latérale, le schéma et le diagramme doivent suivre.
                    self.diagram = None;
                    return self.reload_structure();
                }
            }
            Message::QueryTabNew => {
                let n = self.next_tab_id;
                self.open_tab(format!("Requête {n}"), "");
                self.persist_tabs();
            }
            Message::QueryTabSelect(id) => {
                if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                    self.active = i;
                }
            }
            Message::QueryTabClose(id) => {
                if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                    self.tabs.remove(i);
                    if self.tabs.is_empty() {
                        self.open_tab("Requête 1".into(), "");
                    }
                    self.active = self.active.min(self.tabs.len() - 1);
                    self.persist_tabs();
                }
            }
            Message::QueryTabRename(name) => {
                self.tab_mut().name = name;
                self.persist_tabs();
            }
            Message::HistoryOpen(sql) => {
                let n = self.next_tab_id;
                self.open_tab(format!("Requête {n}"), &sql);
                self.tab = Tab::Results;
            }
            Message::TabSelected(tab) => {
                self.tab = tab;
                if tab == Tab::Diagram && self.diagram.is_none() && !self.diagram_loading {
                    return self.update(Message::LoadDiagram);
                }
                if tab == Tab::Data && self.data_table != self.selected {
                    return self.update(Message::LoadData);
                }
            }
            Message::PageChanged(page) => self.tab_mut().page = page,
            Message::ResultSort(col) => {
                let tab = self.tab_mut();
                tab.sort = toggle_sort(tab.sort, col);
                tab.page = 0;
            }
            Message::ResultFilter(f) => {
                let tab = self.tab_mut();
                tab.filter = f;
                tab.page = 0;
            }
            Message::DataSort(col) => self.data_sort = toggle_sort(self.data_sort, col),
            Message::DataFilter(f) => self.data_filter = f,
            Message::CellEditStart(row, col) => {
                let text = self
                    .data
                    .as_ref()
                    .and_then(|d| d.rows.get(row))
                    .and_then(|r| r.get(col))
                    .map(|c| match c {
                        connector::Cell::Null => String::new(),
                        other => other.display(),
                    })
                    .unwrap_or_default();
                self.editing_cell = Some((row, col, text));
            }
            Message::CellEditChanged(text) => {
                if let Some(cell) = &mut self.editing_cell {
                    cell.2 = text;
                }
            }
            Message::CellEditCancel => self.editing_cell = None,
            Message::Export(source, format) => {
                let Some(c) = self.connector() else { return Task::none() };
                let (columns, rows, order, table, name) = match source {
                    ExportSource::Results => {
                        let tab = self.tab();
                        let Some(r) = &tab.result else { return Task::none() };
                        let order = order_rows(&r.rows, tab.sort, &tab.filter);
                        (&r.columns, &r.rows, order, None, tab.name.clone())
                    }
                    ExportSource::Data => {
                        let Some(d) = &self.data else { return Task::none() };
                        let order = order_rows(&d.rows, self.data_sort, &self.data_filter);
                        let name = self.selected.as_ref().map_or("export".into(), |t| t.name.clone());
                        (&d.columns, &d.rows, order, self.selected.as_ref(), name)
                    }
                };
                let refs: Vec<&Vec<connector::Cell>> = order.iter().map(|&i| &rows[i]).collect();
                match export::render(format, columns, &refs, table, c.as_ref()) {
                    Ok(bytes) => {
                        let suggested = name.replace(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-', "_");
                        self.status = Status::Busy(format!("Export de {} ligne(s)…", refs.len()));
                        return Task::perform(export::save_with_dialog(suggested, format, bytes), Message::Exported);
                    }
                    Err(e) => self.status = Status::Error(e),
                }
            }
            Message::Exported(Ok(Some(path))) => {
                self.status = Status::Info(format!("Exporté : {}", path.display()));
            }
            Message::Exported(Ok(None)) => self.status = Status::Idle,
            Message::Exported(Err(e)) => self.status = Status::Error(e),
            Message::ImportPick => {
                if self.session.is_none() {
                    return Task::none();
                }
                self.edit_menu = false;
                self.status = Status::Busy("Choix du fichier…".into());
                return Task::perform(import::pick_and_read(), Message::ImportRead);
            }
            Message::ImportRead(Ok(Some((path, file)))) => {
                self.status = Status::Info(format!("{} ligne(s) lue(s)", file.rows.len()));
                let existing = self.selected.clone().filter(|_| !self.schema.is_empty()).map(|t| (t, self.schema.clone()));
                let mut form = ImportForm::new(path, file, existing, self.dialect().import_types);
                if form.new_schema.is_none()
                    && let Some(session) = &self.session
                {
                    form.new_schema = session.schemas.iter().find(|s| *s == "public").or(session.schemas.first()).cloned();
                }
                self.import = Some(form);
                self.import_error = None;
            }
            Message::ImportRead(Ok(None)) => self.status = Status::Idle,
            Message::ImportRead(Err(e)) => self.status = Status::Error(e),
            Message::Import(ImportMessage::Cancel) => {
                self.import = None;
                self.import_error = None;
            }
            Message::Import(ImportMessage::Submit) => {
                let (Some(form), Some(c)) = (&self.import, self.connector()) else { return Task::none() };
                match form.to_sql(c.as_ref()) {
                    Ok(pending) => {
                        self.import = None;
                        self.import_error = None;
                        self.dialog = Some(Dialog::Confirm(pending));
                        self.dialog_error = None;
                    }
                    Err(e) => self.import_error = Some(e),
                }
            }
            Message::Import(message) => {
                if let Some(form) = &mut self.import {
                    form.apply(message);
                }
            }
            Message::CellEditSubmit => {
                let Some((row, col, text)) = self.editing_cell.take() else { return Task::none() };
                let (Some(table), Some(data), Some(c)) = (self.selected.clone(), self.data.as_ref(), self.connector()) else {
                    return Task::none();
                };
                let Some(original) = data.rows.get(row) else { return Task::none() };
                // Même chemin que le formulaire : seule la colonne touchée change.
                let mut form = RowForm::update(table, self.schema.clone(), original);
                if let Some(field) = form.fields.get_mut(col) {
                    field.text = text;
                    field.null = false;
                }
                match form.to_sql(c.as_ref()) {
                    Ok(pending) => {
                        self.dialog = Some(Dialog::Confirm(pending));
                        self.dialog_error = None;
                    }
                    Err(e) => self.status = Status::Error(e),
                }
            }
            Message::CopyCell(value) => {
                let preview: String = value.chars().take(40).collect();
                self.status = Status::Info(format!("Copié : {preview}"));
                return iced::clipboard::write(value);
            }
            Message::KeyPressed(key, modifiers) => return self.handle_key(key, modifiers),
            _ => unreachable!(),
        }
        Task::none()
    }

    pub(super) fn query_tab_bar(&self) -> Element<'_, Message> {
        let mut bar = Row::new().spacing(2).align_y(iced::Center);
        for (i, tab) in self.tabs.iter().enumerate() {
            let active = i == self.active;
            let label: Element<'_, Message> = if active {
                text_input("nom", &tab.name)
                    .on_input(Message::QueryTabRename)
                    .size(12)
                    .padding([2, 6])
                    .width(150)
                    .into()
            } else {
                button(text(&tab.name).size(12).wrapping(text::Wrapping::None))
                    .on_press(Message::QueryTabSelect(tab.id))
                    .style(theme::list_item(false))
                    .padding([4, 8])
                    .into()
            };
            let close = button(text("×").size(12))
                .on_press(Message::QueryTabClose(tab.id))
                .style(button::text)
                .padding([2, 4]);
            bar = bar.push(
                container(row![label, close].spacing(2).align_y(iced::Center))
                    .style(if active { theme::header_cell } else { |_: &Theme| container::Style::default() })
                    .padding(2),
            );
        }
        bar = bar.push(
            button(text("+").size(13))
                .on_press(Message::QueryTabNew)
                .style(theme::list_item(false))
                .padding([3, 8]),
        );
        scrollable(bar)
            .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4)))
            .into()
    }

    /// Haut de l'espace de travail : onglets de requêtes, éditeur, actions.
    pub(super) fn editor_pane(&self) -> Element<'_, Message> {
        let connected = self.session.is_some();

        let completing = self.completion.is_some();
        let editor = text_editor(&self.tab().editor)
            .on_action(Message::Edit)
            .key_binding(move |press| editor_key_binding(press, completing))
            .highlight("sql", highlighter::Theme::Base16Ocean)
            .font(MONO)
            .height(Fill)
            .padding(10);

        // Liste de complétion posée sous le curseur (position approchée :
        // police mono de 13 px, interligne 1,3).
        let editor: Element<'_, Message> = match &self.completion {
            Some(c) => {
                let items = c.items.iter().enumerate().map(|(i, item)| {
                    button(text(item).size(12).font(MONO))
                        .on_press(Message::CompletionMove(i as i32 - c.selected as i32))
                        .style(theme::list_item(i == c.selected))
                        .padding([2, 8])
                        .width(Fill)
                        .into()
                });
                let list = container(Column::with_children(items).spacing(1))
                    .style(theme::tooltip)
                    .padding(4)
                    .width(240);
                let x = 10.0 + (c.column.saturating_sub(c.prefix_len)) as f32 * CHAR_WIDTH;
                let y = 10.0 + (c.line as f32 + 1.0) * 13.0 * 1.3 + 2.0;
                stack![
                    editor,
                    container(list).padding(padding::top(y).left(x)),
                ]
                .into()
            }
            None => editor.into(),
        };

        let actions = row![
            button(text("Expliquer").size(13))
                .on_press_maybe(connected.then_some(Message::Explain))
                .style(button::secondary)
                .padding([6, 14]),
            button(text("Exécuter  ⌘↩").size(13))
                .on_press_maybe(connected.then_some(Message::Run))
                .style(theme::primary)
                .padding([6, 14]),
            text("La sélection seule est exécutée si elle existe.")
                .size(12)
                .color(theme::MUTED)
                .width(Fill),
            button(text(if self.results_hidden { "▲ Résultats" } else { "▼ Replier les résultats" }).size(11))
                .on_press(Message::ToggleResults)
                .style(button::text)
                .padding([2, 6]),
        ]
        .spacing(10)
        .align_y(iced::Center);

        column![self.query_tab_bar(), editor, actions].spacing(10).width(Fill).height(Fill).into()
    }

    /// Bas de l'espace de travail : les onglets Résultats, Données, Schéma…
    pub(super) fn results_pane(&self) -> Element<'_, Message> {
        let tab = |label, tab| {
            button(text(label).size(13))
                .on_press(Message::TabSelected(tab))
                .style(theme::tab(self.tab == tab))
                .padding([5, 14])
        };
        let tabs = column![
            row![
                tab("Résultats", Tab::Results),
                tab("Données", Tab::Data),
                tab("Schéma", Tab::Schema),
                tab("Plan d'exécution", Tab::Plan),
                tab("Diagramme", Tab::Diagram),
                tab("Historique", Tab::History),
            ]
            .spacing(2),
            iced::widget::rule::horizontal(1).style(|_: &Theme| iced::widget::rule::Style {
                color: theme::BORDER,
                radius: 0.0.into(),
                fill_mode: iced::widget::rule::FillMode::Full,
                snap: false,
            }),
        ]
        .spacing(6);

        let content = match self.tab {
            Tab::Results => self.results_view(),
            Tab::Data => self.data_view(),
            Tab::Plan => self.plan_view(),
            Tab::Schema => self.schema_view(),
            Tab::Diagram => self.diagram_view(),
            Tab::History => self.history_view(),
        };

        container(column![tabs, content].spacing(8))
            .style(theme::panel)
            .padding(10)
            .width(Fill)
            .height(Fill)
            .into()
    }

    pub(super) fn workspace(&self) -> Element<'_, Message> {
        if self.results_hidden {
            // Partie basse repliée : l'éditeur prend toute la hauteur.
            return self.editor_pane();
        }
        pane_grid(&self.work_panes, |_, pane, _| {
            pane_grid::Content::new(match pane {
                WorkPane::Editor => self.editor_pane(),
                WorkPane::Results => self.results_pane(),
            })
        })
        .on_resize(8, Message::WorkPaneResized)
        .style(theme::pane_grid)
        .spacing(10)
        .width(Fill)
        .height(Fill)
        .into()
    }

    pub(super) fn history_view(&self) -> Element<'_, Message> {
        if self.session.is_none() {
            return hint("L'historique est propre à chaque connexion.");
        }
        if self.history.is_empty() {
            return hint("Aucune requête exécutée sur cette connexion pour l'instant.");
        }
        let entries = self.history.iter().map(|h| {
            let preview: String = h.sql.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(140).collect();
            button(
                row![
                    text(&h.at).size(11).color(theme::MUTED).width(90),
                    text(preview).size(12).font(MONO).wrapping(text::Wrapping::None).width(Fill),
                    text(format!("{} l. · {} ms", h.rows, h.ms)).size(11).color(theme::MUTED),
                ]
                .spacing(10)
                .align_y(iced::Center),
            )
            .on_press(Message::HistoryOpen(h.sql.clone()))
            .style(theme::list_item(false))
            .width(Fill)
            .clip(true)
            .into()
        });
        column![
            text("Clique sur une requête pour l'ouvrir dans un nouvel onglet.").size(11).color(theme::MUTED),
            scrollable(Column::with_children(entries).spacing(2)).height(Fill),
        ]
        .spacing(6)
        .into()
    }

    pub(super) fn results_view(&self) -> Element<'_, Message> {
        let tab = self.tab();
        let Some(result) = &tab.result else {
            return hint("Écris une requête puis clique sur « Exécuter » (⌘↩).");
        };
        if result.columns.is_empty() {
            return hint("La requête n'a renvoyé aucune ligne.");
        }

        let order = order_rows(&result.rows, tab.sort, &tab.filter);
        let total = order.len();
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        let page = tab.page.min(pages - 1);
        let start = page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(total);

        let rows = order[start..end].iter().map(|&i| (i, result.rows[i].iter().map(|c| c.display()).collect()));
        let grid = Grid::new(result.columns.clone(), rows)
            .action(CellAction::Copy)
            .sort(tab.sort, Message::ResultSort)
            .build();

        let filter = text_input("Filtrer…", &tab.filter)
            .on_input(Message::ResultFilter)
            .size(12)
            .padding([3, 8])
            .width(180);
        let mut header = row![source_line(tab.result_sql.as_deref()), filter, export_buttons(ExportSource::Results)]
            .spacing(12)
            .align_y(iced::Center);
        if pages > 1 {
            let nav = |label, target: Option<usize>| {
                button(text(label).size(12))
                    .on_press_maybe(target.map(Message::PageChanged))
                    .style(button::secondary)
                    .padding([2, 8])
            };
            header = header.push(container(row![
                nav("‹", page.checked_sub(1)),
                text(format!("lignes {}–{} sur {total}{}", start + 1, end, if total < result.rows.len() { " (filtrées)" } else { "" })).size(12).color(theme::MUTED),
                nav("›", (page + 1 < pages).then_some(page + 1)),
            ]
            .spacing(8)
            .align_y(iced::Center))
            .align_right(Fill));
        }

        column![header, grid].spacing(6).into()
    }

    // ------------------------------------------------------------------
    // Actions d'édition, partagées par le menu « Édition » et les onglets.
    // `None` : l'action ne s'applique pas dans l'état courant.
    // ------------------------------------------------------------------

    pub(super) fn plan_view(&self) -> Element<'_, Message> {
        let tab = self.tab();
        let Some(plan) = &tab.plan else {
            return hint("Clique sur « Expliquer » (⌘E) pour voir le plan sans exécuter la requête.");
        };

        let total = match plan.total_cost {
            Some(cost) => format!("Coût total estimé : {cost:.2} {}", plan.cost_unit),
            None => "Ce moteur ne fournit pas d'estimation de coût.".into(),
        };

        let tree = plan.nodes.iter().map(|node| {
            let mut stats = Vec::new();
            if let Some(cost) = node.cost {
                stats.push(format!("coût {cost:.2}"));
            }
            if let Some(rows) = node.rows {
                stats.push(format!("~{rows:.0} lignes"));
            }
            row![
                text(format!("└ {}", node.label)).font(MONO).size(13),
                text(stats.join(" · ")).size(12).color(theme::MUTED),
            ]
            .spacing(14)
            .align_y(iced::Center)
            .padding(padding::left(node.depth as f32 * 20.0))
            .into()
        });

        let order = execution_order(&plan.nodes).into_iter().enumerate().map(|(step, i)| {
            text(format!("{}. {}", step + 1, plan.nodes[i].label)).font(MONO).size(13).into()
        });

        let mut content = column![
            source_line(tab.plan_sql.as_deref()),
            text(total).size(15).color(theme::PRIMARY),
        ]
        .spacing(10);
        if let Some(note) = &plan.note {
            content = content.push(text(note).size(12).color(theme::MUTED));
        }
        let content = content
            .push(section("ARBRE DU PLAN"))
            .push(Column::with_children(tree).spacing(4))
            .push(section("ORDRE D'EXÉCUTION"))
            .push(Column::with_children(order).spacing(4));

        scrollable(content).height(Fill).into()
    }
}

/// Raccourcis de l'éditeur : ⌘↩ exécute, ⌘E explique, le reste est standard.
pub(super) fn editor_key_binding(press: KeyPress, completing: bool) -> Option<Binding<Message>> {
    if completing {
        match press.key.as_ref() {
            Key::Named(Named::Tab) => return Some(Binding::Custom(Message::CompletionAccept)),
            Key::Named(Named::Enter) if !press.modifiers.command() => {
                return Some(Binding::Custom(Message::CompletionAccept));
            }
            Key::Named(Named::ArrowDown) => return Some(Binding::Custom(Message::CompletionMove(1))),
            Key::Named(Named::ArrowUp) => return Some(Binding::Custom(Message::CompletionMove(-1))),
            Key::Named(Named::Escape) => return Some(Binding::Custom(Message::CompletionClose)),
            _ => {}
        }
    }
    if press.modifiers.command() {
        match press.key.as_ref() {
            Key::Named(Named::Enter) => return Some(Binding::Custom(Message::Run)),
            Key::Character("e") => return Some(Binding::Custom(Message::Explain)),
            Key::Character("r") => return Some(Binding::Custom(Message::Refresh)),
            _ => {}
        }
    }
    Binding::from_key_press(press)
}
