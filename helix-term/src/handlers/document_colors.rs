use std::collections::HashSet;

use futures_util::{stream::FuturesUnordered, StreamExt};
use helix_core::{syntax::config::LanguageServerFeature, text_annotations::InlineAnnotation};
use helix_event::{cancelable_future, register_hook};
use helix_lsp::lsp;
use helix_view::{
    document::DocumentColorSwatches,
    events::{DocumentDidChange, DocumentDidOpen, LanguageServerExited, LanguageServerInitialized},
    handlers::{lsp::DocumentColorsEvent, Handlers},
    DocumentId, Editor, Theme,
};

use super::document_requests;
use crate::job;

impl document_requests::DocumentRequest for DocumentColorsEvent {
    fn document_id(self) -> DocumentId {
        self.0
    }

    fn request(editor: &mut Editor, doc_id: DocumentId) {
        request_document_colors(editor, doc_id);
    }
}

fn request_document_colors(editor: &mut Editor, doc_id: DocumentId) {
    if !editor.config().lsp.display_color_swatches {
        return;
    }

    let Some(doc) = editor.document_mut(doc_id) else {
        return;
    };

    let cancel = doc.color_swatch_controller.restart();

    let mut seen_language_servers = HashSet::new();
    let mut futures: FuturesUnordered<_> = doc
        .language_servers_with_feature(LanguageServerFeature::DocumentColors)
        .filter(|ls| seen_language_servers.insert(ls.id()))
        .map(|language_server| {
            let text = doc.text().clone();
            let offset_encoding = language_server.offset_encoding();
            let future = language_server
                .text_document_document_color(doc.identifier(), None)
                .unwrap();

            async move {
                let colors: Vec<_> = future
                    .await?
                    .into_iter()
                    .filter_map(|color_info| {
                        let pos = helix_lsp::util::lsp_pos_to_pos(
                            &text,
                            color_info.range.start,
                            offset_encoding,
                        )?;
                        Some((pos, color_info.color))
                    })
                    .collect();
                anyhow::Ok(colors)
            }
        })
        .collect();

    if futures.is_empty() {
        return;
    }

    tokio::spawn(async move {
        let mut all_colors = Vec::new();
        loop {
            match cancelable_future(futures.next(), &cancel).await {
                Some(Some(Ok(items))) => all_colors.extend(items),
                Some(Some(Err(err))) => log::error!("document color request failed: {err}"),
                Some(None) => break,
                // The request was cancelled.
                None => return,
            }
        }
        job::dispatch(move |editor, _| attach_document_colors(editor, doc_id, all_colors)).await;
    });
}

fn attach_document_colors(
    editor: &mut Editor,
    doc_id: DocumentId,
    mut doc_colors: Vec<(usize, lsp::Color)>,
) {
    if !editor.config().lsp.display_color_swatches {
        return;
    }

    let Some(doc) = editor.documents.get_mut(&doc_id) else {
        return;
    };

    if doc_colors.is_empty() {
        doc.color_swatches.take();
        return;
    }

    doc_colors.sort_by_key(|(pos, _)| *pos);

    let mut color_swatches = Vec::with_capacity(doc_colors.len());
    let mut color_swatches_padding = Vec::with_capacity(doc_colors.len());
    let mut colors = Vec::with_capacity(doc_colors.len());

    for (pos, color) in doc_colors {
        color_swatches_padding.push(InlineAnnotation::new(pos, " "));
        color_swatches.push(InlineAnnotation::new(pos, "■"));
        colors.push(Theme::rgb_highlight(
            (color.red * 255.) as u8,
            (color.green * 255.) as u8,
            (color.blue * 255.) as u8,
        ));
    }

    doc.color_swatches = Some(DocumentColorSwatches {
        color_swatches,
        colors,
        color_swatches_padding,
    });
}

pub(super) fn register_hooks(handlers: &Handlers) {
    register_hook!(move |event: &mut DocumentDidOpen<'_>| {
        // when a document is initially opened, request colors for it
        request_document_colors(event.editor, event.doc);

        Ok(())
    });

    let tx = handlers.document_colors.clone();
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        // Update the color swatch' positions, helping ensure they are displayed in the
        // proper place.
        let apply_color_swatch_changes = |annotations: &mut Vec<InlineAnnotation>| {
            event.changes.update_positions(
                annotations
                    .iter_mut()
                    .map(|annotation| (&mut annotation.char_idx, helix_core::Assoc::After)),
            );
        };

        if let Some(DocumentColorSwatches {
            color_swatches,
            colors: _colors,
            color_swatches_padding,
        }) = &mut event.doc.color_swatches
        {
            apply_color_swatch_changes(color_swatches);
            apply_color_swatch_changes(color_swatches_padding);
        }

        // Avoid re-requesting document colors if the change is a ghost transaction (completion)
        // because the language server will not know about the updates to the document and will
        // give out-of-date locations.
        if !event.ghost_transaction {
            // Cancel the ongoing request, if present.
            event.doc.color_swatch_controller.cancel();
            helix_event::send_blocking(&tx, DocumentColorsEvent(event.doc.id()));
        }

        Ok(())
    });

    register_hook!(move |event: &mut LanguageServerInitialized<'_>| {
        document_requests::request_all::<DocumentColorsEvent>(event.editor);

        Ok(())
    });

    register_hook!(move |event: &mut LanguageServerExited<'_>| {
        // Clear and re-request colors only in documents affected by this server.
        let affected_documents: Vec<_> = event
            .editor
            .documents_mut()
            .filter(|doc| doc.supports_language_server(event.server_id))
            .map(|doc| {
                doc.color_swatches.take();
                doc.id()
            })
            .collect();

        for doc_id in affected_documents {
            request_document_colors(event.editor, doc_id);
        }

        Ok(())
    });
}
