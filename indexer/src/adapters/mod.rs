use std::path::Path;
use crate::model::{Candidate, Parsed};

pub mod claude;
pub mod codex;
pub mod omp;
pub mod pi;
pub mod opencode;
pub mod cursor;

pub trait Adapter {
    fn agent(&self) -> &'static str;
    /// All current sources; Ok(vec![]) when the store doesn't exist.
    fn candidates(&mut self, home: &Path) -> anyhow::Result<Vec<Candidate>>;
    /// Ok(None): not a listable session (empty, no user text).
    fn parse(&mut self, home: &Path, c: &Candidate) -> anyhow::Result<Option<Parsed>>;
}
