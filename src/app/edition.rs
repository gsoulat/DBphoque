//! Menu Édition, onglets Schéma et Données.

use super::*;
use iced::widget::column;

impl App {
    /// Messages de la section « Édition ».
    pub(super) fn update_edition(&mut self, message: Message) -> Task<Message> {
        match message {

            Message::LoadData => {
                let (Some(c), Some(table)) = (self.connector(), self.selected.clone()) else {
                    return Task::none();
                };
                self.status = Status::Busy(format!("Lecture de {table}…"));
                let sql = c.data_sql(&table);
                let t = table.clone();
                return Task::perform(
                    async move { c.query(&sql).await },
                    move |r| Message::DataLoaded(t.clone(), r),
                );
            }
            Message::DataLoaded(table, Ok(result)) => {
                self.status = Status::Info(format!("{} ligne(s) dans {table}", result.rows.len()));
                self.data = Some(result);
                self.data_table = Some(table);
                self.data_row = None;
                self.editing_cell = None;
            }
            Message::DataLoaded(_, Err(e)) => self.status = Status::Error(e),
            Message::DataRowSelected(i) => {
                self.editing_cell = None;
                self.data_row = if self.data_row == Some(i) { None } else { Some(i) };
            }
            Message::SchemaRowSelected(i) => {
                self.schema_row = if self.schema_row == Some(i) { None } else { Some(i) };
            }
            Message::ToggleEditMenu => self.edit_menu = !self.edit_menu,
            Message::Noop => {}
            Message::OpenDialog(dialog) => {
                self.dialog = Some(dialog);
                self.dialog_error = None;
                self.edit_menu = false;
            }
            Message::Dialog(DialogMessage::Cancel) => {
                self.dialog = None;
                self.dialog_error = None;
            }
            Message::Dialog(DialogMessage::Submit) => {
                let (Some(dialog), Some(c)) = (&self.dialog, self.connector()) else {
                    return Task::none();
                };
                match dialog.to_sql(c.as_ref()) {
                    Ok(pending) => {
                        self.dialog = Some(Dialog::Confirm(pending));
                        self.dialog_error = None;
                    }
                    Err(e) => self.dialog_error = Some(e),
                }
            }
            Message::Dialog(DialogMessage::Execute) => {
                let (Some(Dialog::Confirm(pending)), Some(c)) = (&self.dialog, self.connector()) else {
                    return Task::none();
                };
                let (sql, refresh, select) = (pending.sql.clone(), pending.refresh, pending.select.clone());
                let extra = pending.extra.clone();
                self.dialog = None;
                self.status = Status::Busy(if extra.is_empty() {
                    "Exécution…".into()
                } else {
                    format!("Exécution de {} instructions…", extra.len() + 1)
                });
                return Task::perform(
                    async move {
                        let mut total = c.execute(&sql).await?;
                        for statement in extra {
                            total += c.execute(&statement).await?;
                        }
                        Ok(total)
                    },
                    move |r| Message::Executed(refresh, select.clone(), r),
                );
            }
            Message::Dialog(message) => {
                if let Some(dialog) = &mut self.dialog {
                    dialog.apply(message);
                }
            }
            Message::Executed(refresh, select, Ok(affected)) => {
                self.status = Status::Info(format!("Exécuté · {affected} ligne(s) touchée(s)"));
                let Some(c) = self.connector() else { return Task::none() };
                return match refresh {
                    Refresh::Data => self.update(Message::LoadData),
                    Refresh::Structure => {
                        // On recharge la liste, puis le schéma de la table
                        // concernée (créée, modifiée ou renommée) ; `TablesLoaded`
                        // s'en charge d'après `selected`.
                        self.selected = select;
                        self.schema.clear();
                        self.schema_row = None;
                        self.data = None;
                        self.data_table = None;
                        self.diagram = None;
                        let _ = c;
                        self.reload_structure()
                    }
                };
            }
            Message::Executed(_, _, Err(e)) => self.status = Status::Error(e),
            _ => unreachable!(),
        }
        Task::none()
    }

