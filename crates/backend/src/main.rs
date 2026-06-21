mod ai;
mod analysis;
mod canonical;
mod delta_policy;
mod models;
mod pdf;
mod routes;
mod store;

use std::{net::SocketAddr, sync::Arc};

use ai::provider_from_env;
use axum::Router;
use routes::api_router;
use store::AppState;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "delta_reader_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState::new(provider_from_env()));
    let app = Router::new()
        .nest("/api", api_router(state))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    tracing::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
