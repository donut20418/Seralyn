#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use seralyn_lib::app::conversation::{
    Conversation, ConversationManager, ConversationSummary, ConversationWithMessages,
};
use seralyn_lib::app::db::Database;
use seralyn_lib::app::events::ProviderKind;
use seralyn_lib::app::providers::{ProviderManager, ProviderStatus};
use std::str::FromStr;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub conversation_manager: Arc<ConversationManager>,
}

#[tauri::command]
async fn create_conversation(
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<Conversation, String> {
    state
        .conversation_manager
        .create_conversation(title.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_conversations(
    state: State<'_, AppState>,
) -> Result<Vec<ConversationSummary>, String> {
    state
        .conversation_manager
        .list_conversations()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_conversation(
    id: String,
    state: State<'_, AppState>,
) -> Result<ConversationWithMessages, String> {
    state
        .conversation_manager
        .get_conversation(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_conversation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .conversation_manager
        .delete_conversation(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_message(
    app: AppHandle,
    conversation_id: String,
    content: String,
    provider: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let provider_kind = ProviderKind::from_str(&provider).map_err(|e| e.to_string())?;

    let (tx, mut rx) = mpsc::channel(100);
    let app_clone = app.clone();

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Err(e) = app_clone.emit("conversation-event", &event) {
                error!("Failed to emit event: {}", e);
            }
        }
    });

    state
        .conversation_manager
        .send_message(&conversation_id, &content, provider_kind, tx)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn switch_provider(
    conversation_id: String,
    provider: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let provider_kind = ProviderKind::from_str(&provider).map_err(|e| e.to_string())?;
    state
        .conversation_manager
        .switch_provider(&conversation_id, provider_kind)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn detect_providers(state: State<'_, AppState>) -> Result<Vec<ProviderStatus>, String> {
    let providers = state
        .conversation_manager
        .detect_providers()
        .await;
    Ok(providers)
}

#[tauri::command]
async fn respond_to_approval(
    conversation_id: String,
    provider: String,
    approval_id: String,
    approved: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let provider_kind = ProviderKind::from_str(&provider).map_err(|e| e.to_string())?;
    state
        .conversation_manager
        .respond_to_approval(&conversation_id, provider_kind, &approval_id, approved)
        .await
        .map_err(|e| e.to_string())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting Seralyn...");

    let db_path = Database::default_path();
    let db = match Database::new(&db_path) {
        Ok(db) => {
            if let Err(e) = db.run_migrations() {
                error!("Failed to run database migrations: {}", e);
                std::process::exit(1);
            }
            Arc::new(db)
        }
        Err(e) => {
            error!("Failed to initialize database: {}", e);
            std::process::exit(1);
        }
    };

    let provider_manager = Arc::new(ProviderManager::new());
    let conversation_manager = Arc::new(ConversationManager::new(db, provider_manager));

    tauri::Builder::default()
        .manage(AppState {
            conversation_manager: conversation_manager.clone(),
        })
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            create_conversation,
            list_conversations,
            get_conversation,
            delete_conversation,
            send_message,
            switch_provider,
            detect_providers,
            respond_to_approval
        ])
        .on_window_event(move |_window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // We could call close_all_sessions here, but since it's async, we might need to block or spawn.
                // Assuming conversation_manager has a sync or spawnable method:
                // For now, spawn a task if it's async, or call directly if sync.
                let cm = conversation_manager.clone();
                tokio::spawn(async move {
                    if let Err(e) = cm.close_all_sessions().await {
                        error!("Failed to close sessions on exit: {}", e);
                    }
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
