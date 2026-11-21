use std::time::Duration;

use crate::model::{DependencyDetails, Metadata, ProjectRequest};

pub const DEFAULT_URL: &str = "https://start.spring.io";
const METADATA_ACCEPT: &str = "application/vnd.initializr.v2.2+json";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("cannot reach {0}")]
    Connect(String),
    #[error("service returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("invalid response: {0}")]
    Decode(String),
}

impl ApiError {
    pub fn is_connectivity(&self) -> bool {
        matches!(self, ApiError::Connect(_))
    }
}

pub struct Client {
    base: String,
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn new(base_url: &str) -> Client {
        Client {
            base: base_url.trim_end_matches('/').to_string(),
            http: reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .build()
                .expect("static client configuration is valid"),
        }
    }

    pub fn metadata(&self) -> Result<(Metadata, String), ApiError> {
        let req = self
            .http
            .get(format!("{}/", self.base))
            .header(reqwest::header::ACCEPT, METADATA_ACCEPT)
            .timeout(Duration::from_secs(5));
        let raw = self.text(req)?;
        Ok((decode(&raw)?, raw))
    }

    pub fn dependencies(&self, boot: &str) -> Result<(DependencyDetails, String), ApiError> {
        let req = self
            .http
            .get(format!("{}/dependencies", self.base))
            .query(&[("bootVersion", boot)])
            .timeout(Duration::from_secs(5));
        let raw = self.text(req)?;
        Ok((decode(&raw)?, raw))
    }

    pub fn generate(&self, action: &str, req: &ProjectRequest) -> Result<Vec<u8>, ApiError> {
        let resp = self
            .http
            .get(format!("{}{action}", self.base))
            .query(&req.query_pairs())
            .timeout(Duration::from_secs(30))
            .send()
            .map_err(|e| self.send_error(e))?;
        let resp = check_status(resp)?;
        let bytes = resp.bytes().map_err(|e| self.send_error(e))?;
        Ok(bytes.to_vec())
    }

    fn text(&self, req: reqwest::blocking::RequestBuilder) -> Result<String, ApiError> {
        let resp = req.send().map_err(|e| self.send_error(e))?;
        check_status(resp)?.text().map_err(|e| self.send_error(e))
    }

    fn send_error(&self, e: reqwest::Error) -> ApiError {
        if e.is_connect() || e.is_timeout() {
            ApiError::Connect(self.base.clone())
        } else {
            ApiError::Decode(e.to_string())
        }
    }
}

fn check_status(
    resp: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, ApiError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    Err(ApiError::Status {
        status: status.as_u16(),
        body: resp.text().unwrap_or_default(),
    })
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, ApiError> {
    serde_json::from_str(raw).map_err(|e| ApiError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    const METADATA: &str = include_str!("../tests/fixtures/metadata.json");

    #[test]
    fn metadata_sends_accept_header() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/")
            .match_header("accept", METADATA_ACCEPT)
            .with_body(METADATA)
            .create();
        let (meta, raw) = Client::new(&server.url()).metadata().expect("ok");
        m.assert();
        assert_eq!(raw, METADATA);
        assert_eq!(meta.boot_version.default.as_deref(), Some("4.2.0"));
    }

    #[test]
    fn status_error_is_not_connectivity() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/dependencies")
            .match_query(Matcher::UrlEncoded("bootVersion".into(), "3.5.0".into()))
            .with_status(400)
            .with_body("bad version")
            .create();
        let err = Client::new(&server.url())
            .dependencies("3.5.0")
            .expect_err("400");
        assert!(
            matches!(err, ApiError::Status { status: 400, .. }),
            "{err:?}"
        );
        assert!(!err.is_connectivity());
    }

    #[test]
    fn unreachable_is_connectivity() {
        let err = Client::new("http://127.0.0.1:9")
            .metadata()
            .expect_err("nothing listens on port 9");
        assert!(err.is_connectivity(), "{err:?}");
    }

    #[test]
    fn generate_hits_action_with_query() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/starter.zip")
            .match_query(Matcher::UrlEncoded("baseDir".into(), "demo".into()))
            .with_body(b"PK")
            .create();
        let req = ProjectRequest {
            artifact_id: "demo".into(),
            ..Default::default()
        };
        let bytes = Client::new(&server.url())
            .generate("/starter.zip", &req)
            .expect("ok");
        assert_eq!(bytes, b"PK");
    }
}
