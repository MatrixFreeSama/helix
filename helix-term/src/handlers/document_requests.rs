//! Shared debounce and refresh lifecycle for document-scoped LSP requests.

use std::{collections::HashSet, marker::PhantomData, time::Duration};

use helix_event::AsyncHook;
use helix_view::{DocumentId, Editor};
use tokio::time::Instant;

use crate::job;

const DOCUMENT_CHANGE_DEBOUNCE: Duration = Duration::from_millis(250);

pub(super) trait DocumentRequest: Send + Sync + 'static {
    fn document_id(self) -> DocumentId;
    fn request(editor: &mut Editor, doc_id: DocumentId);
}

pub(super) struct DebouncedDocuments<E: DocumentRequest> {
    docs: HashSet<DocumentId>,
    event: PhantomData<E>,
}

impl<E: DocumentRequest> DebouncedDocuments<E> {
    pub(super) fn new() -> Self {
        Self {
            docs: HashSet::new(),
            event: PhantomData,
        }
    }
}

impl<E: DocumentRequest> AsyncHook for DebouncedDocuments<E> {
    type Event = E;

    fn handle_event(&mut self, event: E, _timeout: Option<Instant>) -> Option<Instant> {
        self.docs.insert(event.document_id());
        Some(Instant::now() + DOCUMENT_CHANGE_DEBOUNCE)
    }

    fn finish_debounce(&mut self) {
        let docs = std::mem::take(&mut self.docs);

        job::dispatch_blocking(move |editor, _compositor| {
            for doc_id in docs {
                E::request(editor, doc_id);
            }
        });
    }
}

pub(super) fn request_all<E: DocumentRequest>(editor: &mut Editor) {
    let doc_ids: Vec<_> = editor.documents().map(|doc| doc.id()).collect();

    for doc_id in doc_ids {
        E::request(editor, doc_id);
    }
}
