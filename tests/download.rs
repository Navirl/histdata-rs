use histdata_rs::{Downloader, Error, Period, Platform, Request, TimeFrame};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request as MockRequest, ResponseTemplate};

const PAGE_PATH: &str = "/download-free-forex-historical-data/";
const ZIP_BODY: &[u8] = b"PK\x03\x04dummy zip body";

fn page_query(query: &'static str) -> impl Fn(&MockRequest) -> bool {
    move |req: &MockRequest| req.url.query() == Some(query)
}

async fn mount_page(server: &MockServer, query: &'static str, html: &str) {
    Mock::given(method("GET"))
        .and(path(PAGE_PATH))
        .and(page_query(query))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(server)
        .await;
}

const TOKEN_PAGE: &str = r#"<html><form id="file_down"><input type="hidden" name="tk" id="tk" value="tok123"></form></html>"#;

#[tokio::test]
async fn downloads_yearly_m1() {
    let server = MockServer::start().await;
    mount_page(
        &server,
        "/ascii/1-minute-bar-quotes/eurusd/2019",
        TOKEN_PAGE,
    )
    .await;
    let referer = format!(
        "{}{PAGE_PATH}?/ascii/1-minute-bar-quotes/eurusd/2019",
        server.uri()
    );
    Mock::given(method("POST"))
        .and(path("/get.php"))
        .and(header("referer", referer.as_str()))
        .and(header("origin", server.uri().as_str()))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string_contains(
            "tk=tok123&date=2019&datemonth=2019&platform=ASCII&timeframe=M1&fxpair=EURUSD",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(ZIP_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("nested");
    let downloader = Downloader::new(&out)
        .with_base_url(server.uri())
        .with_current_year(2026);

    let path = downloader
        .download(&Request::new("EURUSD", Period::Year(2019)))
        .await
        .unwrap();
    assert_eq!(path, out.join("DAT_ASCII_EURUSD_M1_2019.zip"));
    assert_eq!(std::fs::read(&path).unwrap(), ZIP_BODY);
    assert!(!out.join("DAT_ASCII_EURUSD_M1_2019.zip.part").exists());
}

#[tokio::test]
async fn downloads_monthly_tick_with_zero_padded_datemonth() {
    let server = MockServer::start().await;
    mount_page(
        &server,
        "/ninjatrader/tick-bid-quotes/usdjpy/2019/6",
        TOKEN_PAGE,
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/get.php"))
        .and(body_string_contains(
            "date=2019&datemonth=201906&platform=NT&timeframe=T_BID&fxpair=USDJPY",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(ZIP_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let request = Request::new(
        "usdjpy",
        Period::Month {
            year: 2019,
            month: 6,
        },
    )
    .time_frame(TimeFrame::TickBid)
    .platform(Platform::NinjaTrader);
    let path = Downloader::new(dir.path())
        .with_base_url(server.uri())
        .with_current_year(2026)
        .download(&request)
        .await
        .unwrap();
    assert_eq!(path.file_name().unwrap(), "DAT_NT_USDJPY_T_BID_201906.zip");
}

#[tokio::test]
async fn skips_existing_file_without_network() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("DAT_ASCII_EURUSD_M1_2019.zip");
    std::fs::write(&existing, b"already here").unwrap();

    let path = Downloader::new(dir.path())
        .with_base_url(server.uri())
        .download(&Request::new("eurusd", Period::Year(2019)))
        .await
        .unwrap();
    assert_eq!(path, existing);
    assert_eq!(std::fs::read(&path).unwrap(), b"already here");
}

#[tokio::test]
async fn rejects_invalid_requests_before_network() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(dir.path())
        .with_base_url(server.uri())
        .with_current_year(2026);

    let err = downloader
        .download(&Request::new("eurusd", Period::Year(2026)))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidPeriod(_)), "{err:?}");

    let err = downloader
        .download(&Request::new("eurusd", Period::Year(2019)).time_frame(TimeFrame::TickAsk))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidCombination { .. }), "{err:?}");
}

#[tokio::test]
async fn reports_site_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(page_query("/ascii/1-minute-bar-quotes/eurusd/2018"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    mount_page(
        &server,
        "/ascii/1-minute-bar-quotes/eurusd/2017",
        "<html>no token</html>",
    )
    .await;
    mount_page(
        &server,
        "/ascii/1-minute-bar-quotes/eurusd/2016",
        TOKEN_PAGE,
    )
    .await;
    mount_page(
        &server,
        "/ascii/1-minute-bar-quotes/eurusd/2015",
        TOKEN_PAGE,
    )
    .await;
    Mock::given(method("POST"))
        .and(body_string_contains("date=2016"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_string_contains("date=2015"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>error</html>"))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(dir.path())
        .with_base_url(server.uri())
        .with_current_year(2026);
    let downloader = &downloader;
    let download = |year| async move {
        downloader
            .download(&Request::new("eurusd", Period::Year(year)))
            .await
    };

    assert!(matches!(
        download(2018).await,
        Err(Error::SiteUnavailable { status }) if status.as_u16() == 503
    ));
    assert!(matches!(download(2017).await, Err(Error::TokenNotFound)));
    assert!(matches!(download(2016).await, Err(Error::EmptyResponse)));
    assert!(matches!(download(2015).await, Err(Error::NotZip)));

    // 失敗したときは何も残さない
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// 実サイトに接続する。`cargo test -- --ignored` で実行。
#[tokio::test]
#[ignore]
async fn live_download_from_histdata() {
    let dir = tempfile::tempdir().unwrap();
    let path = Downloader::new(dir.path())
        .download(&Request::new("eurusd", Period::Year(2019)))
        .await
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"PK\x03\x04"));
    assert!(bytes.len() > 100_000, "size = {}", bytes.len());
}
