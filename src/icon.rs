//! Icône de l'application : source `assets/icon.svg`, rendus par `scripts/make_icon.sh`.

use iced::window;

const ICON_256: &[u8] = include_bytes!("../assets/icon-256.png");

/// Pixels RGBA de l'icône 256 px, avec sa largeur et sa hauteur.
fn decode_256() -> Option<(Vec<u8>, u32, u32)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(ICON_256));
    let mut reader = decoder.read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(info.buffer_size());
    Some((rgba, info.width, info.height))
}

/// Icône de fenêtre (barre des tâches sous Windows et Linux).
pub fn window_icon() -> Option<window::Icon> {
    let (rgba, width, height) = decode_256()?;
    window::icon::from_rgba(rgba, width, height).ok()
}

/// Logo affiché dans l'application (barre du haut). La poignée est créée une
/// seule fois : iced identifie l'image par sa poignée, en recréer une à
/// chaque `view` renverrait la texture au GPU à chaque image.
pub fn logo() -> Option<iced::widget::image::Handle> {
    static LOGO: std::sync::OnceLock<Option<iced::widget::image::Handle>> = std::sync::OnceLock::new();
    LOGO.get_or_init(|| {
        let (rgba, width, height) = decode_256()?;
        Some(iced::widget::image::Handle::from_rgba(width, height, rgba))
    })
    .clone()
}

/// Icône du Dock sur macOS. Hors d'un bundle `.app`, macOS affiche sinon
/// l'icône générique d'exécutable ; on la remplace au lancement.
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON_1024: &[u8] = include_bytes!("../assets/icon.png");

    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(ICON_1024);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else { return };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY : appelé sur le thread principal avec une image valide.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon() {}
