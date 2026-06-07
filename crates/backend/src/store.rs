use std::{collections::HashMap, sync::Arc};

use chrono::{Duration, Utc};
use tokio::sync::RwLock;

use crate::{ai::DynChatProvider, models::*};

pub struct AppState {
    sessions: RwLock<HashMap<DocumentId, DocumentSession>>,
    ai: DynChatProvider,
    session_ttl: Duration,
}

impl AppState {
    pub fn new(ai: DynChatProvider) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            ai,
            session_ttl: Duration::hours(6),
        }
    }

    pub fn ai(&self) -> DynChatProvider {
        Arc::clone(&self.ai)
    }

    pub async fn insert(&self, session: DocumentSession) {
        self.purge_expired().await;
        self.sessions.write().await.insert(session.id, session);
    }

    pub async fn get(&self, id: DocumentId) -> Option<DocumentSession> {
        self.purge_expired().await;
        self.sessions.read().await.get(&id).cloned()
    }

    pub async fn update<F, T>(&self, id: DocumentId, f: F) -> Option<T>
    where
        F: FnOnce(&mut DocumentSession) -> T,
    {
        self.purge_expired().await;
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(&id)?;
        Some(f(session))
    }

    async fn purge_expired(&self) {
        let cutoff = Utc::now() - self.session_ttl;
        self.sessions
            .write()
            .await
            .retain(|_, session| session.uploaded_at >= cutoff);
    }
}
