use std::fmt;
use std::str::FromStr;

use crate::{Error, Result};

/// 時間足。値は HistData.com のフォームで使われるコード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeFrame {
    /// 1 分足 (`M1`)
    OneMinute,
    /// ティック (`T`)。Generic ASCII のみ。
    Tick,
    /// ティック・約定値 (`T_LAST`)。NinjaTrader のみ。
    TickLast,
    /// ティック・Bid (`T_BID`)。NinjaTrader のみ。
    TickBid,
    /// ティック・Ask (`T_ASK`)。NinjaTrader のみ。
    TickAsk,
}

impl TimeFrame {
    pub const ALL: [TimeFrame; 5] = [
        TimeFrame::OneMinute,
        TimeFrame::Tick,
        TimeFrame::TickLast,
        TimeFrame::TickBid,
        TimeFrame::TickAsk,
    ];

    pub fn code(self) -> &'static str {
        match self {
            TimeFrame::OneMinute => "M1",
            TimeFrame::Tick => "T",
            TimeFrame::TickLast => "T_LAST",
            TimeFrame::TickBid => "T_BID",
            TimeFrame::TickAsk => "T_ASK",
        }
    }

    pub fn is_tick(self) -> bool {
        !matches!(self, TimeFrame::OneMinute)
    }
}

impl fmt::Display for TimeFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// 出力形式（プラットフォーム）。値は HistData.com のフォームで使われるコード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// MetaTrader (`MT`)
    MetaTrader,
    /// Generic ASCII (`ASCII`)
    GenericAscii,
    /// Excel (`XLSX`)
    Excel,
    /// NinjaTrader (`NT`)
    NinjaTrader,
    /// MetaStock (`MS`)
    MetaStock,
}

impl Platform {
    pub const ALL: [Platform; 5] = [
        Platform::MetaTrader,
        Platform::GenericAscii,
        Platform::Excel,
        Platform::NinjaTrader,
        Platform::MetaStock,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Platform::MetaTrader => "MT",
            Platform::GenericAscii => "ASCII",
            Platform::Excel => "XLSX",
            Platform::NinjaTrader => "NT",
            Platform::MetaStock => "MS",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// ダウンロードページのパス（`/download-free-forex-historical-data/?/` 以降）を返す。
/// HistData.com が提供する 9 通りの組み合わせ以外は [`Error::InvalidCombination`]。
pub fn referer_path(time_frame: TimeFrame, platform: Platform) -> Result<&'static str> {
    use Platform::*;
    use TimeFrame::*;
    Ok(match (time_frame, platform) {
        (Tick, GenericAscii) => "ascii/tick-data-quotes/",
        (TickLast, NinjaTrader) => "ninjatrader/tick-last-quotes/",
        (TickBid, NinjaTrader) => "ninjatrader/tick-bid-quotes/",
        (TickAsk, NinjaTrader) => "ninjatrader/tick-ask-quotes/",
        (OneMinute, GenericAscii) => "ascii/1-minute-bar-quotes/",
        (OneMinute, MetaTrader) => "metatrader/1-minute-bar-quotes/",
        (OneMinute, Excel) => "excel/1-minute-bar-quotes/",
        (OneMinute, NinjaTrader) => "ninjatrader/1-minute-bar-quotes/",
        (OneMinute, MetaStock) => "metastock/1-minute-bar-quotes/",
        _ => {
            return Err(Error::InvalidCombination {
                time_frame,
                platform,
            });
        }
    })
}

/// 通貨ペアのコード（例: `eurusd`）。小文字に正規化して保持する。
///
/// 一覧にないペアも受け付ける。既知のペアは [`crate::pairs`] を参照。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pair(String);

impl Pair {
    pub fn new(code: impl AsRef<str>) -> Self {
        Pair(code.as_ref().trim().to_ascii_lowercase())
    }

    /// 小文字のコード（URL で使う形）。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 大文字のコード（フォームとファイル名で使う形）。
    pub fn to_upper(&self) -> String {
        self.0.to_ascii_uppercase()
    }
}

impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Pair {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Pair::new(s))
    }
}

