//! 既知の通貨ペア一覧。crate 直下の `pairs.csv` をビルド時に埋め込む。
//!
//! ペアを追加するときは `pairs.csv` に 1 行足して再ビルドする。

use std::io::Read;
use std::sync::LazyLock;

use crate::Pair;

/// `pairs.csv` の 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairInfo {
    /// 表示名（例: `EUR/USD`）
    pub name: String,
    pub pair: Pair,
    /// 取引データの開始年
    pub first_year: i32,
    /// 取引データの開始月（1〜12）
    pub first_month: u8,
}

#[derive(Debug, thiserror::Error)]
pub enum PairsCsvError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("pairs.csv の {line} 行目が不正です: {reason}")]
    Malformed { line: usize, reason: &'static str },
}

const EMBEDDED: &str = include_str!("../pairs.csv");

static PAIRS: LazyLock<Vec<PairInfo>> = LazyLock::new(|| {
    parse_pairs_csv(EMBEDDED.as_bytes()).expect("埋め込みの pairs.csv が不正です")
});

/// 埋め込みの通貨ペア一覧。
pub fn pairs() -> &'static [PairInfo] {
    &PAIRS
}

/// 埋め込みの一覧から通貨ペアを探す。
pub fn pair_info(pair: &Pair) -> Option<&'static PairInfo> {
    pairs().iter().find(|info| &info.pair == pair)
}

/// `currency_pair_name,currency_pair_code,history_first_trading_month` 形式の CSV を読む。
/// 1 行目はヘッダとして読み飛ばし、空行は無視する。
pub fn parse_pairs_csv(mut reader: impl Read) -> Result<Vec<PairInfo>, PairsCsvError> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;

    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate().skip(1) {
        let line_no = i + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let malformed = |reason| PairsCsvError::Malformed {
            line: line_no,
            reason,
        };

        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let [name, code, first] = fields[..] else {
            return Err(malformed("列数が 3 ではありません"));
        };
        if code.is_empty() {
            return Err(malformed("通貨ペアのコードが空です"));
        }
        if first.len() != 6 || !first.bytes().all(|b| b.is_ascii_digit()) {
            return Err(malformed("開始年月は YYYYMM 形式で指定してください"));
        }
        let first_year: i32 = first[..4].parse().unwrap();
        let first_month: u8 = first[4..].parse().unwrap();
        if !(1..=12).contains(&first_month) {
            return Err(malformed("開始月が 1〜12 ではありません"));
        }
        out.push(PairInfo {
            name: name.to_string(),
            pair: Pair::new(code),
            first_year,
            first_month,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_csv_parses() {
        assert_eq!(pairs().len(), 66);
        let eurusd = pair_info(&Pair::new("EURUSD")).unwrap();
        assert_eq!(eurusd.name, "EUR/USD");
        assert_eq!((eurusd.first_year, eurusd.first_month), (2000, 5));
    }

    #[test]
    fn rejects_malformed_rows() {
        let csv = "h1,h2,h3\nEUR/USD,eurusd,200005\nbad,row\n";
        assert!(matches!(
            parse_pairs_csv(csv.as_bytes()),
            Err(PairsCsvError::Malformed { line: 3, .. })
        ));
        let csv = "h1,h2,h3\nEUR/USD,eurusd,200013\n";
        assert!(parse_pairs_csv(csv.as_bytes()).is_err());
    }
}
