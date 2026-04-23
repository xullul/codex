//! Command parsing and safety utilities shared across Codex crates.

mod shell_detect;

pub mod bash;
pub(crate) mod command_safety;
pub mod parse_command;
pub mod powershell;
pub(crate) mod powershell_display;
pub(crate) mod powershell_exploration;
pub(crate) mod powershell_line_range;
pub(crate) mod powershell_parser;
pub(crate) mod powershell_syntax;

pub use command_safety::is_dangerous_command;
pub use command_safety::is_safe_command;