impl From<&str> for Pair {
    fn from(s: &str) -> Self {
        Pair::new(s)
    }
}

/// 取得する期間。HistData.com のファイルは年単位か月単位のどちらか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Period {
    Year(i32),
    /// `month` は 1〜12。
    Month {
        year: i32,
        month: u8,
    },
}

impl Period {
    pub fn year(self) -> i32 {
        match self {
            Period::Year(year) | Period::Month { year, .. } => year,
        }
    }

    pub fn month(self) -> Option<u8> {
        match self {
            Period::Year(_) => None,
            Period::Month { month, .. } => Some(month),
        }
    }
}

/// ある年のデータをどの単位で取得できるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodRule {
    /// 年単位のみ（1 分足の過去年）。
    Yearly,
    /// 月単位のみ（1 分足の今年以降）。
    Monthly,
    /// どちらでもよい（ティック）。
    Either,
}

impl PeriodRule {
    pub fn allows(self, period: Period) -> bool {
        matches!(
            (self, period),
            (PeriodRule::Either, _)
                | (PeriodRule::Yearly, Period::Year(_))
                | (PeriodRule::Monthly, Period::Month { .. })
        )
    }
}

/// `year` のデータを取得できる単位を、UTC の現在年を基準に返す。
pub fn period_rule(time_frame: TimeFrame, year: i32) -> PeriodRule {
    period_rule_at(time_frame, year, current_year())
}

/// [`period_rule`] の基準年を指定できる版。
pub fn period_rule_at(time_frame: TimeFrame, year: i32, current_year: i32) -> PeriodRule {
    if time_frame.is_tick() {
        PeriodRule::Either
    } else if year >= current_year {
        PeriodRule::Monthly
    } else {
        PeriodRule::Yearly
    }
}

pub(crate) fn current_year() -> i32 {
    use chrono::Datelike;
    chrono::Utc::now().year()
}

/// 1 回分のダウンロード指定。既定値は Python 版と同じく 1 分足・Generic ASCII。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Request {
    pub pair: Pair,
    pub period: Period,
    pub time_frame: TimeFrame,
    pub platform: Platform,
}

impl Request {
    pub fn new(pair: impl Into<Pair>, period: Period) -> Self {
        Request {
            pair: pair.into(),
            period,
            time_frame: TimeFrame::OneMinute,
            platform: Platform::GenericAscii,
        }
    }

    pub fn time_frame(mut self, time_frame: TimeFrame) -> Self {
        self.time_frame = time_frame;
        self
    }

    pub fn platform(mut self, platform: Platform) -> Self {
        self.platform = platform;
        self
    }

    /// 保存ファイル名。例: `DAT_ASCII_EURUSD_M1_2016.zip`, `DAT_ASCII_EURUSD_T_201906.zip`
    pub fn file_name(&self) -> String {
        format!(
            "DAT_{}_{}_{}_{}.zip",
            self.platform,
            self.pair.to_upper(),
            self.time_frame,
            self.date_month(),
        )
    }

    /// フォームの `datemonth`。月はゼロ埋め 2 桁。
    pub(crate) fn date_month(&self) -> String {
        match self.period {
            Period::Year(year) => year.to_string(),
            Period::Month { year, month } => format!("{year}{month:02}"),
        }
    }

    /// ダウンロードページの URL。月はゼロ埋めしない（Python 版と同じ）。
    pub(crate) fn referer(&self, base_url: &str) -> Result<String> {
        let path = referer_path(self.time_frame, self.platform)?;
        let tail = match self.period {
            Period::Year(year) => format!("{}/{year}", self.pair),
            Period::Month { year, month } => format!("{}/{year}/{month}", self.pair),
        };
        Ok(format!(
            "{base_url}/download-free-forex-historical-data/?/{path}{tail}"
        ))
    }

