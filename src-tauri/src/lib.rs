pub mod archive;
pub mod commands;
pub mod config;
pub mod error;
pub mod imap_sync;
pub mod index;
pub mod message;
pub mod store;

use commands::AppState;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().data_dir()?.join("mail-backup");
            std::fs::create_dir_all(&data_dir)?;
            let config = config::Config::load(&data_dir.join("config.json"))?;
            app.manage(AppState { data_dir, config: Mutex::new(config), cancel: AtomicBool::new(false), running: AtomicBool::new(false) });
            let handle = app.handle().clone();
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("Mail Backup")
                .inner_size(1200.0, 800.0)
                .on_new_window(move |url, _| {
                    // links do e-mail (iframe com <base target="_blank">) abrem no navegador padrão
                    if matches!(url.scheme(), "http" | "https" | "mailto") {
                        let _ = handle.opener().open_url(url.as_str(), None::<&str>);
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::save_account,
            commands::delete_account,
            commands::test_connection,
            commands::pick_folder,
            commands::run_backup,
            commands::cancel_backup,
            commands::list_folders,
            commands::list_messages,
            commands::get_message,
            commands::open_eml,
            commands::save_attachment,
            commands::open_attachment,
            commands::reindex_account,
            commands::export_accounts,
            commands::pick_archive,
            commands::import_archive,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o aplicativo");
}
