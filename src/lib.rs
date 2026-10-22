//! [HistData.com](https://www.histdata.com) から FX のヒストリカルデータ（ZIP）を取得する。
//!
//! Python の [`histdata`](https://github.com/philipperemy/FX-1-Minute-Data) の Rust 移植。
//! 保存されるファイル名と HTTP のやり取りは GitHub master 版と同じ。
//!
//! ```no_run
//! use histdata::{Downloader, Period, Request};
//!
//! # async fn run() -> histdata::Result<()> {
//! let downloader = Downloader::new("data");
//! // 過去年の 1 分足は年単位、今年の 1 分足は月単位で取得する
//! let path = downloader.download(&Request::new("eurusd", Period::Year(2019))).await?;
//! println!("{}", path.display()); // data/DAT_ASCII_EURUSD_M1_2019.zip
//! # Ok(())
//! # }
//! ```

mod download;
mod error;
pub mod pairs;
mod types;

pub use download::{DEFAULT_BASE_URL, Downloader};
pub use error::{Error, Result};
pub use pairs::{PairInfo, pair_info, pairs};
pub use types::{
    Pair, Period, PeriodRule, Platform, Request, TimeFrame, period_rule, period_rule_at,
    referer_path,
};
