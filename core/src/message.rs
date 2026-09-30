use std::fmt::Display;

pub struct Message {
    pub value: String,
}

impl Message {
    pub fn new(value: String) -> Self {
        Message { value }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}
