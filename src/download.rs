use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{self, HeaderMap, HeaderValue};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::types::current_year;
use crate::{Error, Request, Result};

pub const DEFAULT_BASE_URL: &str = "https://www.histdata.com";

/// 失敗したときに再試行する回数の既定値。
pub const DEFAULT_RETRIES: u32 = 3;

/// 最初の再試行までの待ち時間の既定値。再試行のたびに倍になる。
pub const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);

/// サイトに取りに行く間隔の既定値。
pub const DEFAULT_REQUEST_INTERVAL: Duration = Duration::from_secs(1);

const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// [`Downloader::download_all`] の進捗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// 取得し終えたファイルの数
    pub done: usize,
    pub total: usize,
    /// これから取得するファイルの名前。すべて終わったときは空
    pub file_name: String,
}

/// HistData.com からデータの ZIP を取得して保存する。
///
/// サイトに負荷をかけないよう、取りに行く間隔を空け（[`with_request_interval`](Self::with_request_interval)）、
/// 通信エラーや 429・5xx は間隔を空けて再試行する（[`with_retries`](Self::with_retries)）。
/// 間隔は clone した Downloader 同士で共有する。
#[derive(Debug, Clone)]
pub struct Downloader {
    client: reqwest::Client,
    base_url: String,
    output_dir: PathBuf,
    current_year: Option<i32>,
    retries: u32,
    retry_delay: Duration,
    request_interval: Duration,
    /// 最後にサイトへ取りに行き始めた時刻
    last_fetch: Arc<Mutex<Option<Instant>>>,
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
            retries: DEFAULT_RETRIES,
            retry_delay: DEFAULT_RETRY_DELAY,
            request_interval: DEFAULT_REQUEST_INTERVAL,
            last_fetch: Arc::default(),
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

    /// 通信エラーや 429・5xx で失敗したときに再試行する回数。既定は [`DEFAULT_RETRIES`]。
    pub fn with_retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    /// 最初の再試行までの待ち時間。再試行のたびに倍になる。既定は [`DEFAULT_RETRY_DELAY`]。
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    /// サイトに取りに行き始めてから次に取りに行くまでの最短の間隔。保存済みのファイルは数えない。
    /// 既定は [`DEFAULT_REQUEST_INTERVAL`]。
    pub fn with_request_interval(mut self, interval: Duration) -> Self {
        self.request_interval = interval;
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

        let mut delay = self.retry_delay;
        for attempt in 0.. {
            self.wait_for_turn().await;
            match self.fetch(request, &output_path).await {
                Err(e) if attempt < self.retries && e.is_retryable() => {
                    tracing::warn!(error = %e, ?delay, "失敗したので間隔を空けて再試行します");
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2);
                }
                result => return result,
            }
        }
        unreachable!()
    }

    /// `requests` を順に取得して保存し、ZIP のパスを並びどおりに返す。1 件でも失敗したらそこで止める。
    ///
    /// `progress` は各ファイルを取得する前と、すべて終わったときに呼ぶ。
    pub async fn download_all(
        &self,
        requests: &[Request],
        mut progress: impl FnMut(Progress),
    ) -> Result<Vec<PathBuf>> {
        let total = requests.len();
        let mut paths = Vec::with_capacity(total);
        for (done, request) in requests.iter().enumerate() {
            progress(Progress {
                done,
                total,
                file_name: request.file_name(),
            });
            paths.push(self.download(request).await?);
        }
        progress(Progress {
            done: total,
            total,
            file_name: String::new(),
        });
        Ok(paths)
    }

    /// 前に取りに行き始めてから `request_interval` 経つまで待つ。同時に呼ばれたら 1 つずつ通す
    async fn wait_for_turn(&self) {
        let mut last = self.last_fetch.lock().await;
        if let Some(last) = *last {
            tokio::time::sleep_until(last + self.request_interval).await;
        }
        *last = Some(Instant::now());
    }

    /// ページのトークンを取って `get.php` に POST し、`output_path` に保存する（1 回分）
    async fn fetch(&self, request: &Request, output_path: &Path) -> Result<PathBuf> {
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
        let status = response.status();
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(Error::SiteUnavailable { status });
        }

        tokio::fs::create_dir_all(&self.output_dir).await?;
        let part_path = output_path.with_extension("zip.part");
        match save_zip(response, &part_path).await {
            Ok(()) => {
                tokio::fs::rename(&part_path, output_path).await?;
                tracing::info!(path = %output_path.display(), "保存しました");
                Ok(output_path.to_path_buf())
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
