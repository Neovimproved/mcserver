#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        use std::io::IsTerminal;

        #[cfg(debug_assertions)]
        if !std::io::stderr().is_terminal() {
            eprintln!($($arg)*);
        }
    }};

}

#[macro_export]
macro_rules! log_dbg {
    ($expression:expr) => {{
        let object = $expression;
        log!("{object:?}");
        object
    }};
}

#[macro_export]
macro_rules! log_bytes {
    ($bytes:expr) => {{
        #[cfg(debug_assertions)]
        use std::io::{self, IsTerminal};

        #[cfg(debug_assertions)]
        if !io::stderr().is_terminal() {
            io::stderr()
                .write_all(&$bytes)
                .await
                .into_diagnostic()
                .wrap_err("Failed to write to stderr")?;
        }
    }};
}
