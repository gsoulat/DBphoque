//! État de l'application, messages et vues (architecture Elm d'iced).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::widget::text_editor::{Action, Binding, Edit, KeyPress};
use iced::widget::{
    Column, Row, button, canvas, column, container, pane_grid, row, scrollable, stack, text,
    text_editor, text_input, tooltip,
};
use iced::{Color, Element, Event, Fill, Font, Length, Subscription, Task, Theme, highlighter, padding};

use crate::connector::{
    self, ColumnInfo, Connector, Dialect, QueryPlan, QueryResult, Relation, Result, TableRef,
    execution_order,
};
use crate::completion::{self, Completion};
use crate::design::{self, Design, DesignMessage};
use crate::diagram::{self, Diagram};
use crate::editing::{self, Dialog, DialogMessage, Refresh, RenameForm, RowForm, TableForm};
use crate::export::{self, Format};
use crate::import::{self, ImportForm, ImportMessage};
use crate::manager::{self, Feedback, Field, Manager};
use crate::plugins;
use crate::store::{self, ConnectionConfig, Engine, HistoryEntry, Location, SavedQuery};
use crate::theme;
use crate::tunnel::Tunnel;

mod connections;
mod diagram_view;
mod edition;
mod grid;
mod sidebar;
mod workspace;

use grid::*;

const MONO: Font = Font::MONOSPACE;
/// Largeur approximative d'un caractère en police mono de 13 px.
const CHAR_WIDTH: f32 = 7.8;
const MIN_CELL: f32 = 70.0;
const MAX_CELL: f32 = 420.0;
const PAGE_SIZE: usize = 200;
const SIDEBAR_RATIO: f32 = 0.2;
const EDITOR_RATIO: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Results,
    Data,
    Plan,
    Schema,
    Diagram,
    History,
}

/// Un onglet de requête : son éditeur et ce qu'il a produit.
pub struct QueryTab {
    id: u64,
    name: String,
    editor: text_editor::Content,
    result: Option<QueryResult>,
    /// Requête qui a produit `result`, pour ne pas confondre avec l'éditeur.
    result_sql: Option<String>,
    page: usize,
    plan: Option<QueryPlan>,
    plan_sql: Option<String>,
    /// Tri côté client : (colonne, croissant).
    sort: Option<(usize, bool)>,
    filter: String,
}

impl QueryTab {
    fn new(id: u64, name: String, sql: &str) -> Self {
        Self {
            id,
            name,
            editor: text_editor::Content::with_text(sql),
            result: None,
            result_sql: None,
            page: 0,
            plan: None,
            plan_sql: None,
            sort: None,
            filter: String::new(),
        }
    }

