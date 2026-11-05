// Sous Windows, application graphique : pas de console ouverte à côté.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod completion;
mod connector;
mod design;
mod diagram;
mod editing;
mod export;
mod icon;
mod import;
mod manager;
mod plugins;
mod store;
mod theme;
mod tunnel;

use app::App;

fn main() -> iced::Result {
    // Au démarrage : l'état, puis une vérification de mise à jour en arrière-plan.
    // Reprend connexions, requêtes et connecteurs de Quarry (ancien nom).
    store::migrate_from_quarry();
    let boot = || {
        let app = App::new();
        let tasks = iced::Task::batch([
            iced::Task::perform(plugins::check_app_update(), app::Message::UpdateChecked),
            iced::Task::perform(plugins::fetch_registry(), app::Message::PluginsLoaded),
        ]);
        (app, tasks)
    };
    iced::application(boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1280.0, 800.0),
            icon: icon::window_icon(),
            ..Default::default()
        })
        .run()
}
