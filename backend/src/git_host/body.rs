//! Bounded decoded Git RPC input; no HTTP, process or authorization policy here.
use std::io::Read;
#[derive(Debug)]
pub(super) enum DecodeError {
    TooLarge,
    Invalid(std::io::Error),
}
pub(super) fn decode_gzip(reader: impl Read, limit: usize) -> Result<Vec<u8>, DecodeError> {
    let ceiling = limit.checked_add(1).ok_or(DecodeError::TooLarge)?;
    let mut decoder = flate2::read::GzDecoder::new(reader).take(ceiling as u64);
    let mut output = Vec::with_capacity(limit.min(8192));
    decoder
        .read_to_end(&mut output)
        .map_err(DecodeError::Invalid)?;
    if output.len() > limit {
        return Err(DecodeError::TooLarge);
    }
    Ok(output)
}