    fn saved(&self) -> SavedQuery {
        SavedQuery { name: self.name.clone(), sql: self.editor.text() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSource {
    Results,
    Data,
}

/// Que fait un clic sur une cellule de la grille.
enum CellAction {
    Copy,
    SelectRow(fn(usize) -> Message),
}

#[derive(Debug, Clone, Copy)]
enum Pane {
    Sidebar,
    Workspace,
}

/// Panneaux de l'espace de travail : éditeur en haut, résultats en bas.
#[derive(Debug, Clone, Copy)]
enum WorkPane {
    Editor,
    Results,
}

#[derive(Debug, Clone)]
enum Status {
    Idle,
    Busy(String),
    Info(String),
    Error(String),
}

/// La connexion ouverte et ce qu'on en a chargé.
struct Session {
    id: u64,
    connector: Arc<dyn Connector>,
    tables: Vec<TableRef>,
    schemas: Vec<String>,
    /// Tunnel SSH gardé ouvert le temps de la session.
    _tunnel: Option<Arc<Tunnel>>,
}

pub struct App {
    connections: Vec<ConnectionConfig>,
    session: Option<Session>,
    /// Connexion en cours d'ouverture (pour l'indicateur dans la barre latérale).
    connecting: Option<u64>,
    manager: Option<Manager>,
    tabs: Vec<QueryTab>,
    active: usize,
    next_tab_id: u64,
    history: Vec<HistoryEntry>,
    panes: pane_grid::State<Pane>,
    work_panes: pane_grid::State<WorkPane>,
    sidebar_hidden: bool,
    results_hidden: bool,
    /// Filtre de la liste des tables.
    filter: String,
    /// Schémas repliés dans la barre latérale.
    collapsed: HashSet<String>,
    /// Connexions dont la liste de tables est repliée.
    folded: HashSet<u64>,
    diagram: Option<Diagram>,
    diagram_loading: bool,
    design: Design,
    /// Dialecte de la session (copie pour les vues, qui empruntent `&self`).
    dialect_cache: Dialect,
    /// Colonnes connues, pour l'autocomplétion (chargées en arrière-plan).
    columns_cache: HashMap<TableRef, Vec<ColumnInfo>>,
    completion: Option<Completion>,
    /// Lignes de la table sélectionnée (onglet Données).
    data: Option<QueryResult>,
    data_table: Option<TableRef>,
    data_row: Option<usize>,
    data_sort: Option<(usize, bool)>,
    data_filter: String,
    /// Cellule en cours d'édition dans l'onglet Données : (ligne, colonne, texte).
    editing_cell: Option<(usize, usize, String)>,
    /// Colonne sélectionnée dans l'onglet Schéma.
    schema_row: Option<usize>,
    dialog: Option<Dialog>,
    dialog_error: Option<String>,
    import: Option<ImportForm>,
    import_error: Option<String>,
    /// Menu « Édition » déroulé.
    edit_menu: bool,
    /// Nouvelle version de DBphoque publiée sur GitHub.
    update: Option<plugins::AppUpdate>,
    /// Moteurs proposés : intégrés + registre (mis en cache).
    catalog: Vec<Engine>,
    selected: Option<TableRef>,
    schema: Vec<ColumnInfo>,
    tab: Tab,
    status: Status,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Barre latérale
    Connect(u64),
    Connected(u64, Result<(Arc<dyn Connector>, Option<Arc<Tunnel>>)>),
    Disconnect,
    TablesLoaded(Result<Vec<TableRef>>),
    SchemasLoaded(Result<Vec<String>>),
    ColumnsCached(Vec<(TableRef, Vec<ColumnInfo>)>),
    CompletionMove(i32),
    CompletionAccept,
    CompletionClose,
    TableSelected(TableRef),
    InsertTable(TableRef),
    SchemaLoaded(Result<Vec<ColumnInfo>>),
    FilterChanged(String),
    ToggleSchema(String),
    ToggleFold(u64),
    ToggleFolder(String),
    PaneResized(pane_grid::ResizeEvent),
    WorkPaneResized(pane_grid::ResizeEvent),
    ToggleSidebar,
    ToggleResults,
    /// Relit tables, schémas, table sélectionnée et diagramme.
    Refresh,

    // Édition
    LoadData,
    DataLoaded(TableRef, Result<QueryResult>),
    DataRowSelected(usize),
    SchemaRowSelected(usize),
    OpenDialog(Dialog),
    Dialog(DialogMessage),
    Executed(Refresh, Option<TableRef>, Result<u64>),
    ToggleEditMenu,
    /// Absorbe un clic (par exemple dans le menu) sans rien faire.
    Noop,

    // Diagramme
    LoadDiagram,
    DiagramLoaded(Result<(Vec<(TableRef, Vec<ColumnInfo>)>, Vec<Relation>)>),
    DiagramMode(diagram::Mode),
    DiagramLayout,
    DiagramFit,
    DiagramInteraction(diagram::Interaction),
    Design(DesignMessage),

    // Gestionnaire de connexions
    OpenManager,
    CloseManager,
    EditConnection(Option<u64>),
    FormChanged(Field, String),
    FormExtra(String, String),
    FormEngine(Engine),
    TogglePassword,
    OpenPlugins,
    PluginsRefresh,
    PluginsLoaded(Result<plugins::Registry>),
    PluginInstall(String),
    PluginInstalled(Result<plugins::Installed>),
    PluginRemove(String),
    UpdateChecked(Result<Option<plugins::AppUpdate>>),
    OpenUrl(String),
    TestConnection,
    TestDone(Result<String>),
    SaveConnection { connect: bool },
    DeleteConnection,

    // Espace de travail
    Edit(Action),
    Explain,
    Explained(u64, String, Result<QueryPlan>),
    Run,
    QueryDone(u64, String, Result<QueryResult>, Duration),
    QueryTabNew,
    QueryTabSelect(u64),
    QueryTabClose(u64),
    QueryTabRename(String),
    /// Ouvre une requête de l'historique dans un nouvel onglet.
    HistoryOpen(String),
    TabSelected(Tab),
    PageChanged(usize),
    CopyCell(String),
    ResultSort(usize),
    ResultFilter(String),
    DataSort(usize),
    DataFilter(String),
    CellEditStart(usize, usize),
    CellEditChanged(String),
    CellEditSubmit,
    CellEditCancel,
    /// Exporte les lignes affichées (filtrées, triées) de la source.
    Export(ExportSource, Format),
    Exported(Result<Option<std::path::PathBuf>>),
    ImportPick,
    ImportRead(Result<Option<(std::path::PathBuf, import::ImportedFile)>>),
    Import(ImportMessage),
    KeyPressed(Key, Modifiers),
}

/// Ouvre une connexion : mot de passe depuis le trousseau, tunnel SSH si
/// demandé, puis connecteur.
async fn open(
    config: ConnectionConfig,
    typed_password: Option<String>,
) -> Result<(Arc<dyn Connector>, Option<Arc<Tunnel>>)> {
    let password = match typed_password {
        Some(password) => Some(password),
        None if config.engine.is_server() => store::password(config.id)?,
        None => None,
    };
    // Champs déclarés par le moteur : valeurs du fichier + secrets du trousseau.
    let mut options = config.extra.clone();
    for field in config.engine.fields.iter().filter(|f| f.secret) {
        if let Some(v) = store::secret(config.id, &field.key)? {
            options.insert(field.key.clone(), v);
        }
    }
    if config.uses_tunnel() {
        let tunnel = Tunnel::open(&config).await?;
        // La base est atteinte par le port local du tunnel.
        let local = ConnectionConfig { host: "127.0.0.1".into(), port: Some(tunnel.local_port), ..config };
        let connector = connector::connect(&local, password, options).await?;
        return Ok((connector, Some(Arc::new(tunnel))));
    }
    Ok((connector::connect(&config, password, options).await?, None))
}

impl App {
    pub fn new() -> Self {
        crate::icon::set_dock_icon();

        let (connections, status) = match store::load() {
            Ok(connections) => (connections, Status::Idle),
            Err(e) => (Vec::new(), Status::Error(e)),
        };
        // Premier lancement : on ouvre directement le gestionnaire.
        let catalog = store::load_catalog();
        let manager = connections.is_empty().then(|| Manager::new(catalog.clone()));

        let panes = pane_grid::State::with_configuration(pane_grid::Configuration::Split {
            axis: pane_grid::Axis::Vertical,
            ratio: SIDEBAR_RATIO,
            a: Box::new(pane_grid::Configuration::Pane(Pane::Sidebar)),
            b: Box::new(pane_grid::Configuration::Pane(Pane::Workspace)),
        });
        let work_panes = pane_grid::State::with_configuration(pane_grid::Configuration::Split {
            axis: pane_grid::Axis::Horizontal,
            ratio: EDITOR_RATIO,
            a: Box::new(pane_grid::Configuration::Pane(WorkPane::Editor)),
            b: Box::new(pane_grid::Configuration::Pane(WorkPane::Results)),
        });

        Self {
            connections,
            session: None,
            connecting: None,
            manager,
            panes,
            work_panes,
            sidebar_hidden: false,
            results_hidden: false,
            filter: String::new(),
            collapsed: HashSet::new(),
            folded: HashSet::new(),
            diagram: None,
            diagram_loading: false,
            design: Design::default(),
            dialect_cache: editing::default_dialect(),
            columns_cache: HashMap::new(),
            completion: None,
            data: None,
            data_table: None,
            data_row: None,
            data_sort: None,
            data_filter: String::new(),
            editing_cell: None,
            schema_row: None,
            dialog: None,
            dialog_error: None,
            import: None,
            import_error: None,
            edit_menu: false,
            update: None,
            catalog,
            selected: None,
            schema: Vec::new(),
            tabs: vec![QueryTab::new(1, "Requête 1".into(), "SELECT 1 AS hello")],
            active: 0,
            next_tab_id: 2,
            history: Vec::new(),
            tab: Tab::Results,
            status,
        }
    }

    pub fn title(&self) -> String {
        match self.active_config() {
            Some(c) => format!("DBphoque — {}", c.name),
            None => "DBphoque".into(),
        }
    }

    pub fn theme(&self) -> Theme {
        theme::dbphoque()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Seuls les événements clavier ignorés par les widgets arrivent ici :
        // l'éditeur gère lui-même ses raccourcis via `key_binding`.
        iced::event::listen_with(|event, status, _| match (event, status) {
            (
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }),
                iced::event::Status::Ignored,
            ) => Some(Message::KeyPressed(key, modifiers)),
            _ => None,
        })
    }

    fn active_config(&self) -> Option<&ConnectionConfig> {
        let id = self.session.as_ref()?.id;
        self.connections.iter().find(|c| c.id == id)
    }

    fn connector(&self) -> Option<Arc<dyn Connector>> {
        self.session.as_ref().map(|s| s.connector.clone())
    }

    /// Dialecte de la connexion ouverte, ou SQL standard à défaut.
    fn dialect(&self) -> Dialect {
        self.session.as_ref().map_or_else(editing::default_dialect, |s| s.connector.dialect().clone())
    }

    fn tab(&self) -> &QueryTab {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }

    fn tab_mut(&mut self) -> &mut QueryTab {
        let i = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[i]
    }

    /// SQL à exécuter : la sélection si elle existe, sinon tout l'éditeur.
    fn current_sql(&self) -> String {
        let editor = &self.tab().editor;
        editor.selection().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| editor.text())
    }

