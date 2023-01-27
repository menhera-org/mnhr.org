// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::env;
use std::path::Path;
use dotenvy::dotenv;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::str::FromStr;
use hyper::server::conn::AddrStream;
use hyper::{Body, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};
use serde_json::{json, Value};
use hyper::StatusCode;
use tokio::io::AsyncReadExt;
use url::Url;
use regex::Regex;

async fn get_file_contents(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await?;
    let contents = String::from_utf8(contents)?;
    Ok(contents)
}

fn create_json_response(value: &Value, status_code: StatusCode, location: Option<&str>) -> Response<Body> {
    let json = serde_json::to_string(value).unwrap();
    
    let mut response_builder = Response::builder()
        .status(status_code)
        .header("Content-Type", "application/json");
    
    if let Some(location) = location {
        response_builder = response_builder.header("Location", location);
    }

    response_builder
        .body(Body::from(json))
        .unwrap()
}

async fn get(req: Request<Body>, _addr: SocketAddr) -> Result<Response<Body>, Infallible> {
    if req.method() != hyper::Method::GET {
        return Ok(create_json_response(&json!({
            "error": "invalid method",
        }), 405.try_into().unwrap(), None));
    }
    let data_dir = env::var("DATA_DIR").unwrap_or("data".to_string());
    let path = req.uri().path();
    let path = path.trim_start_matches('/');
    let re = Regex::new(r"^[a-zA-Z0-9_]+([-.][a-zA-Z0-9_]+)*$").unwrap();
    if re.is_match(path) == false {
        return Ok(create_json_response(&json!({
            "error": "invalid path",
        }), 400.try_into().unwrap(), None));
    }
    let path = format!("{}/{}", data_dir, path);
    let path = std::path::Path::new(&path);

    if path.is_file() {
        if let Ok(contents) = get_file_contents(path).await {
            let contents = contents.trim();
            if let Ok(url) = Url::parse(contents) {
                let url = url.as_str();
                return Ok(create_json_response(&json!({
                    "error": Value::Null,
                    "url": url,
                }), 301.try_into().unwrap(), Some(url)));
            }
        }
    }

    Ok(create_json_response(&json!({
        "error": "not found",
    }), 404.try_into().unwrap(), None))
}

#[tokio::main]
async fn main() {
    dotenv().ok(); // ignore if .env file is not present

    let addr_string = env::var("LISTEN_ADDR").unwrap_or("".to_string());
    let addr = SocketAddr::from_str(&addr_string).unwrap_or(SocketAddr::from(([127, 0, 0, 1], 8080)));

    let make_svc = make_service_fn(move |conn: &AddrStream| {
        let addr = conn.remote_addr();
        async move {
            let addr = addr.clone();
            Ok::<_, Infallible>(service_fn(move |req : Request<Body>| {
                get(req, addr)
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    // Run this server for... forever!
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
