// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use dotenv::EnvLoader;
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::StatusCode;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use regex::Regex;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;
use tokio::net::TcpListener;
use url::Url;

async fn get_file_contents(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(tokio::fs::read_to_string(path).await?)
}

fn create_json_response(
    value: &Value,
    status_code: StatusCode,
    location: Option<&str>,
) -> Response<Full<Bytes>> {
    let json = serde_json::to_string(value).unwrap();

    let mut response_builder = Response::builder()
        .status(status_code)
        .header("Content-Type", "application/json");

    if let Some(location) = location {
        response_builder = response_builder.header("Location", location);
    }

    response_builder.body(Full::new(Bytes::from(json))).unwrap()
}

async fn get(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    if req.method() != hyper::Method::GET {
        return Ok(create_json_response(
            &json!({
                "error": "invalid method",
            }),
            StatusCode::METHOD_NOT_ALLOWED,
            None,
        ));
    }
    let data_dir = env::var("DATA_DIR").unwrap_or("data".to_string());
    let path = req.uri().path();
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        let url = env::var("DEFAULT_URL").unwrap_or("https://www.menhera.org/".to_string());
        let url = url.as_str();
        return Ok(create_json_response(
            &json!({
                "error": Value::Null,
                "url": url,
            }),
            StatusCode::MOVED_PERMANENTLY,
            Some(url),
        ));
    }
    let re = Regex::new(r"^[a-zA-Z0-9_]+([-.][a-zA-Z0-9_]+)*$").unwrap();
    if !re.is_match(path) {
        return Ok(create_json_response(
            &json!({
                "error": "invalid path",
            }),
            StatusCode::BAD_REQUEST,
            None,
        ));
    }
    let path = format!("{}/{}", data_dir, path);
    let path = std::path::Path::new(&path);

    if path.is_file() {
        if let Ok(contents) = get_file_contents(path).await {
            let contents = contents.trim();
            if let Ok(url) = Url::parse(contents) {
                let url = url.as_str();
                return Ok(create_json_response(
                    &json!({
                        "error": Value::Null,
                        "url": url,
                    }),
                    StatusCode::MOVED_PERMANENTLY,
                    Some(url),
                ));
            }
        }
    }

    Ok(create_json_response(
        &json!({
            "error": "not found",
        }),
        StatusCode::NOT_FOUND,
        None,
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Preserve dotenvy's search for .env in the current directory and its parents.
    if let Ok(directory) = env::current_dir() {
        if let Some(path) = directory
            .ancestors()
            .map(|parent| parent.join(".env"))
            .find(|path| path.is_file())
        {
            // SAFETY: This runs before the Tokio runtime or any other threads are started.
            unsafe {
                EnvLoader::with_path(path)
                    .substitution(true)
                    .load_and_modify()
            }
            .ok();
        }
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let addr_string = env::var("LISTEN_ADDR").unwrap_or("".to_string());
    let addr =
        SocketAddr::from_str(&addr_string).unwrap_or(SocketAddr::from(([127, 0, 0, 1], 8080)));

    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) = Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service_fn(get))
                .await
            {
                eprintln!("connection error: {error}");
            }
        });
    }
}
