//! Loopback REST channel hosted by the GUI process. `OpenCADStudio.exe
//! "<file>" --http <port>` boots the normal editor AND serves this bridge, so
//! a client can open a drawing, let the person at the screen pick sample
//! entities, and read them back with `get_selection` over plain HTTP —
//! something the headless `--http` server cannot do (it owns a private,
//! window-less app). Requests are forwarded into the GUI's automation queue
//! (`Envelope`, exactly like the native bridge in `transport`); the JSON
//! response is written back over HTTP. The channel is deliberately
//! read-only: a local process that can reach the port must not be able to
//! mutate the drawing a person is working on.

use super::{Envelope, Reply};
use crate::rest::{self, HttpRequest};
use iced::futures::{channel::mpsc, Stream};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, AtomicU16, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Port handed over by `main` when the GUI hosts the channel. Set before the
/// iced runtime boots, so the subscription worker sees it on first run.
/// Zero means "no channel" (plain GUI runs).
static GUI_HTTP_PORT: AtomicU16 = AtomicU16::new(0);
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Called from `main` before the GUI boots.
pub fn set_gui_http_port(port: u16) {
    GUI_HTTP_PORT.store(port, Ordering::SeqCst);
}

pub(in crate::app) fn subscribe() -> iced::Subscription<Envelope> {
    iced::Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Envelope> {
    iced::stream::channel(32, |sender| async move {
        let port = GUI_HTTP_PORT.load(Ordering::SeqCst);
        if port != 0 {
            std::thread::spawn(move || {
                if let Err(error) = listen(sender, port, Arc::new(AtomicU16::new(0))) {
                    eprintln!("--http (GUI): cannot serve 127.0.0.1:{port}: {error}");
                }
            });
        }
        iced::futures::future::pending::<()>().await;
    })
}

fn listen(
    sender: mpsc::Sender<Envelope>,
    port: u16,
    bound_port: Arc<AtomicU16>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    bound_port.store(listener.local_addr()?.port(), Ordering::SeqCst);
    eprintln!("OpenCADStudio GUI REST listening on http://127.0.0.1:{port}/api/v1");
    // One thread per connection: a `getpoint` request stays open until the
    // person answers, and must not block the other reads (get_selection…).
    let clients = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming().flatten() {
        if clients.fetch_add(1, Ordering::SeqCst) >= 8 {
            clients.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let clients = clients.clone();
        let sender = sender.clone();
        std::thread::spawn(move || {
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
            if let Ok(Some(request)) = rest::read_request(&mut stream) {
                if request.method == "OPTIONS" {
                    let _ = rest::write_response(&mut stream, 204, &Value::Null);
                } else {
                    let (status, body) = forward(&request, &sender);
                    let _ = rest::write_response(&mut stream, status, &body);
                }
            }
            clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

/// The (method, path) pairs this channel serves, with how long one request
/// may wait for the GUI's answer. Reads only — `get_selection` is the read
/// back, `getpoint` parks until the person clicks (or presses Escape); a
/// long wait is the point, so pollers see `running` the whole time. Anything
/// else is refused before it can touch the GUI.
fn route(method: &str, segments: &[&str]) -> Option<(&'static str, u64)> {
    const PICK_WAIT_SECS: u64 = 30 * 60;
    match (method, segments) {
        ("POST", ["get_selection"]) => Some(("get_selection", 15)),
        ("POST", ["getpoint"]) => Some(("getpoint", PICK_WAIT_SECS)),
        ("GET", ["state"] | ["documents"]) => Some(("state", 15)),
        ("GET", ["capabilities"]) => Some(("capabilities", 15)),
        _ => None,
    }
}

fn forward(request: &HttpRequest, sender: &mpsc::Sender<Envelope>) -> (u16, Value) {
    let path = request.path.trim_start_matches("/api/v1");
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let (op, wait) = match route(&request.method, &segments) {
        Some(route) => route,
        None => return (403, json!({"ok":false,"code":"read_only_channel","error":"The GUI-hosted --http channel serves reads only: POST get_selection, POST getpoint, GET state, GET capabilities"})),
    };
    let mut request_json = request.json();
    if !request_json.is_object() {
        request_json = json!({});
    }
    request_json["op"] = json!(op);
    // The HTTP channel has no descriptor handshake — drop whatever the caller
    // guesses for session_id so the pipeline's session check stays silent.
    if let Some(object) = request_json.as_object_mut() {
        object.remove("session_id");
    }
    request_json["protocol"] = json!(1);
    if request_json["request_id"].as_str().is_none_or(str::is_empty) {
        let serial = REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        request_json["request_id"] = json!(format!("gui-http-{serial}"));
    }
    let (reply, response) = std::sync::mpsc::channel();
    if sender
        .clone()
        .try_send(Envelope {
            request: request_json,
            reply: Reply::Native(reply),
        })
        .is_err()
    {
        return (503, json!({"ok":false,"code":"busy","error":"GUI queue full"}));
    }
    let mut response = response
        .recv_timeout(Duration::from_secs(15))
        .unwrap_or_else(|_| {
            json!({"ok":false,"code":"response_timeout","error":"The GUI did not answer in time"})
        });
    // Interactive requests (`getpoint`) answer `accepted` first and only
    // complete when the person at the screen acts. Poll the operation on the
    // caller's behalf so the HTTP connection is the one thing that waits.
    if matches!(response["status"].as_str(), Some("accepted" | "running")) {
        let request_id = response["request_id"].clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(wait);
        while matches!(response["status"].as_str(), Some("accepted" | "running"))
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(100));
            let (poll_reply, poll_rx) = std::sync::mpsc::channel();
            if sender
                .clone()
                .try_send(Envelope {
                    request: json!({"protocol":1,"op":"operation","request_id":request_id}),
                    reply: Reply::Native(poll_reply),
                })
                .is_err()
            {
                break;
            }
            match poll_rx.recv_timeout(Duration::from_secs(15)) {
                Ok(poll) => response = poll,
                Err(_) => break,
            }
        }
    }
    (rest::map_status(response.clone(), false), response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use iced::futures::StreamExt;
    use std::io::{BufRead, BufReader, Read, Write as _};

    /// Each test owns its port holder: two bridge tests running in parallel
    /// must never read each other's bound port.
    fn wait_for_bridge(bound_port: &AtomicU16) -> u16 {
        for _ in 0..50 {
            let port = bound_port.load(Ordering::SeqCst);
            if port != 0 {
                return port;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("GUI http bridge never bound a port");
    }

    /// Post one request without waiting; the bridge only answers once the
    /// GUI side has replied to the forwarded envelope.
    fn post(port: u16, target: &str, body: &str) -> std::net::TcpStream {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let http = format!(
            "POST {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(http.as_bytes()).unwrap();
        stream
    }

    /// Read one HTTP response (status + JSON body) off the wire.
    fn read_response(stream: &mut std::net::TcpStream) -> (u16, Value) {
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if status_line.is_empty() {
                status_line = line.clone();
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = value.trim().parse().unwrap();
            }
        }
        let mut payload = vec![0u8; content_length];
        reader.read_exact(&mut payload).unwrap();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap();
        (status, serde_json::from_slice(&payload).unwrap())
    }

    #[test]
    fn bridge_serves_get_selection_over_http_and_refuses_writes() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        app.control_request(json!({"protocol":1,"op":"new","request_id":"n1"}));
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"LINE","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let value = u64::from_str_radix(&handle, 16).unwrap();
        app.tabs[app.active_tab]
            .scene
            .select_entity(acadrust::Handle::new(value), false);

        let (sender, mut receiver) = mpsc::channel::<Envelope>(8);
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || listen(sender, 0, listen_bound).unwrap());
        let port = wait_for_bridge(&bound);

        // Read the selection back over loopback HTTP the way the client does.
        let mut stream = post(
            port,
            "/api/v1/get_selection",
            r#"{"session_id":"guessed","request_id":"bridge-1"}"#,
        );
        // The bridge forwarded the envelope; this test plays the GUI and
        // answers it, which is what unblocks the HTTP response.
        let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
        assert_eq!(envelope.request["op"], "get_selection");
        assert!(envelope.request["session_id"].is_null());
        assert_eq!(envelope.request["request_id"], "bridge-1");
        let reply = app.control_request(envelope.request).0;
        envelope.reply.send(reply);

        let (status, body) = read_response(&mut stream);
        assert_eq!(status, 200);
        assert_eq!(body["ok"], true, "{body}");
        assert_eq!(body["status"], "completed");
        assert_eq!(body["result"]["count"], 1);
        assert_eq!(body["result"]["entities"][0]["handle"], handle.as_str());
        assert!(body["result"]["entities"][0]["bounds"].is_object(), "{body}");
        assert_eq!(body["result"]["entities"][0]["type"], "Line");

        // Writes are refused on this channel even though the op exists; the
        // bridge answers them locally without waking the GUI.
        let mut stream = post(port, "/api/v1/entities_delete", "{}");
        let (status, body) = read_response(&mut stream);
        assert_eq!(status, 403);
        assert_eq!(body["code"], "read_only_channel", "{body}");
    }

    #[test]
    fn bridge_parks_getpoint_without_blocking_other_reads() {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        app.control_request(json!({"protocol":1,"op":"new","request_id":"n1"}));
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"LINE","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let value = u64::from_str_radix(&handle, 16).unwrap();
        app.tabs[app.active_tab]
            .scene
            .select_entity(acadrust::Handle::new(value), false);

        let (sender, mut receiver) = mpsc::channel::<Envelope>(8);
        let bound = Arc::new(AtomicU16::new(0));
        let listen_bound = bound.clone();
        std::thread::spawn(move || listen(sender, 0, listen_bound).unwrap());
        let port = wait_for_bridge(&bound);

        // The client asks for a point and holds that connection open.
        let mut point_conn = post(
            port,
            "/api/v1/getpoint",
            r#"{"request_id":"gp-b","prompt":"Chọn vị trí QR"}"#,
        );
        // While the pick is pending, a second connection reads the selection
        // — thread-per-connection keeps the parked request from blocking it.
        let mut sel_conn = post(port, "/api/v1/get_selection", "{}");
        let mut polls = 0usize;
        let (point_status, point_body, sel_body) = loop {
            let envelope = iced::futures::executor::block_on(receiver.next()).unwrap();
            let reply = app.control_request(envelope.request.clone()).0;
            if envelope.request["op"] == "getpoint" {
                // The GUI accepted the pick; the bridge now polls `operation`
                // until the person answers. Simulate the click right away.
                assert_eq!(reply["status"], "accepted", "{reply}");
                envelope.reply.send(reply);
                app.tabs[app.active_tab].last_cursor_world =
                    glam::DVec3::new(1.5, 2.5, 0.0);
                app.update(crate::app::Message::ViewportLeftPress);
            } else if envelope.request["op"] == "get_selection" {
                envelope.reply.send(reply);
                let (status, body) = read_response(&mut sel_conn);
                assert_eq!(status, 200, "{body}");
                assert_eq!(body["result"]["count"], 1);
                assert_eq!(body["result"]["entities"][0]["handle"], handle.as_str());
            } else {
                // An `operation` poll from the parked getpoint connection.
                polls += 1;
                assert!(polls < 100, "getpoint never completed: {reply}");
                envelope.reply.send(reply.clone());
                if reply["status"] == "completed" {
                    let point_status = read_response(&mut point_conn);
                    break (point_status.0, point_status.1, Value::Null);
                }
            }
        };
        assert_eq!(point_status, 200, "{point_body}");
        assert_eq!(point_body["status"], "completed", "{point_body}");
        assert_eq!(point_body["result"]["point"], json!([1.5, 2.5, 0.0]));
        assert!(sel_body.is_null());
    }
}
