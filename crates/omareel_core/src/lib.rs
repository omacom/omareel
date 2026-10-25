//! omareel's program logic, free of Qt so any front end can use it. Where a behaviour came
//! from a Qt class (number formatting, JSON, colours, paths), `compat` reproduces it exactly,
//! because the files omareel writes must stay byte-identical.

pub mod cli;
pub mod compat;
