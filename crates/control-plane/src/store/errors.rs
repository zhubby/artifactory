//! [INPUT]: 依赖数据库约束分类与 HTTP 业务错误。
//! [OUTPUT]: 将唯一性、引用和工作区状态约束转为明确响应。
//! [POS]: SQL 失败与业务接口之间的统一错误边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::error::ApiError;
use artifactory_database::{Constraint, Error};
impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        match artifactory_database::constraint(&error) {
            Some(Constraint::Unique) => Self::Conflict("resource already exists".into()),
            Some(Constraint::ObjectState) => Self::Conflict("workspace is read only".into()),
            Some(Constraint::ForeignKey | Constraint::Check | Constraint::NotNull) => {
                Self::BadRequest("invalid resource reference or value".into())
            }
            None if error.is_not_found() => Self::NotFound,
            None => Self::Internal(error.into()),
        }
    }
}
impl From<toasty::Error> for ApiError {
    fn from(error: toasty::Error) -> Self {
        Error::from(error).into()
    }
}
