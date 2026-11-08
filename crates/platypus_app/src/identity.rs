//! The game's own Windows identity (AppUserModelID, from `project.json` `identifier`): without it
//! every game groups under `platypus-app.exe` in the taskbar and a pinned game shows Platypus's
//! icon. A packaged game also registers its name and icon and keeps a Start-menu shortcut carrying
//! the id, which is where Windows' media flyout and notifications take an app's name.

use std::path::{Path, PathBuf};

use crate::info::Manifest;

/// The largest icon written for the registration (Windows shows at most 256 px).
const MAX_ICON: u32 = 256;

/// Sets the process's app id before the window exists; a packaged game also registers its name
/// and icon and makes sure its Start-menu shortcut is there. Nothing happens without an identifier.
pub fn apply(manifest: &Manifest, icon: Option<&(Vec<u8>, u32, u32)>, packaged: bool) {
    let id = manifest.identifier.trim();
    if id.is_empty() {
        return;
    }
    if !packaged {
        // A dev run only gets its own taskbar group: no registry entry or shortcut for every
        // game folder that is run.
        if !desktop_window::set_app_id(id, "", None) {
            tracing::warn!("could not set the app id {id}");
        }
        return;
    }
    let name = manifest.window_title();
    let png = icon.and_then(|icon| {
        let dir = data_dir(&manifest.name)?;
        match write_icon(&dir.join("icon.png"), icon) {
            Ok(path) => Some(path),
            Err(e) => {
                tracing::warn!("could not write the app icon: {e}");
                None
            }
        }
    });
    if !desktop_window::set_app_id(id, name, png.as_deref()) {
        tracing::warn!("could not register the app id {id}");
    }
    match std::env::current_exe() {
        Ok(exe) => {
            if !desktop_window::ensure_start_menu_shortcut(id, name, &exe) {
                tracing::warn!("could not write the Start-menu shortcut for {name}");
            }
        }
        Err(e) => tracing::warn!("no exe path for the Start-menu shortcut: {e}"),
    }
}

/// `%LOCALAPPDATA%\Platypus\<name>` (Windows only: the registration is Windows only).
fn data_dir(name: &str) -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("Platypus").join(name))
}

/// Writes the RGBA icon as a PNG of at most `MAX_ICON` px, only when the bytes differ from the
/// file's (the shell caches the icon by path).
fn write_icon(path: &Path, icon: &(Vec<u8>, u32, u32)) -> anyhow::Result<PathBuf> {
    let png = icon_png(icon)?;
    if std::fs::read(path).ok().as_deref() != Some(png.as_slice()) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, &png)?;
    }
    Ok(path.to_path_buf())
}

/// The icon encoded as PNG, scaled down to fit `MAX_ICON` px.
fn icon_png((rgba, w, h): &(Vec<u8>, u32, u32)) -> anyhow::Result<Vec<u8>> {
    let mut image = image::DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(*w, *h, rgba.clone())
            .ok_or_else(|| anyhow::anyhow!("icon pixels don't match its {w}x{h} size"))?,
    );
    if *w > MAX_ICON || *h > MAX_ICON {
        // Keeps the aspect ratio.
        image = image.thumbnail(MAX_ICON, MAX_ICON);
    }
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32) -> (Vec<u8>, u32, u32) {
        ([10u8, 20, 30, 255].repeat((w * h) as usize), w, h)
    }

    // M9-ID-01
    #[test]
    fn no_identifier_sets_nothing() {
        let dir = std::env::temp_dir().join(format!("platypus_identity_{}", std::process::id()));
        // An absolute name makes the icon folder this temp one (`join` replaces the base).
        let manifest = Manifest {
            name: dir.display().to_string(),
            ..Manifest::default()
        };
        apply(&manifest, Some(&solid(4, 4)), true);
        assert!(!dir.exists());
    }

    // M9-ID-02
    #[test]
    fn icon_png_keeps_small_icons_and_shrinks_big_ones() {
        let small = image::load_from_memory(&icon_png(&solid(32, 16)).unwrap()).unwrap();
        assert_eq!((small.width(), small.height()), (32, 16));
        let big = image::load_from_memory(&icon_png(&solid(512, 512)).unwrap()).unwrap();
        assert_eq!((big.width(), big.height()), (256, 256));
        let wide = image::load_from_memory(&icon_png(&solid(1024, 512)).unwrap()).unwrap();
        assert_eq!((wide.width(), wide.height()), (256, 128));
        assert!(icon_png(&(vec![0; 3], 2, 2)).is_err());
    }

    // M9-ID-02
    #[test]
    fn write_icon_rewrites_only_changed_icons() {
        let dir =
            std::env::temp_dir().join(format!("platypus_identity_png_{}", std::process::id()));
        let path = dir.join("icon.png");
        write_icon(&path, &solid(8, 8)).unwrap();
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();
        write_icon(&path, &solid(8, 8)).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
        write_icon(&path, &solid(16, 16)).unwrap();
        let png = image::open(&path).unwrap();
        assert_eq!(png.width(), 16);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
