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

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::handlers::lsp::{
        DocumentColorsEvent, DocumentLinksEvent, PullDiagnosticsEvent,
    };

    #[test]
    fn mixed_edit_storm_coalesces_each_feature_independently() {
        // A heavily edited document can generate thousands of both events before
        // the debounce fires. Both features must retain their own pending work.
        let document = DocumentId::default();
        let mut colors = DebouncedDocuments::<DocumentColorsEvent>::new();
        let mut links = DebouncedDocuments::<DocumentLinksEvent>::new();
        let mut diagnostics = DebouncedDocuments::<PullDiagnosticsEvent>::new();
        let mut color_deadline = None;
        let mut link_deadline = None;
        let mut diagnostic_deadline = None;

        for change in 0..50_000 {
            color_deadline = colors.handle_event(DocumentColorsEvent(document), color_deadline);
            if change % 3 == 0 {
                link_deadline = links.handle_event(DocumentLinksEvent(document), link_deadline);
            }
            if change % 2 == 0 {
                diagnostic_deadline = diagnostics.handle_event(
                    PullDiagnosticsEvent {
                        document_id: document,
                    },
                    diagnostic_deadline,
                );
            }
        }

        assert_eq!(colors.docs.len(), 1, "color requests should coalesce");
        assert_eq!(links.docs.len(), 1, "link requests should coalesce");
        assert_eq!(
            diagnostics.docs.len(),
            1,
            "diagnostic requests should coalesce"
        );
        assert!(colors.docs.contains(&document));
        assert!(links.docs.contains(&document));
        assert!(diagnostics.docs.contains(&document));
        assert!(color_deadline.is_some());
        assert!(link_deadline.is_some());
        assert!(diagnostic_deadline.is_some());
    }

    #[test]
    fn repeated_changes_reschedule_the_document_deadline() {
        let document = DocumentId::default();
        let mut handler = DebouncedDocuments::<DocumentColorsEvent>::new();

        let before = Instant::now();
        let first = handler
            .handle_event(DocumentColorsEvent(document), None)
            .expect("document changes must schedule a refresh");
        let second = handler
            .handle_event(DocumentColorsEvent(document), Some(first))
            .expect("another change must keep the refresh scheduled");

        assert!(first >= before + DOCUMENT_CHANGE_DEBOUNCE);
        assert!(second >= first, "new changes must not shorten the deadline");
        assert_eq!(handler.docs.len(), 1);
    }

    #[test]
    fn new_edits_after_a_flushed_batch_are_not_lost() {
        let document = DocumentId::default();
        let mut handler = DebouncedDocuments::<DocumentColorsEvent>::new();

        for _ in 0..10_000 {
            handler.handle_event(DocumentColorsEvent(document), None);
        }

        // finish_debounce drains a batch by taking this set. Incoming edits
        // must begin a fresh batch without modifying the dispatched one.
        let dispatched = std::mem::take(&mut handler.docs);
        assert_eq!(dispatched.len(), 1);
        assert!(handler.docs.is_empty());

        let next_deadline = handler.handle_event(DocumentColorsEvent(document), None);
        assert!(next_deadline.is_some());
        assert_eq!(handler.docs.len(), 1);
        assert_eq!(dispatched.len(), 1);
    }

    #[test]
    fn different_document_request_types_do_not_share_pending_state() {
        let document = DocumentId::default();
        let mut colors = DebouncedDocuments::<DocumentColorsEvent>::new();
        let mut links = DebouncedDocuments::<DocumentLinksEvent>::new();

        colors.handle_event(DocumentColorsEvent(document), None);
        assert_eq!(colors.docs.len(), 1);
        assert!(links.docs.is_empty());

        links.handle_event(DocumentLinksEvent(document), None);
        assert_eq!(links.docs.len(), 1);
        assert_eq!(colors.docs.len(), 1);
    }
}