    /// Propositions pour le mot sous le curseur de l'onglet actif.
    fn compute_completion(&self) -> Option<Completion> {
        let editor = &self.tab().editor;
        let cursor = editor.cursor();
        let (line, column) = (cursor.position.line, cursor.position.column);
        let line_text = editor.line(line)?.text.to_string();
        let before: String = line_text.chars().take(column).collect();
        let tables: Vec<TableRef> = self.session.as_ref().map(|s| s.tables.clone()).unwrap_or_default();
        completion::suggest(&before, &editor.text(), line, column, &tables, &self.columns_cache)
    }

    /// Recharge la liste des tables et des schémas de la connexion ouverte.
    fn reload_structure(&mut self) -> Task<Message> {
        let Some(c) = self.connector() else { return Task::none() };
        let c2 = c.clone();
        Task::batch([
            Task::perform(async move { c.list_tables().await }, Message::TablesLoaded),
            Task::perform(async move { c2.list_schemas().await }, Message::SchemasLoaded),
        ])
    }

    /// Ouvre un onglet et l'active.
    fn open_tab(&mut self, name: String, sql: &str) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(QueryTab::new(id, name, sql));
        self.active = self.tabs.len() - 1;
    }

    /// Enregistre les onglets de la connexion ouverte.
    fn persist_tabs(&mut self) {
        let Some(session) = &self.session else { return };
        let queries: Vec<SavedQuery> = self.tabs.iter().map(QueryTab::saved).collect();
        if let Err(e) = store::save_queries(session.id, &queries) {
            self.status = Status::Error(e);
        }
    }

    /// Remplace les onglets par ceux enregistrés pour une connexion.
    fn load_tabs(&mut self, connection: u64) {
        let saved = store::load_queries(connection);
        self.tabs.clear();
        if saved.is_empty() {
            self.open_tab("Requête 1".into(), "");
        } else {
            for q in saved {
                self.open_tab(q.name, &q.sql);
            }
        }
        self.active = 0;
        self.history = store::load_history(connection);
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // Erreurs communes à plusieurs sections : traitées avant la répartition.
            Message::TablesLoaded(Err(e))
            | Message::SchemasLoaded(Err(e))
            | Message::SchemaLoaded(Err(e))
            | Message::Explained(_, _, Err(e))
            | Message::QueryDone(_, _, Err(e), _) => {
                self.status = Status::Error(e);
                Task::none()
            }
            m @ (Message::Connect(..)
            | Message::Connected(..)
            | Message::Disconnect
            | Message::SchemasLoaded(..)
            | Message::TablesLoaded(..)
            | Message::ColumnsCached(..)
            | Message::CompletionMove(..)
            | Message::CompletionClose
            | Message::CompletionAccept
            | Message::TableSelected(..)
            | Message::InsertTable(..)
            | Message::SchemaLoaded(..)
            | Message::FilterChanged(..)
            | Message::ToggleSchema(..)
            | Message::ToggleFold(..)
            | Message::ToggleFolder(..)
            | Message::PaneResized(..)
            | Message::WorkPaneResized(..)
            | Message::ToggleSidebar
            | Message::Refresh
            | Message::ToggleResults) => self.update_sidebar(m),
            m @ (Message::LoadData
            | Message::DataLoaded(..)
            | Message::DataRowSelected(..)
            | Message::SchemaRowSelected(..)
            | Message::ToggleEditMenu
            | Message::Noop
            | Message::OpenDialog(..)
            | Message::Dialog(..)
            | Message::Executed(..)) => self.update_edition(m),
            m @ (Message::LoadDiagram
            | Message::DiagramLoaded(..)
            | Message::DiagramMode(..)
            | Message::DiagramLayout
            | Message::DiagramFit
            | Message::DiagramInteraction(..)
            | Message::Design(..)) => self.update_diagram_view(m),
            m @ (Message::OpenManager
            | Message::CloseManager
            | Message::EditConnection(..)
            | Message::FormChanged(..)
            | Message::FormExtra(..)
            | Message::FormEngine(..)
            | Message::TogglePassword
            | Message::OpenPlugins
            | Message::PluginsRefresh
            | Message::PluginsLoaded(..)
            | Message::PluginInstall(..)
            | Message::PluginInstalled(..)
            | Message::PluginRemove(..)
            | Message::UpdateChecked(..)
            | Message::OpenUrl(..)
            | Message::TestConnection
            | Message::TestDone(..)
            | Message::SaveConnection { .. }
            | Message::DeleteConnection) => self.update_connections(m),
            m @ (Message::Edit(..)
            | Message::Explain
            | Message::Explained(..)
            | Message::Run
            | Message::QueryDone(..)
            | Message::QueryTabNew
            | Message::QueryTabSelect(..)
            | Message::QueryTabClose(..)
            | Message::QueryTabRename(..)
            | Message::HistoryOpen(..)
            | Message::TabSelected(..)
            | Message::PageChanged(..)
            | Message::ResultSort(..)
            | Message::ResultFilter(..)
            | Message::DataSort(..)
            | Message::DataFilter(..)
            | Message::CellEditStart(..)
            | Message::CellEditChanged(..)
            | Message::CellEditCancel
            | Message::Export(..)
            | Message::Exported(..)
            | Message::ImportPick
            | Message::ImportRead(..)
            | Message::Import(..)
            | Message::CellEditSubmit
            | Message::CopyCell(..)
            | Message::KeyPressed(..)) => self.update_workspace(m),
        }
    }

    /// Raccourcis globaux (hors éditeur, qui a les siens).
    fn handle_key(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        match key.as_ref() {
            Key::Named(Named::Escape) if self.import.is_some() => self.update(Message::Import(ImportMessage::Cancel)),
            Key::Named(Named::Escape) if self.editing_cell.is_some() => self.update(Message::CellEditCancel),
            Key::Named(Named::Escape) if self.design.pending_link.is_some() => {
                self.update(Message::Design(DesignMessage::LinkCancel))
            }
            Key::Named(Named::Escape) if self.dialog.is_some() => {
                self.update(Message::Dialog(DialogMessage::Cancel))
            }
            Key::Named(Named::Escape) if self.manager.is_some() => self.update(Message::CloseManager),
            Key::Named(Named::Enter) if modifiers.command() => self.update(Message::Run),
            Key::Character("e") if modifiers.command() => self.update(Message::Explain),
            Key::Character("r") if modifiers.command() => self.update(Message::Refresh),
            _ => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = if self.sidebar_hidden {
            // Barre repliée : une languette verticale pour la rouvrir.
            let strip = button(text("▶\nC\nO\nN\nN\nE\nX\nI\nO\nN\nS").size(11).color(theme::MUTED))
                .on_press(Message::ToggleSidebar)
                .style(theme::list_item(false))
                .padding([8, 4])
                .height(Fill);
            row![container(strip).style(theme::panel), self.workspace()].spacing(10).height(Fill).into()
        } else {
            pane_grid(&self.panes, |_, pane, _| {
                pane_grid::Content::new(match pane {
                    Pane::Sidebar => self.sidebar(),
                    Pane::Workspace => self.workspace(),
                })
            })
            .on_resize(8, Message::PaneResized)
            .style(theme::pane_grid)
            .spacing(10)
            .height(Fill)
            .into()
        };

        let base = column![self.top_bar(), body, self.status_bar()].spacing(10).padding(12);
        let base: Element<'_, Message> = if self.edit_menu && self.session.is_some() {
            stack![base, self.edit_menu()].into()
        } else {
            base.into()
        };

        if let Some(m) = &self.manager {
            return stack![base, manager::view(m, &self.connections)].into();
        }
        if let Some(form) = &self.import {
            return stack![base, import::view(form, self.import_error.as_deref())].into();
        }
        match &self.dialog {
            Some(d) => stack![base, editing::view(d, self.dialog_error.as_deref(), &self.dialect_cache)].into(),
            None => base.into(),
        }
    }

    fn top_bar(&self) -> Element<'_, Message> {
        let active: Element<'_, Message> = match self.active_config() {
            Some(c) => row![
                text("●").size(13).color(theme::SUCCESS),
                text(&c.name).size(15).color(theme::TEXT),
                text(c.summary()).size(12).font(MONO).color(theme::MUTED),
            ]
            .spacing(10)
            .align_y(iced::Center)
            .into(),
            None => text("Aucune connexion ouverte").size(14).color(theme::MUTED).into(),
        };

        let disconnect = button(text("Déconnecter").size(13))
            .on_press_maybe(self.session.as_ref().map(|_| Message::Disconnect))
            .style(theme::outline(theme::DANGER))
            .padding([5, 12]);

        let edit = button(text(if self.edit_menu { "Édition ▴" } else { "Édition ▾" }).size(13))
            .on_press_maybe(self.session.as_ref().map(|_| Message::ToggleEditMenu))
            .style(theme::outline(theme::PRIMARY))
            .padding([5, 12]);

        let mut brand = row![].spacing(8).align_y(iced::Center);
        if let Some(logo) = crate::icon::logo() {
            brand = brand.push(iced::widget::image(logo).width(30).height(30));
        }
        let brand = brand.push(text("DBphoque").size(20).color(theme::PRIMARY));
        let mut bar = row![brand, container(active).width(Fill)];
        if let Some(u) = &self.update {
            bar = bar.push(
                button(text(format!("Mise à jour {} disponible", u.version)).size(13))
                    .on_press(Message::OpenUrl(u.url.clone()))
                    .style(theme::outline(theme::SUCCESS))
                    .padding([5, 12]),
            );
        }
        let bar = bar.push(edit).push(disconnect);
        container(bar.spacing(16).align_y(iced::Center))
        .style(theme::top_bar)
        .padding([8, 14])
        .width(Fill)
        .into()
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let (label, color) = match &self.status {
            Status::Idle => ("Prêt · ⌘↩ exécuter · ⌘E expliquer · ⌘R actualiser".to_string(), theme::MUTED),
            Status::Busy(msg) => (msg.clone(), theme::WARNING),
            Status::Info(msg) => (msg.clone(), theme::SUCCESS),
            Status::Error(msg) => (msg.clone(), theme::DANGER),
        };
        container(text(label).size(12).font(MONO).color(color))
            .style(theme::status_bar)
            .padding([6, 10])
            .width(Fill)
            .into()
    }
}

