//! Onglet Diagramme et mode Conception.

use super::*;
use iced::widget::column;

impl App {
    /// Messages de la section « Diagramme ».
    pub(super) fn update_diagram_view(&mut self, message: Message) -> Task<Message> {
        match message {

            Message::LoadDiagram => {
                let Some(session) = &self.session else { return Task::none() };
                let c = session.connector.clone();
                let tables = session.tables.clone();
                self.diagram_loading = true;
                self.status = Status::Busy(format!("Lecture de {} tables…", tables.len()));
                return Task::perform(
                    async move {
                        let described = futures_util::future::try_join_all(tables.into_iter().map(|t| {
                            let c = c.clone();
                            async move { c.describe_table(&t).await.map(|cols| (t, cols)) }
                        }))
                        .await?;
                        let relations = c.list_relations().await?;
                        Ok((described, relations))
                    },
                    Message::DiagramLoaded,
                );
            }
            Message::DiagramLoaded(Ok((tables, relations))) => {
                self.diagram_loading = false;
                self.status = Status::Info(format!(
                    "{} tables, {} relations",
                    tables.len(),
                    relations.len()
                ));
                self.diagram = Some(Diagram::new(tables, relations));
            }
            Message::DiagramLoaded(Err(e)) => {
                self.diagram_loading = false;
                self.status = Status::Error(e);
            }
            Message::DiagramMode(mode) => {
                if let Some(d) = &mut self.diagram {
                    d.set_mode(mode);
                }
            }
            Message::DiagramLayout => {
                if let Some(d) = &mut self.diagram {
                    d.auto_layout();
                }
            }
            Message::DiagramFit => {
                if let Some(d) = &mut self.diagram {
                    d.fit();
                }
            }
            Message::DiagramInteraction(diagram::Interaction::Link { from, to }) => {
                self.design.pending_link = Some((from, to));
            }
            Message::DiagramInteraction(interaction) => {
                if let Some(d) = &mut self.diagram {
                    d.apply(interaction);
                }
            }
            Message::Design(message) => return self.update_design(message),
            _ => unreachable!(),
        }
        Task::none()
    }

    pub(super) fn update_design(&mut self, message: DesignMessage) -> Task<Message> {
        use DesignMessage as M;

        let id_type = self.dialect().id_type;
        let id_type = id_type.as_str();

        // Le connecteur est pris avant l'emprunt mutable du diagramme.
        let connector = self.connector();

        // Entrée en conception sans diagramme chargé : on part de vide.
        if matches!(message, M::Toggle | M::Clear) && self.diagram.is_none() {
            let mut d = Diagram::empty();
            d.editable = true;
            self.diagram = Some(d);
            if matches!(message, M::Toggle) {
                return Task::none();
            }
        }
        let Some(d) = &mut self.diagram else { return Task::none() };
        let selected = d.selected;

        match message {
            M::Toggle => {
                d.editable = !d.editable;
                self.design.custom.clear();
            }
            M::Clear => {
                let mut fresh = Diagram::empty();
                fresh.editable = true;
                *d = fresh;
                self.design.custom.clear();
            }
            M::AddTable => {
                let schema = d.nodes.iter().find_map(|n| n.table.schema.clone());
                d.add_table(schema, id_type);
            }
            M::RemoveTable => {
                if let Some(i) = selected {
                    d.remove_table(i);
                    self.design.custom.retain(|(t, _)| *t != i);
                }
            }
            M::TableName(name) => {
                if let Some(i) = selected {
                    d.rename_table(i, &name);
                }
            }
            M::ColumnName(c, v) => {
                if let Some(i) = selected {
                    d.edit_column(i, c, |col| col.name = v);
                }
            }
            M::ColumnType(c, v) => {
                if let Some(i) = selected {
                    d.edit_column(i, c, |col| col.data_type = v);
                }
            }
            M::ColumnCustom(c) => {
                if let Some(i) = selected
                    && !self.design.custom.remove(&(i, c))
                {
                    self.design.custom.insert((i, c));
                }
            }
            M::ColumnNullable(c, v) => {
                if let Some(i) = selected {
                    d.edit_column(i, c, |col| col.nullable = v);
                }
            }
            M::ColumnPrimary(c, v) => {
                if let Some(i) = selected {
                    d.edit_column(i, c, |col| {
                        col.primary_key = v;
                        if v {
                            col.nullable = false;
                        }
                    });
                }
            }
            M::AddColumn => {
                if let Some(i) = selected {
                    d.add_column(i);
                }
            }
            M::RemoveColumn(c) => {
                if let Some(i) = selected {
                    d.remove_column(i, c);
                    self.design.custom.remove(&(i, c));
                }
            }
            M::RemoveRelation(r) => d.remove_relation(r),
            M::LinkChosen(kind) => {
                if let Some((from, to)) = self.design.pending_link.take() {
                    match d.link(from, to, kind) {
                        Ok(()) => self.status = Status::Info("Relation ajoutée".into()),
                        Err(e) => self.status = Status::Error(e),
                    }
                }
            }
            M::LinkCancel => self.design.pending_link = None,
            M::Script => {
                let Some(c) = connector else {
                    self.status = Status::Error("Ouvre une connexion pour générer le script".into());
                    return Task::none();
                };
                match d.to_ddl(c.as_ref()) {
                    Ok(sql) => {
                        let count = d.nodes.iter().filter(|n| !n.existing).count();
                        self.dialog = Some(Dialog::Confirm(editing::Pending {
                            title: format!("Créer {count} table(s) d'après le diagramme"),
                            sql,
                            refresh: Refresh::Structure,
                            select: None,
                            extra: Vec::new(),
                        }));
                        self.dialog_error = None;
                    }
                    Err(e) => self.status = Status::Error(e),
                }
            }
        }
        Task::none()
    }

