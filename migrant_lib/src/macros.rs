/*!
Internal helper macros
*/

/// Construct an `Error` of the given variant from a format string
macro_rules! err {
    ($kind:ident, $($fmt:tt)*) => {
        $crate::errors::Error::$kind(format!($($fmt)*))
    };
}

/// Return early with an `Error` of the given variant
macro_rules! bail {
    ($kind:ident, $($fmt:tt)*) => {
        return Err($crate::errors::Error::$kind(format!($($fmt)*)))
    };
}

pub(crate) use bail;
pub(crate) use err;
