use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CreateService {
    pub cmd: String,
    pub args: Vec<String>,
}
