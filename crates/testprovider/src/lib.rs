//! A provider that speaks just enough of the OpenAI chat API for `veridoc`'s judge to
//! believe it: one `POST /chat/completions` in, one chat-completion envelope out. The judge
//! is a model, and the engine calls it over HTTP, so the tests that need a judge answer need
//! an endpoint rather than a script. Every request body is recorded so a test can assert what
//! the engine actually sent.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// The verdict each task is answered with, as the JSON object the model's text carries.
#[derive(Debug, Clone)]
pub struct Verdicts {
    pub s1: String,
    pub s2: String,
    pub s3: String,
}

impl Default for Verdicts {
    fn default() -> Self {
        Verdicts {
            s1: r#"{"unmentionedClaims": [], "unstatedAssertions": []}"#.to_string(),
            s2: r#"{"pairs": []}"#.to_string(),
            s3: r#"{"pairs": []}"#.to_string(),
        }
    }
}

/// A running mock provider. The listener thread outlives the handle on purpose: a test that
/// passes a base URL to a child process needs the server to stay up after the value that
/// started it is dropped.
pub struct Handle {
    pub base_url: String,
    requests: Arc<Mutex<VecDeque<String>>>,
}

impl Handle {
    /// The `judge.baseUrl` value to put in a config.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Every request body received so far, oldest first.
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("not poisoned")
            .clone()
            .into_iter()
            .collect()
    }

    /// The number of calls the run has made, for a test that must prove the model was
    /// consulted rather than the verdict cache replayed.
    pub fn calls(&self) -> usize {
        self.requests.lock().expect("not poisoned").len()
    }
}

/// Answer every task with its verdict from `verdicts`.
pub fn answering(verdicts: Verdicts) -> Handle {
    serve(move |body| {
        let content = if body.contains("S1 coverage judge") {
            verdicts.s1.clone()
        } else if body.contains("S3 focus judge") {
            verdicts.s3.clone()
        } else {
            verdicts.s2.clone()
        };
        Reply {
            status: 200,
            content,
        }
    })
}

/// Answer with the request body itself, so a test can read what the engine sent.
pub fn echoing() -> Handle {
    serve(|body| Reply {
        status: 200,
        content: body.to_string(),
    })
}

/// Answer every request with `status` and `body`, which is how a transport fault is
/// simulated: a provider that refuses, misconfigures, or returns prose.
pub fn failing(status: u16, body: &str) -> Handle {
    let body = body.to_string();
    serve(move |_| Reply {
        status,
        content: body.clone(),
    })
}

/// Accept the connection and never answer, which is what a reasoning model that spends its
/// whole budget thinking looks like from the client side.
pub fn silent() -> Handle {
    serve_silent()
}

struct Reply {
    status: u16,
    content: String,
}

fn serve(answer: impl Fn(&str) -> Reply + Send + 'static) -> Handle {
    let requests = Arc::new(Mutex::new(VecDeque::new()));
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let shared = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let Some(body) = read_request(&mut stream.try_clone().expect("a second handle")) else {
                continue;
            };
            shared.lock().expect("not poisoned").push_back(body.clone());
            let reply = answer(&body);
            let _ = write_response(stream, reply.status, &reply.content);
        }
    });
    Handle {
        base_url: format!("http://127.0.0.1:{port}"),
        requests,
    }
}

fn serve_silent() -> Handle {
    let requests = Arc::new(Mutex::new(VecDeque::new()));
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let shared = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            if let Some(body) = read_request(&mut stream) {
                shared.lock().expect("not poisoned").push_back(body);
            }
            // The answer never comes; the engine's own timeout ends the call.
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    });
    Handle {
        base_url: format!("http://127.0.0.1:{port}"),
        requests,
    }
}

/// Read one request and return its body. The engine sends `Content-Length`, so the body
/// boundary is known; a request without one is treated as empty.
fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        buffer.push(byte[0]);
        if buffer.len() > 1 << 20 {
            return None;
        }
    }
    let head = String::from_utf8_lossy(&buffer).to_lowercase();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

fn write_response(mut stream: TcpStream, status: u16, content: &str) -> std::io::Result<()> {
    let payload = if status == 200 {
        envelope(content)
    } else {
        content.to_string()
    };
    let reason = if status == 200 { "OK" } else { "Failure" };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        payload.len()
    )?;
    stream.write_all(payload.as_bytes())?;
    stream.flush()
}

/// A chat-completion envelope whose `content` is the verdict text.
fn envelope(content: &str) -> String {
    format!(
        "{{\"id\":\"mock\",\"object\":\"chat.completion\",\"created\":0,\"model\":\"mock\",\
         \"choices\":[{{\"index\":0,\"message\":{{\"role\":\"assistant\",\"content\":{},\
         \"reasoning_content\":\"\"}},\"finish_reason\":\"stop\"}}],\
         \"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}}}",
        serde_json::Value::from(content)
    )
}