/// « Requête : SELECT … » au-dessus d'un résultat, pour savoir d'où il vient.
fn source_line(sql: Option<&str>) -> Element<'_, Message> {
    let sql = sql.unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    let preview: String = sql.chars().take(120).collect();
    let suffix = if sql.chars().count() > 120 { "…" } else { "" };
    text(format!("Requête : {preview}{suffix}"))
        .size(12)
        .font(MONO)
        .color(theme::MUTED)
        .wrapping(text::Wrapping::None)
        .into()
}

/// Instruction qui change la structure de la base : après son exécution,
/// la liste des tables, des schémas et le diagramme doivent être relus.
fn is_ddl(sql: &str) -> bool {
    let head = sql.trim_start().to_uppercase();
    ["CREATE", "DROP", "ALTER", "RENAME", "TRUNCATE"].iter().any(|k| head.starts_with(k))
}

/// Contenu d'onglet que l'on peut remplacer sans perte par l'aperçu d'une
/// table : vide, ou un aperçu généré (`SELECT * FROM … LIMIT 100` sur une
/// ligne). Tout le reste est une requête de l'utilisateur.
fn is_preview_sql(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty() || (trimmed.starts_with("SELECT * FROM") && trimmed.lines().count() == 1)
}

/// Bouton d'action d'un onglet : grisé quand l'action ne s'applique pas.
fn action_button<'a>(label: &'static str, dialog: Option<Dialog>, danger: bool) -> Element<'a, Message> {
    let b = button(text(label).size(12)).on_press_maybe(dialog.map(Message::OpenDialog)).padding([3, 10]);
    if danger { b.style(theme::outline(theme::DANGER)).into() } else { b.style(button::secondary).into() }
}

