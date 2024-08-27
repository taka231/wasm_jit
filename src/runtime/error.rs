use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum RuntimeError {
    #[error("Export not found: {0}")]
    ExportNotFound(String),
    #[error("Function not found: {0}")]
    FunctionNotFound(String),
    #[error("Function type not found: {0}")]
    FunctionTypeNotFound(String),
}
