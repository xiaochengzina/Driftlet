use std::collections::HashMap;
use std::sync::RwLock;
use tauri::WebviewWindow;

/// Thread-safe registry of loaded skin windows
pub struct SkinWindowRegistry {
    windows: RwLock<HashMap<String, WebviewWindow>>,
}

impl SkinWindowRegistry {
    pub fn new() -> Self {
        Self {
            windows: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, skin_id: String, window: WebviewWindow) {
        self.windows
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(skin_id, window);
    }

    pub fn unregister(&self, skin_id: &str) -> Option<WebviewWindow> {
        self.windows
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(skin_id)
    }

    pub fn get(&self, skin_id: &str) -> Option<WebviewWindow> {
        self.windows
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(skin_id)
            .cloned()
    }

    pub fn is_loaded(&self, skin_id: &str) -> bool {
        self.windows
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(skin_id)
    }

    pub fn loaded_ids(&self) -> Vec<String> {
        self.windows
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Return (skin_id, HWND) for all loaded windows.
    /// Used by the periodic cleanup timer to reassert frameless state.
    pub fn all_hwnds(&self) -> Vec<(String, isize)> {
        self.windows
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|(id, w)| w.hwnd().ok().map(|h| (id.clone(), h.0 as isize)))
            .collect()
    }
}
