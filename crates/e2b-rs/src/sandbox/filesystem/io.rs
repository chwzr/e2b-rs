//! Byte I/O for the sandbox filesystem (`read`/`write` over envd `/files`).

use futures::StreamExt as _;

use super::{Filesystem, file_not_found_on_missing};
use crate::errors::{Error, Result};

impl Filesystem {
    /// Read a file as UTF-8 text. Gzip-encoded responses are transparently
    /// decompressed (reqwest's `gzip` feature), so no caller-side toggle is needed.
    pub async fn read(&self, path: &str, user: Option<&str>) -> Result<String> {
        let bytes = self.read_bytes(path, user).await?;
        String::from_utf8(bytes)
            .map_err(|e| Error::Internal(format!("file {path} is not valid UTF-8: {e}")))
    }

    /// Read a file as raw bytes.
    pub async fn read_bytes(&self, path: &str, user: Option<&str>) -> Result<Vec<u8>> {
        let user = self.resolve_user(user);
        let resp = self
            .rest
            .get_files(path, user.as_deref(), false)
            .await
            .map_err(|e| file_not_found_on_missing(e, path))?;
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Read a file as a stream of byte chunks (for large files). The Global
    /// Constraints allow byte-body reads to be `impl Stream` (no background
    /// task / channel needed — the response body IS already a stream).
    pub async fn read_stream(
        &self,
        path: &str,
        user: Option<&str>,
    ) -> Result<impl futures::Stream<Item = Result<bytes::Bytes>>> {
        let user = self.resolve_user(user);
        let resp = self
            .rest
            .get_files(path, user.as_deref(), false)
            .await
            .map_err(|e| file_not_found_on_missing(e, path))?;
        Ok(resp.bytes_stream().map(|chunk| chunk.map_err(Error::from)))
    }
}
