#[derive(Debug)]
pub enum AppError {
    BaseNotFound(String),
    InvalidConfig(String),
    InvalidQuery(String),
    Io(std::io::Error),
    Other(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::BaseNotFound(name) => write!(f, "Base not found: {}", name),
            AppError::InvalidConfig(msg) => write!(f, "Invalid base config: {}", msg),
            AppError::InvalidQuery(msg) => write!(f, "Invalid search query: {}", msg),
            AppError::Io(e) => write!(f, "I/O error: {}", e),
            AppError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl From<Box<dyn std::error::Error>> for AppError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        classify(e.to_string())
    }
}

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn classify(msg: String) -> AppError {
    let lower = msg.to_lowercase();
    if lower.contains("not found") {
        AppError::BaseNotFound(msg)
    } else if lower.contains("broken json") {
        AppError::InvalidConfig(msg)
    } else {
        AppError::Other(msg)
    }
}

impl From<BoxError> for AppError {
    fn from(e: BoxError) -> Self {
        match e.downcast::<AppError>() {
            Ok(app_err) => *app_err,
            Err(other) => classify(other.to_string()),
        }
    }
}

impl From<String> for AppError {
    fn from(msg: String) -> Self {
        classify(msg)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e)
    }
}

pub type AppResult<T> = Result<T, AppError>;
