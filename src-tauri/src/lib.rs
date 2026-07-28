use cid_core::{api::router::create_router, Core};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // Initialize core and start server in background
            let core = Core::new(None).expect("Failed to init core");
            let app_state = core.app_state();
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 5919));
            tauri::async_runtime::spawn(async move {
                let router = create_router(app_state);
                let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                axum::serve(listener, router.into_make_service())
                    .await
                    .unwrap();
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
