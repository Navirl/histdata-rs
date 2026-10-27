use crate::{Platform, TimeFrame};

/// ダウンロード処理のエラー。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// HistData.com が提供していない時間足とプラットフォームの組み合わせ。
    #[error("無効な組み合わせです: time_frame={time_frame}, platform={platform}")]
    InvalidCombination {
        time_frame: TimeFrame,
        platform: Platform,
    },

    /// 年・月の指定が HistData.com の提供単位に合っていない。
    #[error("無効な期間指定です: {0}")]
    InvalidPeriod(&'static str),

    /// ダウンロードページの GET が 200 以外を返した。`get.php` が 429 か 5xx を返したときも。
    #[error("www.histdata.com から 200 以外の応答がありました (status={status})")]
    SiteUnavailable { status: reqwest::StatusCode },

    /// ページにトークン (`<input id="tk">`) が無い。年・月・通貨ペアに該当するデータが無いことが多い。
    #[error("トークンが見つかりません。年・月・通貨ペアが正しいか確認してください")]
    TokenNotFound,

    /// `get.php` の応答本体が空。
    #[error("データが見つかりません（応答が空です）")]
    EmptyResponse,

    /// `get.php` の応答が ZIP ではない。
    #[error("応答が ZIP ではありません")]
    NotZip,

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// 時間をおけば成功しうる失敗か（通信エラー、429、5xx）。
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::SiteUnavailable { status } => {
                status.is_server_error() || *status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }
            Error::Http(e) => e.is_timeout() || e.is_connect() || e.is_request() || e.is_body(),
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
