use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;

use crate::{analysis, models::*, pdf::extract_text_chunks, store::AppState};

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/documents", post(upload_document))
        .route("/documents/:id/pdf", get(get_pdf))
        .route("/documents/:id/concepts", get(get_concepts))
        .route("/documents/:id/baseline", post(set_baseline))
        .route("/documents/:id/analyze", post(analyze))
        .route("/documents/:id/reader", get(get_reader))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state)
}

async fn upload_document(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiError> {
    let mut filename = "document.pdf".to_owned();
    let mut pdf_bytes: Option<Bytes> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(ApiError::bad_request)?
    {
        if field.name() == Some("file") {
            if let Some(field_filename) = field.file_name() {
                filename = field_filename.to_owned();
            }
            pdf_bytes = Some(field.bytes().await.map_err(ApiError::bad_request)?);
            break;
        }
    }

    let pdf_bytes = pdf_bytes.ok_or_else(|| ApiError::bad_request("Missing PDF file field"))?;
    let (page_count, chunks) = extract_text_chunks(&pdf_bytes).map_err(ApiError::bad_request)?;
    let id = Uuid::new_v4();
    let concepts = analysis::generate_concepts(state.ai().as_ref(), &chunks)
        .await
        .map_err(ApiError::internal)?;

    state
        .insert(DocumentSession {
            id,
            filename,
            uploaded_at: Utc::now(),
            page_count,
            pdf_bytes: pdf_bytes.to_vec(),
            chunks,
            concepts,
            baseline: None,
            quests: Vec::new(),
            annotations: Vec::new(),
        })
        .await;

    Ok(Json(UploadResponse {
        document_id: id,
        page_count,
        extraction_status: "text_extracted".to_owned(),
    }))
}

async fn get_pdf(
    State(state): State<Arc<AppState>>,
    Path(id): Path<DocumentId>,
) -> Result<Response, ApiError> {
    let session = state.get(id).await.ok_or(ApiError::not_found())?;
    Ok((
        [(header::CONTENT_TYPE, "application/pdf")],
        session.pdf_bytes,
    )
        .into_response())
}

async fn get_concepts(
    State(state): State<Arc<AppState>>,
    Path(id): Path<DocumentId>,
) -> Result<Json<Vec<ConceptTag>>, ApiError> {
    let session = state.get(id).await.ok_or(ApiError::not_found())?;
    Ok(Json(session.concepts))
}

async fn set_baseline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<DocumentId>,
    Json(request): Json<BaselineRequest>,
) -> Result<Json<UserBaseline>, ApiError> {
    let baseline = UserBaseline {
        express_text: request.express_text,
        mastered_concept_ids: request.mastered_concept_ids,
    };

    state
        .update(id, |session| {
            session.baseline = Some(baseline.clone());
        })
        .await
        .ok_or(ApiError::not_found())?;

    Ok(Json(baseline))
}

async fn analyze(
    State(state): State<Arc<AppState>>,
    Path(id): Path<DocumentId>,
) -> Result<Json<AnalyzeResponse>, ApiError> {
    let session = state.get(id).await.ok_or(ApiError::not_found())?;
    let baseline = session
        .baseline
        .clone()
        .ok_or_else(|| ApiError::bad_request("Baseline must be set before analysis"))?;

    let (quests, annotations) = analysis::analyze_document(
        state.ai().as_ref(),
        &session.chunks,
        &session.concepts,
        &baseline,
    )
    .await
    .map_err(ApiError::internal)?;

    state
        .update(id, |session| {
            session.quests = quests.clone();
            session.annotations = annotations.clone();
        })
        .await
        .ok_or(ApiError::not_found())?;

    Ok(Json(AnalyzeResponse {
        quests,
        chunk_annotations: annotations,
    }))
}

async fn get_reader(
    State(state): State<Arc<AppState>>,
    Path(id): Path<DocumentId>,
) -> Result<Json<ReaderResponse>, ApiError> {
    let session = state.get(id).await.ok_or(ApiError::not_found())?;
    Ok(Json(ReaderResponse {
        document_id: session.id,
        filename: session.filename,
        page_count: session.page_count,
        chunks: session.chunks,
        concepts: session.concepts,
        baseline: session.baseline,
        quests: session.quests,
        chunk_annotations: session.annotations,
    }))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(error: impl ToString) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }

    fn internal(error: impl ToString) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "Document not found".to_owned(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}