    pub(super) fn dialog_new_schema(&self) -> Option<Dialog> {
        let c = self.connector()?;
        c.create_schema_sql("x")?;
        Some(Dialog::Schema(editing::SchemaForm { name: String::new() }))
    }

    /// Supprime le schéma de la table sélectionnée (vide uniquement).
    pub(super) fn dialog_drop_schema(&self) -> Option<Dialog> {
        let c = self.connector()?;
        let schema = self.selected.as_ref()?.schema.clone()?;
        let sql = c.drop_schema_sql(&schema)?;
        Some(Dialog::Confirm(editing::Pending {
            title: format!("Supprimer le schéma {schema} (doit être vide)"),
            sql,
            refresh: Refresh::Structure,
            select: None,
            extra: Vec::new(),
        }))
    }

    pub(super) fn dialog_new_table(&self) -> Option<Dialog> {
        let session = self.session.as_ref()?;
        let config = self.active_config()?;
        let mut schemas = session.schemas.clone();
        if schemas.is_empty() {
            schemas = session.tables.iter().filter_map(|t| t.schema.clone()).collect();
            schemas.sort_unstable();
            schemas.dedup();
        }
        let _ = config;
        Some(Dialog::Table(TableForm::new(schemas, &self.dialect().id_type)))
    }

    pub(super) fn dialog_rename_table(&self) -> Option<Dialog> {
        let table = self.selected.clone()?;
        Some(Dialog::Rename(RenameForm { table, column: None, to: String::new() }))
    }

    pub(super) fn dialog_drop_table(&self) -> Option<Dialog> {
        let (table, c) = (self.selected.as_ref()?, self.connector()?);
        Some(Dialog::Confirm(editing::Pending {
            title: format!("Supprimer la table {table}"),
            sql: c.drop_table_sql(table),
            refresh: Refresh::Structure,
            select: None,
            extra: Vec::new(),
        }))
    }

    pub(super) fn dialog_add_column(&self) -> Option<Dialog> {
        let table = self.selected.clone()?;
        Some(Dialog::Column(editing::ColumnForm { table, draft: editing::ColumnDraft::new() }))
    }

    pub(super) fn dialog_rename_column(&self) -> Option<Dialog> {
        let (table, column) = (self.selected.clone()?, self.selected_column()?);
        Some(Dialog::Rename(RenameForm { table, column: Some(column), to: String::new() }))
    }

    pub(super) fn dialog_drop_column(&self) -> Option<Dialog> {
        let (table, column, c) = (self.selected.as_ref()?, self.selected_column()?, self.connector()?);
        Some(Dialog::Confirm(editing::Pending {
            title: format!("Supprimer la colonne {column}"),
            sql: c.alter_table_sql(table, &connector::Alter::DropColumn(column)),
            refresh: Refresh::Structure,
            select: Some(table.clone()),
            extra: Vec::new(),
        }))
    }

    pub(super) fn dialog_insert_row(&self) -> Option<Dialog> {
        let table = self.selected.clone()?;
        if self.schema.is_empty() {
            return None;
        }
        Some(Dialog::Row(RowForm::insert(table, self.schema.clone())))
    }

    pub(super) fn dialog_update_row(&self) -> Option<Dialog> {
        let (table, row) = (self.selected.clone()?, self.selected_data_row()?);
        if !self.schema.iter().any(|c| c.primary_key) {
            return None;
        }
        Some(Dialog::Row(RowForm::update(table, self.schema.clone(), row)))
    }

    pub(super) fn dialog_delete_row(&self) -> Option<Dialog> {
        let (table, row, c) = (self.selected.as_ref()?, self.selected_data_row()?, self.connector()?);
        let key: Vec<_> = self
            .schema
            .iter()
            .enumerate()
            .filter(|(_, col)| col.primary_key)
            .filter_map(|(i, col)| Some((col.name.clone(), row.get(i)?.clone())))
            .collect();
        if key.is_empty() {
            return None;
        }
        Some(Dialog::Confirm(editing::Pending {
            title: format!("Supprimer une ligne de {table}"),
            sql: c.delete_sql(table, &key),
            refresh: Refresh::Data,
            select: Some(table.clone()),
            extra: Vec::new(),
        }))
    }