fn hint(message: &str) -> Element<'_, Message> {
    container(text(message).color(theme::MUTED)).center(Fill).into()
}

fn section(title: &str) -> Element<'_, Message> {
    text(title).size(11).color(theme::MUTED).into()
}

/// « Exporter : CSV · JSON · SQL · Excel ».
fn export_buttons<'a>(source: ExportSource) -> Element<'a, Message> {
    let mut r = row![text("Exporter :").size(11).color(theme::MUTED)].spacing(4).align_y(iced::Center);
    for format in Format::ALL {
        r = r.push(
            button(text(format.label()).size(11))
                .on_press(Message::Export(source, format))
                .style(theme::list_item(false))
                .padding([2, 6]),
        );
    }
    r.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_is_detected_regardless_of_case_and_spacing() {
        assert!(is_ddl("CREATE SCHEMA test"));
        assert!(is_ddl("  create table t (id int)"));
        assert!(is_ddl("ALTER TABLE t ADD COLUMN c TEXT"));
        assert!(is_ddl("drop table t"));
        assert!(!is_ddl("SELECT * FROM t"));
        assert!(!is_ddl("INSERT INTO t VALUES (1)"));
        assert!(!is_ddl("-- CREATE dans un commentaire\nSELECT 1"));
    }

    #[test]
    fn sort_toggles_asc_desc_none() {
        assert_eq!(toggle_sort(None, 2), Some((2, true)));
        assert_eq!(toggle_sort(Some((2, true)), 2), Some((2, false)));
        assert_eq!(toggle_sort(Some((2, false)), 2), None);
        assert_eq!(toggle_sort(Some((2, false)), 0), Some((0, true)));
    }

    #[test]
    fn rows_are_filtered_then_sorted_numerically_with_nulls_last() {
        use connector::Cell;
        let rows = vec![
            vec![Cell::Text("b".into()), Cell::Int(10)],
            vec![Cell::Text("a".into()), Cell::Null],
            vec![Cell::Text("c".into()), Cell::Int(9)],
            vec![Cell::Text("ab".into()), Cell::Float(9.5)],
        ];
        assert_eq!(order_rows(&rows, None, ""), vec![0, 1, 2, 3]);
        assert_eq!(order_rows(&rows, Some((1, true)), ""), vec![2, 3, 0, 1]);
        assert_eq!(order_rows(&rows, Some((1, false)), ""), vec![0, 3, 2, 1]);
        assert_eq!(order_rows(&rows, Some((0, true)), "A"), vec![1, 3]);
    }

    #[test]
    fn preview_replaces_only_empty_or_generated_tabs() {
        assert!(is_preview_sql(""));
        assert!(is_preview_sql("   \n"));
        assert!(is_preview_sql("SELECT * FROM \"public\".\"users\" LIMIT 100"));
        assert!(!is_preview_sql("SELECT * FROM users\nWHERE id = 1"));
        assert!(!is_preview_sql("SELECT name FROM users"));
        assert!(!is_preview_sql("CREATE SCHEMA test"));
    }
}
