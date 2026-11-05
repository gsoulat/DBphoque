//! Gestionnaire de connexions et connecteurs.

use super::*;

impl App {
    /// Messages de la section « Gestionnaire de connexions ».
    pub(super) fn update_connections(&mut self, message: Message) -> Task<Message> {
        match message {

            Message::OpenManager => {
                self.manager = Some(match self.active_config() {
                    Some(config) => Manager::edit(config, self.catalog.clone()),
                    None => Manager::new(self.catalog.clone()),
                });
            }
            Message::CloseManager => self.manager = None,
            Message::EditConnection(id) => {
                let config = id.and_then(|id| self.connections.iter().find(|c| c.id == id));
                let catalog = self.catalog.clone();
                self.manager = Some(match config {
                    Some(c) => Manager::edit(c, catalog),
                    None => Manager::new(catalog),
                });
            }
            Message::FormChanged(field, value) => {
                if let Some(m) = &mut self.manager {
                    m.form.set(field, value);
                }
            }
            Message::FormExtra(key, value) => {
                if let Some(m) = &mut self.manager {
                    m.form.extra.insert(key, value);
                }
            }
            Message::FormEngine(engine) => {
                if let Some(m) = &mut self.manager {
                    m.form.set_engine(engine);
                }
            }
            Message::TogglePassword => {
                if let Some(m) = &mut self.manager {
                    m.show_password = !m.show_password;
                }
            }
            Message::OpenPlugins => {
                let catalog = self.catalog.clone();
                let m = self.manager.get_or_insert_with(|| Manager::new(catalog));
                m.plugins_page = true;
                m.installed = plugins::installed();
                if m.registry.is_none() {
                    return self.update(Message::PluginsRefresh);
                }
            }
            Message::PluginsRefresh => {
                if let Some(m) = &mut self.manager {
                    m.plugins_feedback = Some(Feedback::Busy("Lecture du registre…".into()));
                    m.installed = plugins::installed();
                }
                return Task::perform(plugins::fetch_registry(), Message::PluginsLoaded);
            }
            Message::PluginsLoaded(result) => {
                if let Ok(registry) = &result
                    && !registry.engines.is_empty()
                {
                    self.catalog = store::merge_catalog(Engine::builtins(), registry.engines.clone());
                    let _ = store::save_catalog(&self.catalog);
                    if let Some(m) = &mut self.manager {
                        m.catalog = self.catalog.clone();
                    }
                }
                if let Some(m) = &mut self.manager {
                    match result {
                        Ok(registry) => {
                            m.plugins_feedback = Some(Feedback::Ok(format!(
                                "{} connecteur(s) disponibles (registre {})",
                                registry.connectors.len(),
                                registry.tag
                            )));
                            m.registry = Some(registry);
                        }
                        Err(e) => m.plugins_feedback = Some(Feedback::Error(e)),
                    }
                }
            }
            Message::PluginInstall(id) => {
                let Some(m) = &mut self.manager else { return Task::none() };
                let Some(entry) = m.registry.as_ref().and_then(|r| r.connectors.iter().find(|c| c.id == id)).cloned() else {
                    return Task::none();
                };
                m.installing = Some(id.clone());
                m.plugins_feedback = Some(Feedback::Busy(format!("Téléchargement de {}…", entry.engine)));
                return Task::perform(plugins::install(entry), Message::PluginInstalled);
            }
            Message::PluginInstalled(result) => {
                if let Some(m) = &mut self.manager {
                    m.installing = None;
                    m.installed = plugins::installed();
                    m.plugins_feedback = Some(match result {
                        Ok(i) => Feedback::Ok(format!("{} {} installé", i.id, i.version)),
                        Err(e) => Feedback::Error(e),
                    });
                }
            }
            Message::PluginRemove(id) => {
                if let Some(m) = &mut self.manager {
                    m.plugins_feedback = Some(match plugins::remove(&id) {
                        Ok(()) => Feedback::Ok(format!("{id} supprimé")),
                        Err(e) => Feedback::Error(e),
                    });
                    m.installed = plugins::installed();
                }
            }
            Message::UpdateChecked(Ok(update)) => self.update = update,
            // Dépôt privé ou hors ligne : on reste silencieux.
            Message::UpdateChecked(Err(_)) => {}
            Message::OpenUrl(url) => {
                if let Err(e) = open::that(&url) {
                    self.status = Status::Error(format!("Impossible d'ouvrir {url} : {e}"));
                }
            }
            Message::TestConnection => {
                let Some(m) = &mut self.manager else { return Task::none() };
                let config = match m.form.to_config(m.editing.unwrap_or(0)) {
                    Ok(config) => config,
                    Err(e) => {
                        m.feedback = Some(Feedback::Error(e));
                        return Task::none();
                    }
                };
                let typed = (!m.form.password.is_empty()).then(|| m.form.password.clone());
                m.feedback = Some(Feedback::Busy("Test en cours…".into()));
                return Task::perform(
                    async move {
                        let (c, tunnel) = open(config, typed).await?;
                        Ok(format!(
                            "Connexion {} réussie{}",
                            c.engine(),
                            if tunnel.is_some() { " (via tunnel SSH)" } else { "" }
                        ))
                    },
                    Message::TestDone,
                );
            }
            Message::TestDone(result) => {
                if let Some(m) = &mut self.manager {
                    m.feedback = Some(match result {
                        Ok(msg) => Feedback::Ok(msg),
                        Err(e) => Feedback::Error(e),
                    });
                }
            }
            Message::SaveConnection { connect } => return self.save_connection(connect),
            Message::DeleteConnection => {
                let Some(id) = self.manager.as_ref().and_then(|m| m.editing) else {
                    return Task::none();
                };
                self.connections.retain(|c| c.id != id);
                if self.session.as_ref().is_some_and(|s| s.id == id) {
                    self.session = None;
                }
                let result = store::delete_password(id).and(store::save(&self.connections));
                let mut manager = Manager::new(self.catalog.clone());
                manager.feedback = Some(match result {
                    Ok(()) => Feedback::Ok("Connexion supprimée".into()),
                    Err(e) => Feedback::Error(e),
                });
                self.manager = Some(manager);
            }
            _ => unreachable!(),
        }
        Task::none()
    }

