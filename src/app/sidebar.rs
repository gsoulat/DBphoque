//! Barre latérale : connexions, schémas et tables.

use super::*;
use iced::widget::column;

impl App {
    /// Messages de la section « Barre latérale ».
    pub(super) fn update_sidebar(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Connect(id) => {
                let Some(config) = self.connections.iter().find(|c| c.id == id).cloned() else {
                    return Task::none();
                };
                self.connecting = Some(id);
                self.status = Status::Busy(format!("Connexion à {}…", config.name));
                return Task::perform(open(config, None), move |r| Message::Connected(id, r));
            }
            Message::Connected(id, Ok((c, tunnel))) => {
                self.connecting = None;
                self.status = Status::Info(format!(
                    "Connecté ({}){}",
                    c.engine(),
                    if tunnel.is_some() { " via tunnel SSH" } else { "" }
                ));
                self.persist_tabs();
                self.dialect_cache = c.dialect().clone();
                self.session = Some(Session {
                    id,
                    connector: c.clone(),
                    tables: Vec::new(),
                    schemas: Vec::new(),
                    _tunnel: tunnel,
                });
                self.load_tabs(id);
                self.selected = None;
                self.schema.clear();
                self.collapsed.clear();
                self.folded.remove(&id);
                self.diagram = None;
                self.data = None;
                self.data_table = None;
                return self.reload_structure();
            }
            Message::Connected(_, Err(e)) => {
                self.connecting = None;
                self.status = Status::Error(e);
            }
            Message::Disconnect => {
                self.persist_tabs();
                self.session = None;
                self.dialect_cache = editing::default_dialect();
                self.history.clear();
                self.selected = None;
                self.schema.clear();
                self.diagram = None;
                self.data = None;
                self.data_table = None;
                self.status = Status::Idle;
            }
            Message::SchemasLoaded(Ok(schemas)) => {
                if let Some(session) = &mut self.session {
                    session.schemas = schemas;
                }
            }
            Message::TablesLoaded(Ok(tables)) => {
                let describe_all = self.connector().map(|c| {
                    let tables = tables.clone();
                    Task::perform(
                        async move {
                            let mut out = Vec::new();
                            for t in tables {
                                if let Ok(cols) = c.describe_table(&t).await {
                                    out.push((t, cols));
                                }
                            }
                            out
                        },
                        Message::ColumnsCached,
                    )
                });
                if let Some(session) = &mut self.session {
                    session.tables = tables;
                }
                self.columns_cache.clear();
                let mut tasks = Vec::new();
                if let Some(t) = describe_all {
                    tasks.push(t);
                }
                // Diagramme ouvert : on le recharge avec la nouvelle structure.
                if self.tab == Tab::Diagram && self.diagram.is_none() && !self.diagram_loading {
                    tasks.push(self.update(Message::LoadDiagram));
                }
                // Table sélectionnée encore présente : on relit son schéma
                // (et ses lignes si l'onglet Données est ouvert).
                let still_there = self
                    .selected
                    .as_ref()
                    .is_some_and(|t| self.session.as_ref().is_some_and(|s| s.tables.contains(t)));
                if still_there {
                    tasks.push(self.update(Message::TableSelected(self.selected.clone().unwrap())));
                } else {
                    self.selected = None;
                }
                return Task::batch(tasks);
            }
            Message::ColumnsCached(all) => {
                self.columns_cache = all.into_iter().collect();
            }
            Message::CompletionMove(delta) => {
                if let Some(c) = &mut self.completion {
                    let n = c.items.len() as i32;
                    c.selected = ((c.selected as i32 + delta).rem_euclid(n)) as usize;
                }
            }
            Message::CompletionClose => self.completion = None,
            Message::CompletionAccept => {
                let Some(c) = self.completion.take() else { return Task::none() };
                let Some(item) = c.items.get(c.selected).cloned() else { return Task::none() };
                let editor = &mut self.tab_mut().editor;
                for _ in 0..c.prefix_len {
                    editor.perform(Action::Edit(Edit::Backspace));
                }
                editor.perform(Action::Edit(Edit::Paste(Arc::new(item))));
            }
            Message::TableSelected(table) => {
                let Some(c) = self.connector() else { return Task::none() };
                let same = self.selected.as_ref() == Some(&table);
                if !same {
                    // On ne détruit pas une requête en cours : l'aperçu remplace
                    // l'onglet actif seulement s'il est vide ou déjà un aperçu.
                    let current = self.tab().editor.text();
                    let sql = c.preview_sql(&table);
                    if is_preview_sql(&current) {
                        self.tab_mut().editor = text_editor::Content::with_text(&sql);
                    } else {
                        self.open_tab(table.name.clone(), &sql);
                    }
                }
                self.selected = Some(table.clone());
                self.schema_row = None;
                if self.tab != Tab::Data && !same {
                    self.tab = Tab::Schema;
                }
                let describe = Task::perform(
                    async move { c.describe_table(&table).await },
                    Message::SchemaLoaded,
                );
                if self.tab == Tab::Data {
                    return Task::batch([describe, self.update(Message::LoadData)]);
                }
                return describe;
            }
            Message::InsertTable(table) => {
                let Some(c) = self.connector() else { return Task::none() };
                let name = match &table.schema {
                    Some(schema) => format!(
                        "{}.{}",
                        c.quote_identifier(schema),
                        c.quote_identifier(&table.name)
                    ),
                    None => c.quote_identifier(&table.name),
                };
                self.tab_mut().editor.perform(Action::Edit(Edit::Paste(Arc::new(name))));
                return Task::none();
            }
            Message::SchemaLoaded(Ok(columns)) => {
                if let Some(t) = &self.selected {
                    self.columns_cache.insert(t.clone(), columns.clone());
                }
                self.schema = columns;
            }
            Message::FilterChanged(filter) => self.filter = filter,
            Message::ToggleSchema(schema) => {
                if !self.collapsed.remove(&schema) {
                    self.collapsed.insert(schema);
                }
            }
            Message::ToggleFold(id) => {
                if !self.folded.remove(&id) {
                    self.folded.insert(id);
                }
            }
            Message::ToggleFolder(folder) => {
                let key = format!("folder:{folder}");
                if !self.collapsed.remove(&key) {
                    self.collapsed.insert(key);
                }
            }
            Message::PaneResized(event) => self.panes.resize(event.split, event.ratio),
            Message::WorkPaneResized(event) => self.work_panes.resize(event.split, event.ratio),
            Message::ToggleSidebar => self.sidebar_hidden = !self.sidebar_hidden,
            Message::Refresh => {
                if self.session.is_none() {
                    return Task::none();
                }
                self.status = Status::Busy("Actualisation…".into());
                self.diagram = None;
                self.data = None;
                self.data_table = None;
                return self.reload_structure();
            }
            Message::ToggleResults => self.results_hidden = !self.results_hidden,
            _ => unreachable!(),
        }
        Task::none()
    }

    pub(super) fn sidebar(&self) -> Element<'_, Message> {
        let mut list = Column::new().spacing(2);

        // Les dossiers d'abord (repliables), puis les connexions sans dossier
        // par emplacement.
        let mut folders: Vec<&str> = self
            .connections
            .iter()
            .map(|c| c.folder.as_str())
            .filter(|f| !f.is_empty())
            .collect();
        folders.sort_unstable();
        folders.dedup();

        for folder in folders {
            let collapsed = self.collapsed.contains(&format!("folder:{folder}"));
            let count = self.connections.iter().filter(|c| c.folder == folder).count();
            list = list.push(
                button(
                    text(format!("{} {folder}  ({count})", if collapsed { "▸" } else { "▾" }))
                        .size(12)
                        .color(theme::TEXT)
                        .wrapping(text::Wrapping::None),
                )
                .on_press(Message::ToggleFolder(folder.to_owned()))
                .style(theme::list_item(false))
                .padding([4, 6])
                .width(Fill)
                .clip(true),
            );
            if collapsed {
                continue;
            }
            for config in self.connections.iter().filter(|c| c.folder == folder) {
                list = list.push(container(self.connection_entry(config)).padding(padding::left(10)));
            }
        }

        for location in Location::ALL {
            let group: Vec<_> = self
                .connections
                .iter()
                .filter(|c| c.folder.is_empty() && c.location() == location)
                .collect();
            if group.is_empty() {
                continue;
            }
            list = list.push(
                container(text(location.label()).size(11).color(theme::MUTED))
                    .padding(padding::top(10).bottom(4)),
            );
            for config in group {
                list = list.push(self.connection_entry(config));
            }
        }

        if self.connections.is_empty() {
            list = list.push(text("Aucune connexion enregistrée.").size(13).color(theme::MUTED));
        }

        let header = row![
            text("CONNEXIONS").size(11).color(theme::MUTED).width(Fill),
            tooltip(
                button(text("↻").size(13))
                    .on_press_maybe(self.session.as_ref().map(|_| Message::Refresh))
                    .style(button::text)
                    .padding([2, 4]),
                text("Actualiser (⌘R)").size(12),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(6),
            button(text("Gérer").size(12)).on_press(Message::OpenManager).style(button::text),
            button(text("◀").size(11).color(theme::MUTED))
                .on_press(Message::ToggleSidebar)
                .style(button::text)
                .padding([2, 4]),
        ]
        .spacing(4)
        .align_y(iced::Center);

        let filter = text_input("Filtrer les tables…", &self.filter)
            .on_input(Message::FilterChanged)
            .size(13)
            .padding(6);

        container(column![header, filter, scrollable(list).height(Fill)].spacing(6))
            .style(theme::panel)
            .padding(10)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// Une connexion ; si elle est ouverte, ses tables s'affichent dessous,
    /// groupées par schéma.
    pub(super) fn connection_entry<'a>(&'a self, config: &'a ConnectionConfig) -> Element<'a, Message> {
        let session = self.session.as_ref().filter(|s| s.id == config.id);
        let marker = if session.is_some() {
            text("●").color(theme::SUCCESS)
        } else if self.connecting == Some(config.id) {
            text("●").color(theme::WARNING)
        } else {
            text("○").color(theme::MUTED)
        }
        .size(13);

        let folded = self.folded.contains(&config.id);
        let entry = button(
            row![
                text(if session.is_none() { " " } else if folded { "▸" } else { "▾" })
                    .size(14)
                    .color(theme::MUTED)
                    .width(12),
                marker,
                column![
                    text(&config.name).size(13).wrapping(text::Wrapping::None),
                    text(format!("{} · {}", config.engine, config.summary()))
                        .size(11)
                        .color(theme::MUTED)
                        .wrapping(text::Wrapping::None),
                ]
                .width(Fill),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .on_press_maybe(session.is_none().then_some(Message::Connect(config.id)))
        .style(theme::list_item(session.is_some()))
        .width(Fill)
        .clip(true);

        let Some(session) = session else { return entry.into() };

        // Connexion ouverte : la flèche devant le nom replie sa liste de tables.
        let entry = entry.on_press(Message::ToggleFold(config.id));
        if folded {
            return entry.into();
        }

        let filter = self.filter.trim().to_lowercase();
        let mut by_schema: BTreeMap<String, Vec<&TableRef>> = BTreeMap::new();
        for table in &session.tables {
            if !filter.is_empty() && !table.to_string().to_lowercase().contains(&filter) {
                continue;
            }
            by_schema.entry(table.schema.clone().unwrap_or_default()).or_default().push(table);
        }

        let mut tree = Column::new().spacing(1);
        for (schema, tables) in by_schema {
            let collapsed = self.collapsed.contains(&schema) && filter.is_empty();
            if !schema.is_empty() {
                let arrow = if collapsed { "▸" } else { "▾" };
                tree = tree.push(
                    button(
                        text(format!("{arrow} {schema}  ({})", tables.len()))
                            .size(12)
                            .color(theme::MUTED)
                            .wrapping(text::Wrapping::None),
                    )
                    .on_press(Message::ToggleSchema(schema.clone()))
                    .style(theme::list_item(false))
                    .padding([3, 6])
                    .width(Fill)
                    .clip(true),
                );
            }
            if collapsed {
                continue;
            }
            for table in tables {
                tree = tree.push(self.table_entry(table, !schema.is_empty()));
            }
        }

        if session.tables.is_empty() {
            tree = tree.push(text("Aucune table").size(12).color(theme::MUTED));
        }

        tree = tree.push(
            button(text("+ Nouvelle table").size(12))
                .on_press_maybe(self.dialog_new_table().map(Message::OpenDialog))
                .style(theme::list_item(false))
                .padding([3, 6])
                .width(Fill),
        );

        column![entry, container(tree).padding(padding::left(12))].spacing(2).into()
    }

    /// Une table : clic = schéma + requête d'aperçu, « + » = insérer le nom
    /// au curseur de l'éditeur.
    pub(super) fn table_entry<'a>(&'a self, table: &'a TableRef, indent: bool) -> Element<'a, Message> {
        let active = self.selected.as_ref() == Some(table);
        let name = button(
            text(&table.name).size(12).font(MONO).wrapping(text::Wrapping::None),
        )
        .on_press(Message::TableSelected(table.clone()))
        .style(theme::list_item(active))
        .padding([3, 6])
        .width(Fill)
        .clip(true);

        let name = tooltip(name, text(table.to_string()).size(12).font(MONO), tooltip::Position::Right)
            .style(theme::tooltip)
            .padding(6)
            .gap(8);

        let insert = tooltip(
            button(container(text("+").size(13)).center_x(Fill))
                .on_press(Message::InsertTable(table.clone()))
                .style(theme::list_item(false))
                .padding([2, 0])
                .width(24),
            text("Insérer dans l'éditeur").size(12),
            tooltip::Position::Right,
        )
        .style(theme::tooltip)
        .padding(6)
        .gap(8);

        row![name, insert]
            .spacing(4)
            .align_y(iced::Center)
            .width(Fill)
            .padding(padding::left(if indent { 10.0 } else { 0.0 }))
            .into()
    }
}