    pub(super) fn diagram_view(&self) -> Element<'_, Message> {
        let Some(d) = &self.diagram else {
            return if self.diagram_loading {
                hint("Lecture des tables et des relations…")
            } else {
                let mut actions = row![].spacing(10);
                if self.session.is_some() {
                    actions = actions.push(
                        button(text("Charger le diagramme de la base").size(13))
                            .on_press(Message::LoadDiagram)
                            .padding([6, 14]),
                    );
                }
                actions = actions.push(
                    button(text("Concevoir un nouveau schéma").size(13))
                        .on_press(Message::Design(DesignMessage::Toggle))
                        .style(button::secondary)
                        .padding([6, 14]),
                );
                container(
                    column![
                        actions,
                        text("La conception dessine des tables virtuelles ; le script généré les crée dans la base connectée.")
                            .size(12)
                            .color(theme::MUTED),
                    ]
                    .spacing(10)
                    .align_x(iced::Center),
                )
                .center(Fill)
                .into()
            };
        };

        let mode = |m: diagram::Mode| {
            button(text(m.label()).size(12))
                .on_press(Message::DiagramMode(m))
                .style(theme::list_item(d.mode == m))
                .padding([3, 10])
        };
        let tool = |label, message| {
            button(text(label).size(12))
                .on_press(message)
                .style(button::secondary)
                .padding([3, 10])
        };
        let modes = Row::with_children(diagram::Mode::ALL.map(|m| mode(m).into())).spacing(4);
        let design_toggle = button(text(if d.editable { "Conception ✓" } else { "Conception" }).size(12))
            .on_press(Message::Design(DesignMessage::Toggle))
            .style(theme::outline(if d.editable { theme::SUCCESS } else { theme::PRIMARY }))
            .padding([3, 10]);

        let mut toolbar = row![modes, design_toggle].spacing(6).align_y(iced::Center);
        if d.editable {
            toolbar = toolbar
                .push(tool("+ Table", Message::Design(DesignMessage::AddTable)))
                .push(tool("Générer le script", Message::Design(DesignMessage::Script)))
                .push(tool("Vider", Message::Design(DesignMessage::Clear)));
        }
        toolbar = toolbar
            .push(text(d.mode.description()).size(12).color(theme::MUTED).width(Fill))
            .push(tool("Réorganiser", Message::DiagramLayout))
            .push(tool("Ajuster", Message::DiagramFit))
            .push(tool("Recharger", Message::LoadDiagram));

        let hint_text = text(if d.editable {
            "Tire la poignée ● d'une table jusqu'à une autre pour les lier · clic sur une table : la modifier à droite"
        } else if d.relations.is_empty() && !d.nodes.is_empty() {
            "Aucune clé étrangère déclarée : les tables s'affichent sans liens."
        } else {
            "Molette : zoom · glisser le fond : déplacer · glisser une boîte : la ranger"
        })
        .size(11)
        .color(theme::MUTED);

        let canvas = canvas(d).width(Fill).height(Fill);
        let canvas = Element::from(canvas).map(Message::DiagramInteraction);

        let body: Element<'_, Message> = if d.editable {
            row![canvas, design::panel(&self.design, d, &self.dialect_cache)].spacing(8).into()
        } else {
            canvas
        };

        let view = column![toolbar, hint_text, body].spacing(6);
        match self.design.pending_link {
            Some((from, to)) if from < d.nodes.len() && to < d.nodes.len() => {
                stack![view, design::link_prompt(d, from, to)].into()
            }
            _ => view.into(),
        }
    }
}
