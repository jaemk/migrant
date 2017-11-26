/*!
Macros

*/


macro_rules! print_flush {
    ($lit:expr) => {
        print_flush!($lit,)
    };
    ($lit:expr, $($arg:expr),*) => {
        {
            use ::std::io::Write;
            print!($lit, $($arg),*);
            ::std::io::stdout().flush().expect("Failed Flushing stdout");
        }
    }
}


// -------------
// error-chain
// -------------

/// Helper for formatting Errors that wrap strings
macro_rules! format_err {
    ($error:expr, $str:expr) => {
        $error(format!($str))
    };
    ($error:expr, $str:expr, $($arg:expr),*) => {
        $error(format!($str, $($arg),*))
    }
}


/// Helper for formatting strings with error-chain's `bail!` macro
macro_rules! bail_fmt {
    ($error:expr, $str:expr) => {
        bail!(format_err!($error, $str))
    };
    ($error:expr, $str:expr, $($arg:expr),*) => {
        bail!(format_err!($error, $str, $($arg),*))
    }
}

