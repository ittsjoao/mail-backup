pub mod error;
pub mod index;
pub mod store;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o aplicativo");
}
