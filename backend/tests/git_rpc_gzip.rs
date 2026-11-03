//! Compile and exercise the exact production reader boundary with small limits.
#[path = "../src/git_host/body.rs"]
mod body;
use body::{DecodeError, decode_gzip};
use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
struct Counted<R> {
    inner: R,
    read: Arc<AtomicUsize>,
}
impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.read.fetch_add(count, Ordering::SeqCst);
        Ok(count)
    }
}
#[test]
fn oversized_decoding_stops_reading_before_consuming_the_whole_input() {
    let mut state = 7u64;
    let bytes: Vec<u8> = (0..1024 * 1024)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as u8
        })
        .collect();
    let encoded = gzip(&bytes);
    let read = Arc::new(AtomicUsize::new(0));
    let result = decode_gzip(
        Counted {
            inner: &encoded[..],
            read: read.clone(),
        },
        4096,
    );
    assert!(matches!(result, Err(DecodeError::TooLarge)));
    assert!(
        read.load(Ordering::SeqCst) < encoded.len() / 2,
        "an oversized body must stop decompression early, rather than allocate and read it all first"
    );
}
#[test]
fn exact_limit_empty_and_invalid_gzip_keep_their_meaning() {
    assert_eq!(decode_gzip(&gzip(b"abcde")[..], 5).unwrap(), b"abcde");
    assert!(matches!(
        decode_gzip(&gzip(b"abcde")[..], 4),
        Err(DecodeError::TooLarge)
    ));
    assert!(decode_gzip(&gzip(b"")[..], 0).unwrap().is_empty());
    match decode_gzip(&b"not gzip"[..], 32) {
        Err(DecodeError::Invalid(error)) => drop(error),
        other => panic!("invalid input must remain a decoding error: {other:?}"),
    }
}
