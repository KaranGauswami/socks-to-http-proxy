#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub struct Auth {
    pub username: String,
    pub password: String,
}

impl Auth {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }
}
