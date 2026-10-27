//! 動作確認用。
//!
//! ```text
//! cargo run --example download -- <pair> <year> [month] [output_dir]
//! cargo run --example download -- eurusd 2019
//! cargo run --example download -- eurusd 2026 9 data
//! ```

use histdata::{Downloader, Period, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [pair, year, rest @ ..] = &args[..] else {
        eprintln!("usage: download <pair> <year> [month] [output_dir]");
        std::process::exit(2);
    };
    let year: i32 = year.parse()?;
    let period = match rest.first() {
        Some(month) => Period::Month {
            year,
            month: month.parse()?,
        },
        None => Period::Year(year),
    };
    let output_dir = rest.get(1).map(String::as_str).unwrap_or("output");

    let path = Downloader::new(output_dir)
        .download(&Request::new(pair.as_str(), period))
        .await?;
    println!("{}", path.display());
    Ok(())
}
