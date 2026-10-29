//! Embedded Web Console Asset Management

use rust_embed::RustEmbed;
use axum::{
    response::{IntoResponse, Response},
    http::{header, StatusCode, Uri},
    body::Body,
};

#[derive(RustEmbed)]
#[folder = "../public/"]
pub struct ConsoleAssets;

pub async fn console_handler(uri: Uri) -> Response {
    let raw_path = uri.path();
    let mut path = raw_path
        .trim_start_matches("/console")
        .trim_start_matches('/')
        .to_string();

    if path.starts_with("api/") {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"error":{"message":"API endpoint not found"}}"#))
            .unwrap();
    }

    if path.is_empty() || path == "index.html" {
        path = "index.html".to_string();
    }

    match ConsoleAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // Serve index.html for root or unknown sub-routes
            if let Some(content) = ConsoleAssets::get("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Body::from(content.data))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))
                    .unwrap()
            }
        }
    }
}
