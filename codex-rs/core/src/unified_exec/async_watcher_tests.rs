use super::resolve_aggregated_output;
use super::split_valid_utf8_prefix_with_max;
use crate::unified_exec::head_tail_buffer::HeadTailBuffer;

use pretty_assertions::assert_eq;
use std::sync::Arc;
use tokio::sync::Mutex;

#[test]
fn split_valid_utf8_prefix_respects_max_bytes_for_ascii() {
    let mut buf = b"hello word!".to_vec();

    let first =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 5).expect("expected prefix");
    assert_eq!(first, b"hello".to_vec());
    assert_eq!(buf, b" word!".to_vec());

    let second =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 5).expect("expected prefix");
    assert_eq!(second, b" word".to_vec());
    assert_eq!(buf, b"!".to_vec());
}

#[test]
fn split_valid_utf8_prefix_avoids_splitting_utf8_codepoints() {
    // "é" is 2 bytes in UTF-8. With a max of 3 bytes, we should only emit 1 char (2 bytes).
    let mut buf = "ééé".as_bytes().to_vec();

    let first =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 3).expect("expected prefix");
    assert_eq!(std::str::from_utf8(&first).unwrap(), "é");
    assert_eq!(buf, "éé".as_bytes().to_vec());
}

#[test]
fn split_valid_utf8_prefix_makes_progress_on_invalid_utf8() {
    let mut buf = vec![0xff, b'a', b'b'];

    let first =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 2).expect("expected prefix");
    assert_eq!(first, vec![0xff]);
    assert_eq!(buf, b"ab".to_vec());
}

#[tokio::test]
async fn resolve_aggregated_output_marks_omitted_output() {
    let mut buffer = HeadTailBuffer::new(/*max_bytes*/ 10);
    buffer.push_chunk(b"0123456789".to_vec());
    buffer.push_chunk(b"ab".to_vec());
    let transcript = Arc::new(Mutex::new(buffer));

    assert_eq!(
        resolve_aggregated_output(&transcript, "fallback".to_string()).await,
        "01234... [2 bytes omitted from the middle of command output] ...789ab"
    );
}