    pub(super) fn selected_column(&self) -> Option<String> {
        self.schema_row.and_then(|i| self.schema.get(i)).map(|c| c.name.clone())
    }

    pub(super) fn selected_data_row(&self) -> Option<&Vec<connector::Cell>> {
        let data = self.data.as_ref().filter(|_| self.data_table == self.selected)?;
        data.rows.get(self.data_row?)
    }

    /// Menu « Édition » déroulé sous le bouton de la barre du haut.
    pub(super) fn edit_menu(&self) -> Element<'_, Message> {
        let item = |label: &'static str, dialog: Option<Dialog>, danger: bool| {
            let color = match (dialog.is_some(), danger) {
                (false, _) => Color { a: 0.4, ..theme::MUTED },
                (true, true) => theme::DANGER,
                (true, false) => theme::TEXT,
            };
            button(text(label).size(13).color(color))
                .on_press_maybe(dialog.map(Message::OpenDialog))
                .style(theme::list_item(false))
                .padding([5, 12])
                .width(Fill)
        };
        let section = |label: &'static str| {
            container(text(label).size(11).color(theme::MUTED)).padding(padding::top(8).bottom(2).left(12))
        };
        let table_name = self.selected.as_ref().map_or("aucune table sélectionnée".to_string(), |t| t.to_string());

        let menu = column![
            container(text(table_name).size(12).color(theme::PRIMARY)).padding([6, 12]),
            section("SCHÉMAS"),
            item("Nouveau schéma…", self.dialog_new_schema(), false),
            item("Supprimer le schéma de la table sélectionnée…", self.dialog_drop_schema(), true),
            section("TABLE"),
            item("Nouvelle table…", self.dialog_new_table(), false),
            item("Renommer la table…", self.dialog_rename_table(), false),
            item("Supprimer la table…", self.dialog_drop_table(), true),
            section("COLONNES"),
            item("Ajouter une colonne…", self.dialog_add_column(), false),
            item("Renommer la colonne sélectionnée…", self.dialog_rename_column(), false),
            item("Supprimer la colonne sélectionnée…", self.dialog_drop_column(), true),
            section("LIGNES"),
            item("Ajouter une ligne…", self.dialog_insert_row(), false),
            item("Modifier la ligne sélectionnée…", self.dialog_update_row(), false),
            item("Supprimer la ligne sélectionnée…", self.dialog_delete_row(), true),
            section("FICHIERS"),
            button(text("Importer un fichier (CSV, Excel, Parquet)…").size(13).color(theme::TEXT))
                .on_press(Message::ImportPick)
                .style(theme::list_item(false))
                .padding([5, 12])
                .width(Fill),
        ]
        .width(320);

        let panel = container(menu).style(theme::panel).padding(6);

        // Un clic hors du menu le referme ; dans le menu, il est absorbé.
        iced::widget::mouse_area(
            container(iced::widget::mouse_area(panel).on_press(Message::Noop))
            .align_right(Fill)
            .padding(padding::top(52).right(12))
            .width(Fill)
            .height(Fill),
        )
        .on_press(Message::ToggleEditMenu)
        .into()
    }

    pub(super) fn schema_view(&self) -> Element<'_, Message> {
        let Some(table) = &self.selected else {
            return hint("Sélectionne une table dans la liste pour voir son schéma.");
        };
        let header = ["Colonne", "Type", "NULL", "Défaut", "Clé"].map(String::from).to_vec();
        let rows = self.schema.iter().map(|c| {
            vec![
                c.name.clone(),
                c.data_type.clone(),
                if c.nullable { "oui" } else { "non" }.into(),
                c.default.clone().unwrap_or_default(),
                if c.primary_key { "PK" } else { "" }.into(),
            ]
        });

        let toolbar = row![
            text(table.to_string()).size(15).color(theme::PRIMARY).width(Fill),
            action_button("+ Colonne", self.dialog_add_column(), false),
            action_button("Renommer la colonne", self.dialog_rename_column(), false),
            action_button("Supprimer la colonne", self.dialog_drop_column(), true),
            action_button("Renommer la table", self.dialog_rename_table(), false),
            action_button("Supprimer la table", self.dialog_drop_table(), true),
        ]
        .spacing(6)
        .align_y(iced::Center);

        column![
            toolbar,
            text("Clique sur une colonne pour la sélectionner.").size(11).color(theme::MUTED),
            Grid::new(header, rows.enumerate())
                .selected(self.schema_row)
                .action(CellAction::SelectRow(Message::SchemaRowSelected))
                .build(),
        ]
        .spacing(6)
        .into()
    }

    pub(super) fn data_view(&self) -> Element<'_, Message> {
        let Some(table) = &self.selected else {
            return hint("Sélectionne une table dans la liste pour voir et modifier ses lignes.");
        };
        let Some(data) = self.data.as_ref().filter(|_| self.data_table.as_ref() == Some(table)) else {
            return hint("Lecture des lignes…");
        };

        let has_pk = self.schema.iter().any(|c| c.primary_key);

        let mut toolbar = row![text(table.to_string()).size(15).color(theme::PRIMARY).width(Fill)];
        if self.editing_cell.is_some() {
            toolbar = toolbar
                .push(text("Cellule en cours de modification").size(12).color(theme::WARNING))
                .push(button(text("Enregistrer").size(12)).on_press(Message::CellEditSubmit).padding([3, 10]))
                .push(
                    button(text("Annuler").size(12))
                        .on_press(Message::CellEditCancel)
                        .style(button::secondary)
                        .padding([3, 10]),
                );
        }
        let toolbar = toolbar.push(action_button("+ Ligne", self.dialog_insert_row(), false))
            .push(action_button("Modifier", self.dialog_update_row(), false))
            .push(action_button("Supprimer", self.dialog_delete_row(), true));
        let toolbar = row![
            toolbar,
            button(text("Rafraîchir").size(12)).on_press(Message::LoadData).style(button::secondary).padding([3, 10]),
        ]
        .spacing(6)
        .align_y(iced::Center);

        let note = if !has_pk {
            "Cette table n'a pas de clé primaire : on peut insérer, mais pas modifier ni supprimer une ligne sans risque."
        } else if data.rows.len() >= connector::MAX_ROWS {
            "Affichage limité aux premières lignes. Clic : sélectionner · double-clic sur une cellule : la modifier · en-tête : trier."
        } else {
            "Clic : sélectionner · double-clic sur une cellule : la modifier, puis Enregistrer · en-tête : trier."
        };

        let order = order_rows(&data.rows, self.data_sort, &self.data_filter);
        let rows = order.iter().map(|&i| (i, data.rows[i].iter().map(|c| c.display()).collect()));
        let mut grid = Grid::new(data.columns.clone(), rows)
            .selected(self.data_row)
            .action(CellAction::SelectRow(Message::DataRowSelected))
            .sort(self.data_sort, Message::DataSort);
        if has_pk {
            grid = grid.editable(self.editing_cell.as_ref().map(|(r, c, t)| (*r, *c, t.as_str())));
        }

        let filter = text_input("Filtrer…", &self.data_filter)
            .on_input(Message::DataFilter)
            .size(12)
            .padding([3, 8])
            .width(180);

        column![
            toolbar,
            row![text(note).size(11).color(theme::MUTED).width(Fill), filter, export_buttons(ExportSource::Data)]
                .spacing(8)
                .align_y(iced::Center),
            grid.build(),
        ]
        .spacing(6)
        .into()
    }
}
