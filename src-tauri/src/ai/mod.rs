//! 로컬 AI — 전부 이 맥 안에서 돈다. 밖으로 나가는 건 모델을 처음 받을 때뿐.
//!
//! 지금은 CLIP 하나다: 사진마다 512차원 벡터를 만들어 두고, 「이 사진과
//! 비슷한 것」을 그 벡터 사이 각도로 찾는다. 얼굴·글로 찾기는 그 뒤.
//!
//! 엔진은 ONNX Runtime(ort). 애플 실리콘이면 CoreML로 신경엔진·GPU를 쓴다.

pub mod clip;
pub mod embed;
pub mod models;
pub mod similar;

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("모델이 없습니다 — 설정 › AI에서 받으세요")]
    NoModel,
    #[error("ONNX Runtime: {0}")]
    Ort(String),
    #[error("그림을 읽을 수 없습니다: {0}")]
    Image(#[from] image::ImageError),
    #[error("데이터베이스: {0}")]
    Db(#[from] crate::db::conn::DbError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AiError>;

// ort의 오류는 어느 단계에서 났는지를 타입으로 들고 있다(Error<SessionBuilder>
// 같은 것). 우리는 그걸 되살릴 일이 없어 글로만 받는다.
impl<R> From<ort::Error<R>> for AiError {
    fn from(e: ort::Error<R>) -> Self {
        AiError::Ort(e.to_string())
    }
}