    pub(crate) fn validate_period(&self, current_year: i32) -> Result<()> {
        if let Some(month) = self.period.month()
            && !(1..=12).contains(&month)
        {
            return Err(Error::InvalidPeriod("月は 1〜12 で指定してください"));
        }
        match period_rule_at(self.time_frame, self.period.year(), current_year) {
            rule if rule.allows(self.period) => Ok(()),
            PeriodRule::Monthly => Err(Error::InvalidPeriod(
                "今年の 1 分足は月単位で指定してください（Period::Month）",
            )),
            _ => Err(Error::InvalidPeriod(
                "過去年の 1 分足は年単位で指定してください（Period::Year）",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://www.histdata.com";

    #[test]
    fn file_name_yearly_and_monthly() {
        let r = Request::new("eurusd", Period::Year(2016));
        assert_eq!(r.file_name(), "DAT_ASCII_EURUSD_M1_2016.zip");

        let r = Request::new(
            "EurUsd",
            Period::Month {
                year: 2019,
                month: 6,
            },
        )
        .time_frame(TimeFrame::Tick);
        assert_eq!(r.file_name(), "DAT_ASCII_EURUSD_T_201906.zip");

        let r = Request::new(
            "usdjpy",
            Period::Month {
                year: 2019,
                month: 11,
            },
        )
        .time_frame(TimeFrame::TickLast)
        .platform(Platform::NinjaTrader);
        assert_eq!(r.file_name(), "DAT_NT_USDJPY_T_LAST_201911.zip");
    }

    #[test]
    fn referer_month_is_not_zero_padded() {
        let r = Request::new(
            "EURUSD",
            Period::Month {
                year: 2019,
                month: 6,
            },
        );
        assert_eq!(
            r.referer(BASE).unwrap(),
            "https://www.histdata.com/download-free-forex-historical-data/?/ascii/1-minute-bar-quotes/eurusd/2019/6"
        );
        let r = Request::new("eurgbp", Period::Year(2016)).platform(Platform::MetaTrader);
        assert_eq!(
            r.referer(BASE).unwrap(),
            "https://www.histdata.com/download-free-forex-historical-data/?/metatrader/1-minute-bar-quotes/eurgbp/2016"
        );
    }

    #[test]
    fn exactly_nine_valid_combinations() {
        let valid: Vec<_> = TimeFrame::ALL
            .iter()
            .flat_map(|&tf| Platform::ALL.iter().map(move |&p| (tf, p)))
            .filter(|&(tf, p)| referer_path(tf, p).is_ok())
            .collect();
        assert_eq!(valid.len(), 9);
        assert!(matches!(
            referer_path(TimeFrame::Tick, Platform::MetaTrader),
            Err(Error::InvalidCombination { .. })
        ));
    }

    #[test]
    fn period_rules() {
        assert_eq!(
            period_rule_at(TimeFrame::OneMinute, 2020, 2026),
            PeriodRule::Yearly
        );
        assert_eq!(
            period_rule_at(TimeFrame::OneMinute, 2026, 2026),
            PeriodRule::Monthly
        );
        assert_eq!(
            period_rule_at(TimeFrame::OneMinute, 2027, 2026),
            PeriodRule::Monthly
        );
        assert_eq!(
            period_rule_at(TimeFrame::TickBid, 2020, 2026),
            PeriodRule::Either
        );
    }

    #[test]
    fn validate_period() {
        let past_month = Request::new(
            "eurusd",
            Period::Month {
                year: 2020,
                month: 1,
            },
        );
        assert!(matches!(
            past_month.validate_period(2026),
            Err(Error::InvalidPeriod(_))
        ));

        let this_year = Request::new("eurusd", Period::Year(2026));
        assert!(matches!(
            this_year.validate_period(2026),
            Err(Error::InvalidPeriod(_))
        ));

        let bad_month = Request::new(
            "eurusd",
            Period::Month {
                year: 2026,
                month: 13,
            },
        );
        assert!(matches!(
            bad_month.validate_period(2026),
            Err(Error::InvalidPeriod(_))
        ));

        let tick = Request::new(
            "eurusd",
            Period::Month {
                year: 2020,
                month: 1,
            },
        )
        .time_frame(TimeFrame::Tick);
        assert!(tick.validate_period(2026).is_ok());
        assert!(
            Request::new("eurusd", Period::Year(2020))
                .validate_period(2026)
                .is_ok()
        );
    }
}
