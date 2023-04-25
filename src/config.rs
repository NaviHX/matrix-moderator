use crate::reply::ReplyType;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct ReplyConfigEntry {
    pub patterns: Vec<String>,
    pub reply: ReplyType,
}
