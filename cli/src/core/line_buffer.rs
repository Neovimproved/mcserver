use std::collections::{VecDeque, vec_deque};

use bitcode::{Decode, Encode};

#[derive(Clone, Debug, Decode, Encode)]
pub struct LineBuffer<T = String, const CAPACITY: usize = 1024usize> {
    lines: VecDeque<T>,
}

impl<T, const CAPACITY: usize> Default for LineBuffer<T, CAPACITY> {
    fn default() -> Self {
        Self {
            lines: VecDeque::with_capacity(CAPACITY),
        }
    }
}

impl<T, const CAPACITY: usize> LineBuffer<T, CAPACITY> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, line: T) {
        if self.lines.len() >= CAPACITY {
            self.pop();
        }

        self.lines.push_back(line);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.lines.pop_front()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn extend(&mut self, other: LineBuffer<T>) {
        self.lines.extend(other.lines);
    }

    pub fn iter(&self) -> vec_deque::Iter<'_, T> {
        self.lines.iter()
    }

    pub fn iter_mut(&mut self) -> vec_deque::IterMut<'_, T> {
        self.lines.iter_mut()
    }

    pub fn into_vec_deque(self) -> VecDeque<T> {
        self.lines
    }
}

impl<T, const CAPACITY: usize> IntoIterator for LineBuffer<T, CAPACITY> {
    type Item = T;

    type IntoIter = vec_deque::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.into_iter()
    }
}

impl<T, const CAPACITY: usize> From<VecDeque<T>> for LineBuffer<T, CAPACITY> {
    fn from(mut lines: VecDeque<T>) -> Self {
        lines.reserve_exact(CAPACITY.saturating_sub(lines.len()));
        lines.shrink_to(CAPACITY);
        Self { lines }
    }
}

impl<T, const CAPACITY: usize> From<Vec<T>> for LineBuffer<T, CAPACITY> {
    fn from(mut lines: Vec<T>) -> Self {
        lines.reserve_exact(CAPACITY);
        lines.shrink_to(CAPACITY);
        Self {
            lines: VecDeque::from(lines),
        }
    }
}
