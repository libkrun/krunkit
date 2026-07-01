// SPDX-License-Identifier: Apache-2.0

use std::{
    fs::File,
    io::{ErrorKind, Read, Write},
    net::{Ipv4Addr, TcpListener},
    os::{
        fd::{FromRawFd, RawFd},
        unix::net::UnixListener,
    },
    str::FromStr,
};

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

#[link(name = "krun")]
extern "C" {
    fn krun_get_shutdown_eventfd(ctx_id: u32) -> i32;
}

const VM_STATE_PATH: &str = "/vm/state";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UriScheme {
    Tcp,
    Unix,
    #[default]
    None,
}

impl FromStr for UriScheme {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tcp" => Ok(Self::Tcp),
            "unix" => Ok(Self::Unix),
            "none" => Ok(Self::None),
            _ => Err(anyhow!("invalid scheme")),
        }
    }
}

/// Socket address in which the restful URI socket should listen on. Identical to Rust's
/// SocketAddrV4, but requires a modified FromStr implementation due to how the address is
/// presented on the command line.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum RestfulUri {
    Tcp(Ipv4Addr, u16),
    Unix(String),
    #[default]
    None,
}

impl FromStr for RestfulUri {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let expression = regex::Regex::new(r"^(?P<scheme>none|tcp|unix)://(?P<value>.*)").unwrap();
        let Some(cap) = expression.captures(s) else {
            return Err(anyhow!("invalid scheme input"));
        };
        let scheme = &cap["scheme"];
        let value = &cap["value"];
        match UriScheme::from_str(scheme)? {
            UriScheme::Tcp => {
                let (ip_addr, port) = parse_tcp_input(value)?;
                Ok(Self::Tcp(ip_addr, port))
            }
            UriScheme::Unix => {
                if value.is_empty() {
                    return Err(anyhow!("empty unix socket path"));
                }
                Ok(Self::Unix(value.to_string()))
            }
            UriScheme::None => Ok(Self::None),
        }
    }
}

fn parse_tcp_input(input: &str) -> Result<(Ipv4Addr, u16), anyhow::Error> {
    let mut parts: Vec<String> = input.split(':').map(|s| s.to_string()).collect();
    if parts.len() != 2 {
        return Err(anyhow!("restful URI formatted incorrectly"));
    }

    // Ipv4Address's FromStr does not understand that the "localhost" IP address translates to
    // 127.0.0.1, this must be manually translated.
    if &parts[0][..] == "localhost" {
        parts[0] = String::from("127.0.0.1");
    }

    let ip_addr =
        Ipv4Addr::from_str(&parts[0]).context("restful URI IP address formatted incorrectly")?;
    let port = u16::from_str(&parts[1]).context("restful URI port number formatted incorrectly")?;
    Ok((ip_addr, port))
}

/// Retrieve the shutdown event file descriptor initialized by libkrun.
pub unsafe fn get_shutdown_eventfd(ctx_id: u32) -> i32 {
    let fd = krun_get_shutdown_eventfd(ctx_id);
    if fd < 0 {
        panic!("unable to retrieve krun shutdown file descriptor");
    }
    fd
}

fn write_http_response<T: Write>(stream: &mut T, status: u16, reason: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len(),
    );
    if let Err(e) = stream.write_all(response.as_bytes()) {
        log::error!("Failed to write HTTP response: {e}");
    }
}

