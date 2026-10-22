use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use reqwest::header::{self, HeaderMap, HeaderValue};
use tokio::io::AsyncWriteExt;

use crate::types::current_year;
use crate::{Error, Request, Result};

pub const DEFAULT_BASE_URL: &str = "https://www.histdata.com";

const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// HistData.com からデータの ZIP を取得して保存する。
#[derive(Debug, Clone)]
pub struct Downloader {
    client: reqwest::Client,
    base_url: String,
    output_dir: PathBuf,
    current_year: Option<i32>,
}

impl Downloader {
    /// `output_dir` に保存する Downloader を作る。
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("histdata-rs/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest::Client の初期化に失敗しました");
        Downloader {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            output_dir: output_dir.into(),
            current_year: None,
        }
    }

    /// 使う HTTP クライアントを差し替える。
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    /// 接続先を差し替える（末尾の `/` は不要）。主にテスト用。
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// 年・月の検証で使う「今年」を固定する。既定は UTC の現在年。
    pub fn with_current_year(mut self, year: i32) -> Self {
        self.current_year = Some(year);
        self
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// `request` の保存先パス。
    pub fn output_path(&self, request: &Request) -> PathBuf {
        self.output_dir.join(request.file_name())
    }

    /// `request` のデータを取得して保存し、ZIP のパスを返す。
    ///
    /// 保存先に同名のファイルがあればダウンロードせずにそのパスを返す。
    pub async fn download(&self, request: &Request) -> Result<PathBuf> {
        let output_path = self.output_path(request);
        if tokio::fs::try_exists(&output_path).await? {
            tracing::info!(path = %output_path.display(), "既に存在するためダウンロードを省略します");
            return Ok(output_path);
        }

        request.validate_period(self.current_year.unwrap_or_else(current_year))?;
        let referer = request.referer(&self.base_url)?;

        // URL は reqwest に正規化させ、以降は ASCII に直った形をヘッダにも使う
        let page_request = self.client.get(&referer).build()?;
        let referer = page_request.url().clone();
        tracing::debug!(%referer, "ダウンロードページを取得します");
        let page = self.client.execute(page_request).await?;
        if page.status() != reqwest::StatusCode::OK {
            return Err(Error::SiteUnavailable {
                status: page.status(),
            });
        }
        let token = extract_token(&page.text().await?).ok_or(Error::TokenNotFound)?;

        let date_month = request.date_month();
        let year = request.period.year().to_string();
        let fx_pair = request.pair.to_upper();
        let form = [
            ("tk", token.as_str()),
            ("date", year.as_str()),
            ("datemonth", date_month.as_str()),
            ("platform", request.platform.code()),
            ("timeframe", request.time_frame.code()),
            ("fxpair", fx_pair.as_str()),
        ];
        tracing::debug!(?form, "get.php に POST します");
        let response = self
            .client
            .post(format!("{}/get.php", self.base_url))
            .headers(post_headers(&referer))
            .form(&form)
            .send()
            .await?;

        tokio::fs::create_dir_all(&self.output_dir).await?;
        let part_path = output_path.with_extension("zip.part");
        match save_zip(response, &part_path).await {
            Ok(()) => {
                tokio::fs::rename(&part_path, &output_path).await?;
                tracing::info!(path = %output_path.display(), "保存しました");
                Ok(output_path)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                Err(e)
            }
        }
    }
}

/// Python 版と同じヘッダ。Referer が無いと get.php はデータを返さない。
fn post_headers(referer: &reqwest::Url) -> HeaderMap {
    // Url の文字列表現は ASCII のみなので HeaderValue に必ず変換できる
    let value = |s: &str| HeaderValue::from_str(s).expect("URL は ASCII のみ");
    let mut headers = HeaderMap::new();
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("max-age=0"));
    headers.insert(
        header::ORIGIN,
        value(&referer.origin().ascii_serialization()),
    );
    headers.insert(
        header::UPGRADE_INSECURE_REQUESTS,
        HeaderValue::from_static("1"),
    );
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
        ),
    );
    headers.insert(header::REFERER, value(referer.as_str()));
    headers
}

/// ダウンロードページの `<input id="tk">` の値。無いか空なら `None`。
fn extract_token(html: &str) -> Option<String> {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("input#tk").unwrap();
    document
        .select(&selector)
        .next()?
        .value()
        .attr("value")
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 応答本体を `path` に書き出す。先頭が ZIP のシグネチャでなければ書き出す前にエラーにする。
async fn save_zip(response: reqwest::Response, path: &Path) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut head = Vec::with_capacity(ZIP_MAGIC.len());
    let mut file = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let file = match &mut file {
            Some(file) => file,
            None => {
                head.extend_from_slice(&chunk);
                if head.len() < ZIP_MAGIC.len() {
                    continue;
                }
                if !head.starts_with(ZIP_MAGIC) {
                    return Err(Error::NotZip);
                }
                let mut f = tokio::fs::File::create(path).await?;
                f.write_all(&head).await?;
                file = Some(f);
                continue;
            }
        };
        file.write_all(&chunk).await?;
    }

    match file {
        Some(mut file) => {
            file.flush().await?;
            Ok(())
        }
        None if head.is_empty() => Err(Error::EmptyResponse),
        None => Err(Error::NotZip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_token() {
        let html = r#"<form id="file_down"><input type="hidden" name="tk" id="tk" value="abc123"/></form>"#;
        assert_eq!(extract_token(html).as_deref(), Some("abc123"));
        assert_eq!(extract_token(r#"<input id="tk" value="">"#), None);
        assert_eq!(extract_token("<html></html>"), None);
    }
}
