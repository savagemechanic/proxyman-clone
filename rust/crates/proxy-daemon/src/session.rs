use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use proxy_core::{BodyPreview, CapturedTransaction, TransactionState};
use tokio::sync::RwLock;

const DEFAULT_CAPACITY: usize = 1_000;

#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<Inner>,
}

struct Inner {
    next_id: AtomicU64,
    capacity: usize,
    transactions: RwLock<VecDeque<CapturedTransaction>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl SessionStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                next_id: AtomicU64::new(1),
                capacity: capacity.max(1),
                transactions: RwLock::new(VecDeque::new()),
            }),
        }
    }

    pub fn next_id(&self) -> u64 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn insert(&self, transaction: CapturedTransaction) {
        let mut transactions = self.inner.transactions.write().await;
        transactions.push_front(transaction);
        while transactions.len() > self.inner.capacity {
            transactions.pop_back();
        }
    }

    pub async fn set_request_preview(&self, id: u64, preview: BodyPreview) {
        let mut transactions = self.inner.transactions.write().await;
        if let Some(transaction) = transactions
            .iter_mut()
            .find(|transaction| transaction.id == id)
        {
            transaction.request_body_bytes = preview.total_bytes;
            transaction.request_body_preview = Some(preview);
        }
    }

    pub async fn complete(
        &self,
        id: u64,
        status_code: u16,
        response_headers: Vec<proxy_core::HeaderField>,
        response_preview: BodyPreview,
    ) {
        let mut transactions = self.inner.transactions.write().await;
        if let Some(transaction) = transactions
            .iter_mut()
            .find(|transaction| transaction.id == id)
        {
            transaction.status_code = Some(status_code);
            transaction.response_headers = response_headers;
            transaction.response_body_bytes = response_preview.total_bytes;
            transaction.response_body_preview = Some(response_preview);
            transaction.state = TransactionState::Complete;
        }
    }

    pub async fn fail(&self, id: u64) {
        let mut transactions = self.inner.transactions.write().await;
        if let Some(transaction) = transactions
            .iter_mut()
            .find(|transaction| transaction.id == id)
        {
            transaction.state = TransactionState::Failed;
        }
    }

    pub async fn list(&self) -> Vec<CapturedTransaction> {
        self.inner
            .transactions
            .read()
            .await
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transaction(id: u64) -> CapturedTransaction {
        CapturedTransaction {
            id,
            started_at_unix_ms: id,
            scheme: "http".into(),
            host: "example.com".into(),
            method: "GET".into(),
            target: "/".into(),
            request_headers: Vec::new(),
            request_body_bytes: 0,
            request_body_preview: None,
            status_code: None,
            response_headers: Vec::new(),
            response_body_bytes: 0,
            response_body_preview: None,
            state: TransactionState::Pending,
        }
    }

    #[tokio::test]
    async fn keeps_newest_transactions_within_capacity() {
        let store = SessionStore::new(2);
        store.insert(transaction(1)).await;
        store.insert(transaction(2)).await;
        store.insert(transaction(3)).await;
        let captured = store.list().await;
        assert_eq!(
            captured.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![3, 2]
        );
    }
}
