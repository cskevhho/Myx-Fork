use crate::*;
use std::io::{Read, Write};

/// Exercise the real HTTP/detail path, including status handling and parsing.
fn playlist_response(status: &str, body: &str) -> Vec<LibItem> {
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/items", server.local_addr().unwrap());
    let response=format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = server.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4096];
        let _bytes_read = socket.read(&mut request).unwrap();
        socket.write_all(response.as_bytes()).unwrap();
    });
    let items = fetch_playlist_items(&http_client(), "test-token", url);
    worker.join().unwrap();
    items
}

#[test]
fn forbidden_playlist_explains_access_instead_of_claiming_it_is_empty() {
    let items = playlist_response(
        "403 Forbidden",
        r#"{"error":{"status":403,"message":"Forbidden"}}"#,
    );
    let text = items
        .iter()
        .map(|i| i.name.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("403"));
    assert!(text.contains("owner") || text.contains("own"));
    assert!(!text.contains("empty"));
}

#[test]
fn empty_playlist_and_failed_request_are_distinct() {
    let empty = playlist_response("200 OK", r#"{"items":[],"next":null}"#);
    assert!(empty.iter().any(|i| i.name.contains("empty")));
    let failed = playlist_response("500 Internal Server Error", r#"{"error":{"status":500}}"#);
    assert!(failed.iter().any(|i| i.name.contains("500")));
    assert!(!failed.iter().any(|i| i.name.contains("empty")));
}

#[test]
fn owned_playlist_still_returns_selectable_tracks() {
    let items = playlist_response(
        "200 OK",
        r#"{"items":[{"item":{"name":"Track","uri":"spotify:track:a","artists":[{"name":"Artist"}]}}],"next":null}"#,
    );
    assert_eq!(items.len(), 1);
    assert!(items[0].is_track());
    assert_eq!(items[0].name, "Track");
}
