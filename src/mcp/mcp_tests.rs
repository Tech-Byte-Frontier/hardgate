use super::read_mcp_message;
use std::io::Cursor;

fn framed(headers: &[&str], body: &[u8]) -> Vec<u8> {
    let mut input = Vec::new();
    for header in headers {
        input.extend_from_slice(header.as_bytes());
        input.extend_from_slice(b"\r\n");
    }
    input.extend_from_slice(b"\r\n");
    input.extend_from_slice(body);
    input
}

#[test]
fn normal_content_length_frame_reads_exact_body() {
    let body = br#"{"jsonrpc":"2.0","method":"ping"}"#;
    let header = format!("Content-Length: {}", body.len());
    let mut reader = Cursor::new(framed(&[header.as_str()], body));

    assert_eq!(
        read_mcp_message(&mut reader).expect("valid framing should parse"),
        Some(String::from_utf8(body.to_vec()).expect("test body is UTF-8"))
    );
}

#[test]
fn duplicate_equal_content_lengths_are_accepted() {
    let body = b"{}";
    let mut reader = Cursor::new(framed(&["Content-Length: 02", "content-length: 2"], body));

    assert_eq!(
        read_mcp_message(&mut reader).expect("matching duplicate lengths should parse"),
        Some("{}".to_string())
    );
}

#[test]
fn conflicting_duplicate_lengths_are_rejected_before_body_read() {
    let body = b"payload";
    let input = framed(&["Content-Length: 3", "Content-Length: 4"], body);
    let mut reader = Cursor::new(input);
    let error = read_mcp_message(&mut reader).expect_err("conflicting lengths must fail closed");

    assert!(
        error
            .to_string()
            .contains("Conflicting MCP Content-Length headers")
    );
    let consumed = reader.position() as usize;
    assert_eq!(
        &reader.get_ref()[consumed..],
        b"\r\npayload",
        "the blank line and body must remain unread"
    );
}

#[test]
fn malformed_content_length_is_rejected_before_body_read() {
    let input = framed(&["Content-Length: not-a-number"], b"{}");
    let mut reader = Cursor::new(input);
    let error = read_mcp_message(&mut reader).expect_err("malformed length must fail closed");

    assert!(
        error
            .to_string()
            .contains("Malformed MCP Content-Length header")
    );
    let consumed = reader.position() as usize;
    assert_eq!(&reader.get_ref()[consumed..], b"\r\n{}");
}

#[test]
fn overflowing_content_length_is_rejected() {
    let header = format!("Content-Length: {}0", usize::MAX);
    let mut reader = Cursor::new(framed(&[header.as_str()], b"{}"));
    let error = read_mcp_message(&mut reader).expect_err("overflow must fail closed");

    assert!(
        error
            .to_string()
            .contains("MCP Content-Length value overflows usize")
    );
}

#[test]
fn truncated_content_length_body_is_rejected() {
    let mut reader = Cursor::new(framed(&["Content-Length: 5"], b"abc"));
    let error = read_mcp_message(&mut reader).expect_err("short body must fail closed");

    assert!(error.to_string().contains("failed to fill whole buffer"));
}

#[test]
fn truncated_content_length_headers_are_rejected() {
    let mut reader = Cursor::new(b"Content-Length: 2\r\n".to_vec());
    let error = read_mcp_message(&mut reader).expect_err("short headers must fail closed");

    assert!(
        error
            .to_string()
            .contains("Unexpected EOF in MCP Content-Length headers")
    );
}

#[test]
fn newline_delimited_body_remains_supported() {
    let mut reader = Cursor::new(
        br#"{"jsonrpc":"2.0"}
"#
        .to_vec(),
    );

    assert_eq!(
        read_mcp_message(&mut reader).expect("newline-delimited JSON should parse"),
        Some("{\"jsonrpc\":\"2.0\"}\n".to_string())
    );
}
