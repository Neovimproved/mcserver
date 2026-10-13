use std::collections::{VecDeque, vec_deque};

use bitcode::{Decode, Encode};

#[derive(Clone, Debug, Decode, Encode)]
pub struct LineBuffer<const CAPACITY: usize = 1024usize> {
    lines: VecDeque<String>,
}

impl<const CAPACITY: usize> Default for LineBuffer<CAPACITY> {
    fn default() -> Self {
        Self {
            lines: VecDeque::with_capacity(CAPACITY),
        }
    }
}

impl<const CAPACITY: usize> LineBuffer<CAPACITY> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, line: String) {
        if self.lines.len() >= CAPACITY {
            self.pop();
        }

        self.lines.push_back(line);
    }

    pub fn pop(&mut self) -> Option<String> {
        self.lines.pop_front()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn extend(&mut self, other: LineBuffer) {
        self.lines.extend(other.lines);
    }

    pub fn iter(&self) -> vec_deque::Iter<'_, String> {
        self.lines.iter()
    }

    pub fn iter_mut(&mut self) -> vec_deque::IterMut<'_, String> {
        self.lines.iter_mut()
    }
}

impl IntoIterator for LineBuffer {
    type Item = String;

    type IntoIter = vec_deque::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.into_iter()
    }
}

impl<const CAPACITY: usize> From<VecDeque<String>> for LineBuffer<CAPACITY> {
    fn from(mut lines: VecDeque<String>) -> Self {
        lines.reserve_exact(CAPACITY.saturating_sub(lines.len()));
        lines.shrink_to(CAPACITY);
        Self { lines }
    }
}

impl<const CAPACITY: usize> From<Vec<String>> for LineBuffer<CAPACITY> {
    fn from(mut lines: Vec<String>) -> Self {
        lines.reserve_exact(CAPACITY);
        lines.shrink_to(CAPACITY);
        Self {
            lines: VecDeque::from(lines),
        }
    }
}
