//! HTTP requests: client setup, the first request, and ranged requests.

use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_RANGE, ETAG, HeaderMap, HeaderValue,
    LAST_MODIFIED, RANGE,
};
use reqwest::{Client, Response, StatusCode};
use url::Url;

use crate::download::DownloadOptions;
use crate::error::{Error, Result};
use crate::naming;
use crate::segment::UNKNOWN_END;

/// Values that change when the file on the server changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validators {
    fn from_headers(headers: &HeaderMap) -> Self {
        let text = |name| {
            headers
                .get(name)
                .and_then(|v: &HeaderValue| v.to_str().ok())
                .map(str::to_string)
        };
        Self {
            etag: text(ETAG),
            last_modified: text(LAST_MODIFIED),
        }
    }
}

/// Builds a client. Each worker gets its own, so each worker gets its own TCP
/// connection: HTTP/2 would otherwise multiplex every range request over one
/// connection and one congestion window, which removes the point of splitting.
pub fn build_client(opts: &DownloadOptions) -> Result<Client> {
    let mut headers = opts.headers.clone();
    // Compressed transfer encodings break byte offsets.
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    Ok(Client::builder()
        .user_agent(opts.user_agent.as_str())
        .default_headers(headers)
        .connect_timeout(opts.connect_timeout)
        .tcp_nodelay(true)
        .pool_max_idle_per_host(1)
        .build()?)
}

/// What the first response says about the file.
pub struct Probe {
    pub response: Response,
    /// URL after redirects; later requests go straight here.
    pub final_url: Url,
    pub ranged: bool,
    pub total: Option<u64>,
    pub validators: Validators,
    pub file_name: Option<String>,
}

/// Sends `GET` with `Range: bytes=0-`. A `206` proves ranges work and gives the
/// size, and its body is already the start of the file, so no extra round trip
/// is spent on a separate `HEAD`.
pub async fn probe(client: &Client, url: &Url) -> Result<Probe> {
    let response = client
        .get(url.clone())
        .header(RANGE, "bytes=0-")
        .send()
        .await?;
    let status = response.status();
    let final_url = response.url().clone();
    let headers = response.headers();
    let validators = Validators::from_headers(headers);
    let file_name = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(naming::from_content_disposition);

    let (ranged, total) = match status {
        StatusCode::PARTIAL_CONTENT => match content_range(headers) {
            Some(ContentRange {
                start: 0,
                total: Some(total),
                ..
            }) => (true, Some(total)),
            Some(ContentRange {
                start: 0,
                total: None,
                ..
            }) => (false, None),
            other => {
                return Err(Error::RangeNotHonored {
                    expected: 0,
                    got: other.map(|c| c.start),
                });
            }
        },
        StatusCode::OK => (false, response.content_length()),
        // `bytes=0-` on an empty file.
        StatusCode::RANGE_NOT_SATISFIABLE
            if content_range(headers).and_then(|c| c.total) == Some(0) =>
        {
            (false, Some(0))
        }
        status => {
            return Err(Error::HttpStatus {
                status: status.as_u16(),
                url: final_url.to_string(),
            });
        }
    };

    Ok(Probe {
        response,
        final_url,
        ranged,
        total,
        validators,
        file_name,
    })
}

/// Requests `[from, to)`. Without range support the whole body is requested and
/// the first `from` bytes must be skipped; the returned number says how many.
pub async fn fetch(
    client: &Client,
    url: &Url,
    from: u64,
    to: u64,
    ranged: bool,
    expected_total: Option<u64>,
) -> Result<(Response, u64)> {
    if !ranged {
        let response = client.get(url.clone()).send().await?;
        check_status(&response, StatusCode::OK)?;
        return Ok((response, from));
    }

    let range = if to == UNKNOWN_END {
        format!("bytes={from}-")
    } else {
        format!("bytes={from}-{}", to - 1)
    };
    let response = client.get(url.clone()).header(RANGE, range).send().await?;
    if response.status() == StatusCode::OK {
        return Err(Error::RangeNotHonored {
            expected: from,
            got: None,
        });
    }
    check_status(&response, StatusCode::PARTIAL_CONTENT)?;
    let got = content_range(response.headers());
    if got.map(|c| c.start) != Some(from) {
        return Err(Error::RangeNotHonored {
            expected: from,
            got: got.map(|c| c.start),
        });
    }
    if let (Some(expected), Some(actual)) = (expected_total, got.and_then(|c| c.total))
        && expected != actual
    {
        return Err(Error::RemoteChanged);
    }
    Ok((response, 0))
}

fn check_status(response: &Response, want: StatusCode) -> Result<()> {
    if response.status() == want {
        Ok(())
    } else {
        Err(Error::HttpStatus {
            status: response.status().as_u16(),
            url: response.url().to_string(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end_inclusive: u64,
    pub total: Option<u64>,
}

fn content_range(headers: &HeaderMap) -> Option<ContentRange> {
    parse_content_range(headers.get(CONTENT_RANGE)?.to_str().ok()?)
}

/// Parses `bytes 0-99/1000`, `bytes 0-99/*` and `bytes */1000`.
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    if span.trim() == "*" {
        return Some(ContentRange {
            start: 0,
            end_inclusive: 0,
            total,
        });
    }
    let (start, end) = span.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end_inclusive = end.trim().parse().ok()?;
    (start <= end_inclusive).then_some(ContentRange {
        start,
        end_inclusive,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_forms() {
        assert_eq!(
            parse_content_range("bytes 0-99/1000"),
            Some(ContentRange {
                start: 0,
                end_inclusive: 99,
                total: Some(1000)
            })
        );
        assert_eq!(parse_content_range("bytes 5-9/*").unwrap().total, None);
        assert_eq!(parse_content_range("bytes */0").unwrap().total, Some(0));
        assert_eq!(parse_content_range("bytes 9-5/10"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
    }
}
