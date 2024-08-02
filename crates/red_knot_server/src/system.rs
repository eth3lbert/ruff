use std::any::Any;
use std::fmt::Display;
use std::sync::Arc;

use lsp_types::Url;

use ruff_db::file_revision::FileRevision;
use ruff_db::system::walk_directory::WalkDirectoryBuilder;
use ruff_db::system::{
    DirectoryEntry, FileType, Metadata, OsSystem, Result, System, SystemPath, SystemPathBuf,
    SystemVirtualPath,
};
use ruff_notebook::{Notebook, NotebookError};

use crate::session::index::Index;
use crate::DocumentQuery;

/// Converts the given [`Url`] to a [`SystemPathBuf`].
///
/// This fails in the following cases:
/// * The URL scheme is not `file`.
/// * The URL cannot be converted to a file path (refer to [`Url::to_file_path`]).
/// * If the URL is not a valid UTF-8 string.
pub(crate) fn url_to_system_path(url: &Url) -> std::result::Result<SystemPathBuf, ()> {
    if url.scheme() == "file" {
        Ok(SystemPathBuf::from_path_buf(url.to_file_path()?).map_err(|_| ())?)
    } else {
        Err(())
    }
}

#[derive(Debug)]
pub(crate) struct LSPSystem {
    /// A read-only copy of the index where the server stores all the open documents and settings.
    index: Option<Arc<Index>>,

    /// A system implementation that uses the local file system.
    os_system: OsSystem,
}

impl LSPSystem {
    pub(crate) fn new(index: Arc<Index>) -> Self {
        let cwd = std::env::current_dir().unwrap();
        let os_system = OsSystem::new(SystemPathBuf::from_path_buf(cwd).unwrap());

        Self {
            index: Some(index),
            os_system,
        }
    }

    /// Takes the index out of the system.
    pub(crate) fn take_index(&mut self) -> Option<Arc<Index>> {
        self.index.take()
    }

    /// Sets the index for the system.
    pub(crate) fn set_index(&mut self, index: Arc<Index>) {
        self.index = Some(index);
    }

    /// Returns a reference to the contained index.
    ///
    /// # Panics
    ///
    /// Panics if the index is `None`.
    fn index(&self) -> &Index {
        self.index.as_ref().unwrap()
    }

    fn make_document_ref(&self, url: Url) -> Result<DocumentQuery> {
        let index = self.index();
        let key = index.key_from_url(url);
        index.make_document_ref(key).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Document not found in the index",
            )
        })
    }

    fn system_path_to_document_ref(&self, path: &SystemPath) -> Result<DocumentQuery> {
        let url = Url::from_file_path(path.as_std_path()).map_err(|()| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Failed to convert system path to URL: {path:?}"),
            )
        })?;
        self.make_document_ref(url)
    }

    fn system_virtual_path_to_document_ref(
        &self,
        path: &SystemVirtualPath,
    ) -> Result<DocumentQuery> {
        let url = Url::parse(path.as_str()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Failed to convert virtual path to URL: {path:?}"),
            )
        })?;
        self.make_document_ref(url)
    }
}

impl System for LSPSystem {
    fn path_metadata(&self, path: &SystemPath) -> Result<Metadata> {
        let document = self.system_path_to_document_ref(path);

        // First, we need to check if the document is opened in the editor. If it is, we need to
        // use the document's version as the file revision. Otherwise, fall back to the OS system.
        match document {
            Ok(document) => {
                // The file revision is just an opaque number which doesn't have any significant
                // meaning other than that the file has changed if the revisions are different.
                #[allow(clippy::cast_sign_loss)]
                Ok(Metadata::new(
                    FileRevision::new(document.version() as u128),
                    None,
                    FileType::File,
                ))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.os_system.path_metadata(path)
            }
            Err(err) => Err(err),
        }
    }

    fn canonicalize_path(&self, path: &SystemPath) -> Result<SystemPathBuf> {
        self.os_system.canonicalize_path(path)
    }

    fn read_to_string(&self, path: &SystemPath) -> Result<String> {
        let document = self.system_path_to_document_ref(path);

        match document {
            Ok(document) => {
                if let DocumentQuery::Text { document, .. } = &document {
                    Ok(document.contents().to_string())
                } else {
                    Err(not_a_text_document(path))
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.os_system.read_to_string(path)
            }
            Err(err) => Err(err),
        }
    }

    fn read_to_notebook(&self, path: &SystemPath) -> std::result::Result<Notebook, NotebookError> {
        let document = self.system_path_to_document_ref(path);

        match document {
            Ok(document) => {
                if let DocumentQuery::Notebook { notebook, .. } = &document {
                    Ok(notebook.make_ruff_notebook())
                } else {
                    Err(not_a_notebook(path))
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.os_system.read_to_notebook(path)
            }
            Err(err) => Err(NotebookError::from(err)),
        }
    }

    fn virtual_path_metadata(&self, path: &SystemVirtualPath) -> Result<Metadata> {
        // Virtual paths only exists in the LSP system, so we don't need to check the OS system.
        let document = self.system_virtual_path_to_document_ref(path)?;

        // The file revision is just an opaque number which doesn't have any significant
        // meaning other than that the file has changed if the revisions are different.
        #[allow(clippy::cast_sign_loss)]
        Ok(Metadata::new(
            FileRevision::new(document.version() as u128),
            None,
            FileType::File,
        ))
    }

    fn read_virtual_path_to_string(&self, path: &SystemVirtualPath) -> Result<String> {
        let document = self.system_virtual_path_to_document_ref(path)?;

        if let DocumentQuery::Text { document, .. } = &document {
            Ok(document.contents().to_string())
        } else {
            Err(not_a_text_document(path))
        }
    }

    fn read_virtual_path_to_notebook(
        &self,
        path: &SystemVirtualPath,
    ) -> std::result::Result<Notebook, NotebookError> {
        let document = self.system_virtual_path_to_document_ref(path)?;

        if let DocumentQuery::Notebook { notebook, .. } = &document {
            Ok(notebook.make_ruff_notebook())
        } else {
            Err(not_a_notebook(path))
        }
    }

    fn current_directory(&self) -> &SystemPath {
        self.os_system.current_directory()
    }

    fn read_directory<'a>(
        &'a self,
        path: &SystemPath,
    ) -> Result<Box<dyn Iterator<Item = Result<DirectoryEntry>> + 'a>> {
        self.os_system.read_directory(path)
    }

    fn walk_directory(&self, path: &SystemPath) -> WalkDirectoryBuilder {
        self.os_system.walk_directory(path)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn not_a_text_document(path: impl Display) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("Input is not a text document: {path}"),
    )
}

fn not_a_notebook(path: impl Display) -> NotebookError {
    NotebookError::from(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("Input is not a notebook: {path}"),
    ))
}