    pub(super) fn save_connection(&mut self, connect: bool) -> Task<Message> {
        let Some(m) = &mut self.manager else { return Task::none() };
        let id = m
            .editing
            .unwrap_or_else(|| self.connections.iter().map(|c| c.id).max().unwrap_or(0) + 1);

        let config = match m.form.to_config(id) {
            Ok(config) => config,
            Err(e) => {
                m.feedback = Some(Feedback::Error(e));
                return Task::none();
            }
        };

        let mut result = Ok(());
        if config.engine.is_server() && !m.form.password.is_empty() {
            result = store::set_password(id, &m.form.password);
        }
        // Champs secrets : au trousseau, et jamais dans le fichier.
        for field in config.engine.fields.iter().filter(|f| f.secret) {
            if let Some(v) = m.form.extra.get(&field.key).filter(|v| !v.is_empty()) {
                result = result.and(store::set_secret(id, &field.key, v));
            }
        }
        match self.connections.iter_mut().find(|c| c.id == id) {
            Some(existing) => *existing = config,
            None => self.connections.push(config),
        }
        let result = result.and(store::save(&self.connections));

        m.editing = Some(id);
        m.form.password.clear();
        m.feedback = Some(match &result {
            Ok(()) => Feedback::Ok("Connexion enregistrée".into()),
            Err(e) => Feedback::Error(e.clone()),
        });

        if connect && result.is_ok() {
            self.manager = None;
            return self.update(Message::Connect(id));
        }
        Task::none()
    }

    // ----------------------------------------------------------------------
    // Vues
    // ----------------------------------------------------------------------
}