fn write_http_error<T: Write>(stream: &mut T, status: u16, reason: &str) {
    let response = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\n\r\n");
    if let Err(e) = stream.write_all(response.as_bytes()) {
        log::error!("Failed to write HTTP response: {e}");
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VmStateResponse {
    state: String,
    can_start: bool,
    can_pause: bool,
    can_resume: bool,
    can_stop: bool,
    can_hard_stop: bool,
}

impl VmStateResponse {
    fn new(state: &str, can_stop: bool) -> Self {
        Self {
            state: state.to_string(),
            can_start: false,
            can_pause: false,
            can_resume: false,
            can_stop,
            can_hard_stop: can_stop,
        }
    }
}

#[derive(Deserialize)]
struct VmStateRequest {
    state: String,
}

fn normalize_path(path: &str) -> &str {
    let path = path.split('?').next().unwrap_or(path);
    path.strip_suffix('/').unwrap_or(path)
}

fn content_length(headers: &[httparse::Header]) -> usize {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("Content-Length"))
        .and_then(|h| std::str::from_utf8(h.value).ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn handle_incoming_stream<T: Read + Write>(
    stream: &mut T,
    shutdown_fd: &mut File,
    stopping: &mut bool,
) {
    let mut buf = [0u8; 4096];
    let sz = match stream.read(&mut buf) {
        Ok(0) => return,
        Ok(sz) => sz,
        Err(e) => {
            log::error!("Failed to read from stream: {e}");
            return;
        }
    };

    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);
    let header_len = match req.parse(&buf[..sz]) {
        Ok(httparse::Status::Complete(len)) => len,
        Ok(httparse::Status::Partial) => {
            write_http_error(stream, 400, "Bad Request");
            return;
        }
        Err(_) => {
            write_http_error(stream, 400, "Bad Request");
            return;
        }
    };

    let method = match req.method {
        Some(m) => m,
        None => {
            write_http_error(stream, 400, "Bad Request");
            return;
        }
    };

    let path = match req.path {
        Some(p) => p,
        None => {
            write_http_error(stream, 400, "Bad Request");
            return;
        }
    };

    if normalize_path(path) != VM_STATE_PATH {
        write_http_error(stream, 404, "Not Found");
        return;
    }

    match method {
        "GET" => {
            let (state, can_stop) = if *stopping {
                ("VirtualMachineStateStopping", false)
            } else {
                ("VirtualMachineStateRunning", true)
            };
            let body = serde_json::to_string(&VmStateResponse::new(state, can_stop)).unwrap();
            write_http_response(stream, 200, "OK", &body);
        }
        "POST" => {
            let body_len = content_length(req.headers);
            let body_end = std::cmp::min(header_len + body_len, sz);
            let body = &buf[header_len..body_end];

            let state_req: VmStateRequest = match serde_json::from_slice(body) {
                Ok(r) => r,
                Err(_) => {
                    write_http_response(
                        stream,
                        400,
                        "Bad Request",
                        "{\"error\":\"missing or invalid 'state' field\"}",
                    );
                    return;
                }
            };

            match state_req.state.as_str() {
                "Stop" | "HardStop" => {
                    *stopping = true;
                    let body = serde_json::to_string(&VmStateResponse::new(
                        "VirtualMachineStateStopping",
                        false,
                    ))
                    .unwrap();
                    write_http_response(stream, 200, "OK", &body);
                    if let Err(e) = shutdown_fd.write_all(&1u64.to_le_bytes()) {
                        log::error!("Failed to write to shutdown fd: {e}");
                    }
                }
                other => {
                    let error = format!("{{\"error\":\"unsupported state change: {other}\"}}");
                    write_http_response(stream, 400, "Bad Request", &error);
                }
            }
        }
        _ => {
            write_http_error(stream, 405, "Method Not Allowed");
        }
    }
}

/// Listen for status and shutdown requests from the client. Shut down the krun VM when prompted.
pub fn status_listener(
    shutdown_eventfd: RawFd,
    addr: Option<RestfulUri>,
) -> Result<(), anyhow::Error> {
    // VM is shut down by writing to the shutdown event file.
    let mut shutdown = unsafe { File::from_raw_fd(shutdown_eventfd) };

    let addr = addr.unwrap_or_default();

    let mut stopping = false;

    match addr {
        RestfulUri::Tcp(addr, port) => {
            let listener = TcpListener::bind((addr, port))
                .map_err(|e| anyhow!("Unable to bind to TCP listener: {}", e))?;

            for stream in listener.incoming() {
                handle_incoming_stream(&mut stream.unwrap(), &mut shutdown, &mut stopping)
            }
        }
        RestfulUri::Unix(path) => {
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != ErrorKind::NotFound {
                    return Err(anyhow!("failed to remove socket with error {e}"));
                }
            }
            let listener = UnixListener::bind(path)
                .map_err(|e| anyhow!("Unable to bind to unix socket: {}", e))?;

            for stream in listener.incoming() {
                handle_incoming_stream(&mut stream.unwrap(), &mut shutdown, &mut stopping)
            }
        }
        RestfulUri::None => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_request(method: &str, path: &str, body: Option<&str>) -> Vec<u8> {
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
        if let Some(b) = body {
            req.push_str(&format!("Content-Length: {}\r\n", b.len()));
            req.push_str("Content-Type: application/json\r\n");
        }
        req.push_str("\r\n");
        if let Some(b) = body {
            req.push_str(b);
        }
        req.into_bytes()
    }

    struct MockStream {
        read: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl std::io::Read for MockStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.read.read(buf)
        }
    }

    impl std::io::Write for MockStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn handle_request(request: &[u8], stopping: &mut bool) -> String {
        let mut stream = MockStream {
            read: Cursor::new(request.to_vec()),
            written: Vec::new(),
        };

        let (sock_a, _sock_b) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut shutdown_fd =
            unsafe { File::from_raw_fd(std::os::fd::AsRawFd::as_raw_fd(&sock_a)) };

        handle_incoming_stream(&mut stream, &mut shutdown_fd, stopping);

        std::mem::forget(shutdown_fd);

        String::from_utf8(stream.written).unwrap()
    }

    fn response_status(response: &str) -> u16 {
        response
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap()
    }

    fn response_body(response: &str) -> &str {
        response.split("\r\n\r\n").nth(1).unwrap_or("")
    }

    #[test]
    fn get_vm_state_returns_running() {
        let req = make_request("GET", "/vm/state", None);
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 200);
        let body = response_body(&resp);
        assert!(body.contains("\"state\":\"VirtualMachineStateRunning\""));
        assert!(body.contains("\"canStop\":true"));
    }

    #[test]
    fn get_vm_state_trailing_slash() {
        let req = make_request("GET", "/vm/state/", None);
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 200);
        assert!(response_body(&resp).contains("VirtualMachineStateRunning"));
    }

    #[test]
    fn get_vm_state_while_stopping() {
        let req = make_request("GET", "/vm/state", None);
        let mut stopping = true;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 200);
        let body = response_body(&resp);
        assert!(body.contains("\"state\":\"VirtualMachineStateStopping\""));
        assert!(body.contains("\"canStop\":false"));
    }

    #[test]
    fn post_stop_returns_stopping() {
        let req = make_request("POST", "/vm/state", Some("{\"state\":\"Stop\"}"));
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 200);
        assert!(response_body(&resp).contains("VirtualMachineStateStopping"));
        assert!(stopping);
    }

    #[test]
    fn post_hardstop_returns_stopping() {
        let req = make_request("POST", "/vm/state", Some("{\"state\":\"HardStop\"}"));
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 200);
        assert!(stopping);
    }

    #[test]
    fn post_invalid_state_returns_400() {
        let req = make_request("POST", "/vm/state", Some("{\"state\":\"Pause\"}"));
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 400);
        assert!(!stopping);
    }

    #[test]
    fn post_missing_body_returns_400() {
        let req = make_request("POST", "/vm/state", None);
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 400);
    }

    #[test]
    fn unknown_path_returns_404() {
        let req = make_request("GET", "/vm/inspect", None);
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 404);
    }

    #[test]
    fn unknown_method_returns_405() {
        let req = make_request("DELETE", "/vm/state", None);
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 405);
    }

    #[test]
    fn path_with_query_string() {
        let req = make_request("GET", "/vm/state?foo=bar", None);
        let mut stopping = false;
        let resp = handle_request(&req, &mut stopping);
        assert_eq!(response_status(&resp), 200);
    }

    #[test]
    fn deserialize_vm_state_request() {
        let r: VmStateRequest = serde_json::from_str("{\"state\":\"Stop\"}").unwrap();
        assert_eq!(r.state, "Stop");
        let r: VmStateRequest = serde_json::from_str("{ \"state\" : \"HardStop\" }").unwrap();
        assert_eq!(r.state, "HardStop");
    }

    #[test]
    fn deserialize_vm_state_request_invalid() {
        assert!(serde_json::from_str::<VmStateRequest>("").is_err());
        assert!(serde_json::from_str::<VmStateRequest>("{}").is_err());
        assert!(serde_json::from_str::<VmStateRequest>("not json").is_err());
    }

    #[test]
    fn serialize_vm_state_response() {
        let resp = VmStateResponse::new("VirtualMachineStateRunning", true);
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert_eq!(json["state"], "VirtualMachineStateRunning");
        assert_eq!(json["canStop"], true);
        assert_eq!(json["canHardStop"], true);
        assert_eq!(json["canPause"], false);
    }

    #[test]
    fn parse_valid_unix_scheme() {
        assert_eq!(
            RestfulUri::Unix("/tmp/path".to_string()),
            RestfulUri::from_str("unix:///tmp/path").unwrap()
        );
    }

    #[test]
    fn parse_unix_scheme_missing_path() {
        assert_eq!(
            anyhow!("empty unix socket path").to_string(),
            RestfulUri::from_str("unix://").err().unwrap().to_string()
        );
    }

    #[test]
    fn parse_unix_scheme_missing_slashes() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("unix:").err().unwrap().to_string()
        );
    }

    #[test]
    fn parse_unix_scheme_misspelling() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("uni://path")
                .err()
                .unwrap()
                .to_string()
        );
    }

    #[test]
    fn parse_valid_tcp_scheme() {
        assert_eq!(
            RestfulUri::Tcp(Ipv4Addr::new(127, 0, 0, 1), 8080),
            RestfulUri::from_str("tcp://localhost:8080").unwrap(),
        );
    }

    #[test]
    fn parse_tcp_scheme_missing_port() {
        assert_eq!(
            anyhow!("restful URI formatted incorrectly").to_string(),
            RestfulUri::from_str("tcp://localhost")
                .err()
                .unwrap()
                .to_string()
        );
    }

    #[test]
    fn parse_tcp_scheme_with_unix_path() {
        assert_eq!(
            anyhow!("restful URI formatted incorrectly").to_string(),
            RestfulUri::from_str("tcp:///tmp/path")
                .err()
                .unwrap()
                .to_string(),
        );
    }

    #[test]
    fn parse_valid_none_scheme() {
        assert_eq!(RestfulUri::None, RestfulUri::from_str("none://").unwrap());
    }

    #[test]
    fn parse_none_scheme_missing_postfix() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("none").err().unwrap().to_string(),
        );
    }

    #[test]
    fn parse_random_string_scheme() {
        assert_eq!(
            anyhow!("invalid scheme input").to_string(),
            RestfulUri::from_str("foobar").err().unwrap().to_string(),
        );
    }
}
