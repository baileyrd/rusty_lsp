//! Asynchronous LSP message framing.
//!
//! LSP frames each JSON-RPC payload with HTTP-style headers:
//!
//! ```text
//! Content-Length: 123\r\n
//! \r\n
//! {"jsonrpc":"2.0", ...}
//! ```
//!
//! These functions read and write that framing over any async byte stream, so
//! the same plumbing serves stdio, sockets, or in-memory pipes (used by the
//! integration tests). They are public so applications can build custom
//! transports — proxies, multiplexers, test harnesses — on the same wire
//! format the [`crate::Server`] uses.

use crate::error::{Error, Result};
use crate::jsonrpc::Message;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const CONTENT_LENGTH: &str = "content-length";
/// Guards against a malformed/hostile `Content-Length` exhausting memory.
const MAX_CONTENT_LENGTH: usize = 256 * 1024 * 1024;

/// Read a single framed [`Message`] from `reader`.
///
/// Returns `Ok(None)` on a clean end-of-stream (the peer closed the
/// connection at a frame boundary), and an error if the stream ends
/// mid-frame or the framing is malformed.
pub async fn read_message<R>(reader: &mut R) -> Result<Option<Message>>
where
    R: AsyncBufRead + Unpin,
{
    let Some(content_length) = read_headers(reader).await? else {
        return Ok(None);
    };

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::protocol("connection closed mid-message")
        } else {
            Error::Io(e)
        }
    })?;

    let message = serde_json::from_slice(&body)?;
    Ok(Some(message))
}

/// Read the header block, returning the parsed `Content-Length`.
///
/// `Ok(None)` signals a clean EOF before any header bytes were seen.
async fn read_headers<R>(reader: &mut R) -> Result<Option<usize>>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    let mut saw_any_byte = false;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            // EOF. Clean only if it lands exactly on a frame boundary.
            return if saw_any_byte {
                Err(Error::protocol("connection closed inside header block"))
            } else {
                Ok(None)
            };
        }
        saw_any_byte = true;

        // A bare CRLF (or LF) terminates the header block.
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case(CONTENT_LENGTH) {
                let value = value.trim();
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| Error::protocol(format!("invalid Content-Length: {value:?}")))?;
                content_length = Some(parsed);
            }
            // Other headers (e.g. Content-Type) are accepted and ignored.
        } else {
            return Err(Error::protocol(format!(
                "malformed header line: {trimmed:?}"
            )));
        }
    }

    match content_length {
        Some(len) if len > MAX_CONTENT_LENGTH => Err(Error::protocol(format!(
            "Content-Length {len} exceeds limit"
        ))),
        Some(len) => Ok(Some(len)),
        None => Err(Error::protocol("missing Content-Length header")),
    }
}

/// Write a single framed [`Message`] to `writer` and flush it.
pub async fn write_message<W>(writer: &mut W, message: &Message) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(message)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

/// Convenience: wrap a reader so [`read_message`] can be called repeatedly.
///
/// Equivalent to `tokio::io::BufReader::new(reader)`, re-exported so callers do
/// not need to remember that [`read_message`] requires a buffered source.
pub fn buffered<R: tokio::io::AsyncRead + Unpin>(reader: R) -> tokio::io::BufReader<R> {
    tokio::io::BufReader::new(reader)
}
