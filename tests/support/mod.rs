mod fixtures;
pub use fixtures::response;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: String,
    pub body: String,
}
pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub fn new(handler: impl Fn(&Request) -> (u16, String) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let worker_stop = stop.clone();
        let records = requests.clone();
        let handler = Arc::new(handler);
        let thread = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        let handler = handler.clone();
                        let records = records.clone();
                        thread::spawn(move || {
                            if let Some(req) = read_request(&mut socket) {
                                let (code, body) = handler(&req);
                                records.lock().unwrap().push(req);
                                let response=format!("HTTP/1.1 {code} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",body.len());
                                let _ = socket.write_all(response.as_bytes());
                            }
                        });
                    }
                    Err(_) => thread::sleep(Duration::from_millis(3)),
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.take().unwrap().join();
    }
}
fn read_request(socket: &mut TcpStream) -> Option<Request> {
    socket.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > 1024 * 1024 {
            return None;
        }
        if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_string();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("content-length") {
                        v.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            if bytes.len() < end + 4 + length {
                continue;
            }
            let mut first = headers.lines().next()?.split_whitespace();
            return Some(Request {
                method: first.next()?.into(),
                path: first.next()?.into(),
                headers: headers.clone(),
                body: String::from_utf8_lossy(&bytes[end + 4..end + 4 + length]).into(),
            });
        }
    }
}
pub const YAML:&str="proxies:\n  - name: Test node\n    type: socks5\n    server: 127.0.0.1\n    port: 1080\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [Test node]\nrules: ['MATCH,PROXY']\n";
