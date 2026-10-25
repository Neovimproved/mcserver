#![allow(unused)]

use ratatui::text::Span;
use vte::Perform;

pub struct Log<'a> {
    spans: Vec<Span<'a>>,
}

impl<'a> Log<'a> {
    pub fn new() -> Self {
        Self { spans: vec![] }
    }
}

impl<'a> Perform for Log<'a> {
    fn execute(&mut self, _byte: u8) {}
}
