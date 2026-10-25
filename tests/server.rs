use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{Request, Version};
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde_json::{json, Value};
use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;

struct Server {
    child: Child,
    directory: PathBuf,
    addr: SocketAddr,
}

impl Server {
    async fn start(with_dotenv: bool) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "mnhr-org-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join("data")).unwrap();
        fs::create_dir(directory.join("working")).unwrap();
        fs::write(directory.join("data/link"), "  https://example.com  \n").unwrap();
        fs::write(directory.join("data/invalid"), "invalid URL").unwrap();
        fs::write(directory.join("data/invalid-utf8"), [0xff]).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_mnhr-org"));
        command
            .current_dir(directory.join("working"))
            .env_remove("LISTEN_ADDR")
            .env_remove("DATA_DIR")
            .env_remove("DEFAULT_URL");
        if with_dotenv {
            fs::write(
                directory.join(".env"),
                format!(
                    "LISTEN_ADDR={addr}\nDATA_DIR=../data\nBASE_URL=https://example.org\nDEFAULT_URL=${{BASE_URL}}/home\n"
                ),
            )
            .unwrap();
            // An inherited variable must take precedence over the .env file.
            command.env("BASE_URL", "https://override.example");
        } else {
            command
                .env("LISTEN_ADDR", addr.to_string())
                .env("DATA_DIR", "../data");
        }
        drop(listener);
        let child = command.spawn().unwrap();
        let mut server = Self {
            child,
            directory,
            addr,
        };
        for _ in 0..200 {
            assert!(server.child.try_wait().unwrap().is_none(), "server exited");
            if TcpStream::connect(addr).await.is_ok() {
                return server;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("server did not start");
    }

    async fn check(
        &self,
        version: Version,
        method: &str,
        path: &str,
        status: u16,
        body: Value,
        location: Option<&str>,
    ) {
        let io = TokioIo::new(TcpStream::connect(self.addr).await.unwrap());
        let request = Request::builder()
            .method(method)
            .uri(format!("http://{}{path}", self.addr))
            .version(version)
            .body(Full::new(Bytes::new()))
            .unwrap();
        let response = if version == Version::HTTP_2 {
            let (mut sender, connection) =
                http2::handshake(TokioExecutor::new(), io).await.unwrap();
            tokio::spawn(async move { connection.await.unwrap() });
            sender.send_request(request).await.unwrap()
        } else {
            let (mut sender, connection) = http1::handshake(io).await.unwrap();
            tokio::spawn(async move { connection.await.unwrap() });
            sender.send_request(request).await.unwrap()
        };
        assert_eq!(response.version(), version);
        assert_eq!(response.status(), status, "{method} {path}");
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(
            response
                .headers()
                .get("location")
                .map(|v| v.to_str().unwrap()),
            location
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), body);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[tokio::test]
async fn redirects_and_errors_over_http1_and_http2() {
    let server = Server::start(true).await;
    for version in [Version::HTTP_11, Version::HTTP_2] {
        for (path, url) in [
            ("/", "https://override.example/home"),
            ("/link?ignored=yes", "https://example.com/"),
        ] {
            server
                .check(
                    version,
                    "GET",
                    path,
                    301,
                    json!({"error": null, "url": url}),
                    Some(url),
                )
                .await;
        }
        for (method, path, status, error) in [
            ("POST", "/link", 405, "invalid method"),
            ("GET", "/../link", 400, "invalid path"),
            ("GET", "/nested/link", 400, "invalid path"),
            ("GET", "/missing", 404, "not found"),
            ("GET", "/invalid", 404, "not found"),
            ("GET", "/invalid-utf8", 404, "not found"),
        ] {
            server
                .check(version, method, path, status, json!({"error": error}), None)
                .await;
        }
    }
}

#[tokio::test]
async fn starts_without_dotenv_and_uses_default_url() {
    let server = Server::start(false).await;
    server
        .check(
            Version::HTTP_11,
            "GET",
            "/",
            301,
            json!({"error": null, "url": "https://www.menhera.org/"}),
            Some("https://www.menhera.org/"),
        )
        .await;
}
