#[macro_export]
macro_rules! PR_HTML_URL_REGEX {
    () => {
        r"(?P<html_url>https://github.com/(?P<owner>[^/\n]+)/(?P<repo>[^/\n]+)/pull/(?P<number>[[:digit:]]+))"
    };
}

#[macro_export]
macro_rules! COMPANION_PREFIX_REGEX {
	() => {
		r"companion[^[[:alpha:]]\n]*"
	};
}

#[macro_export]
macro_rules! COMPANION_LONG_REGEX {
	() => {
		concat!(COMPANION_PREFIX_REGEX!(), PR_HTML_URL_REGEX!())
	};
}

#[macro_export]
macro_rules! COMPANION_SHORT_REGEX {
	() => {
		concat!(
			COMPANION_PREFIX_REGEX!(),
			r"(?P<owner>[^/\n]+)/(?P<repo>[^/\n]+)#(?P<number>[[:digit:]]+)"
		)
	};
}
#[macro_export]
macro_rules! results {
    ($e:expr $(, $es:expr)*$(,)? $(;# $($i:ident)*)?) => {
		match $e {
			Ok(x) => results!($($es),* ;# $($($i)*)? x),
			Err(e) => Err(e)
		}
    };
    ($(;# $($i:ident)*)?) => {
        Ok(($($($i),*)?))
    };
}
