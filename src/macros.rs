#[macro_export]
macro_rules! OWNER_AND_REPO_SEQUENCE {
	() => {
		r"(?P<owner>[^ \t\n]+)/(?P<repo>[^ \t\n]+)"
	};
}

#[macro_export]
macro_rules! PR_HTML_URL_REGEX {
	() => {
		concat!(
			r"(?P<html_url>https://[^ \t\n]+/",
			OWNER_AND_REPO_SEQUENCE!(),
			r"/pull/(?P<number>[[:digit:]]+))"
		)
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
			OWNER_AND_REPO_SEQUENCE!(),
			r"#(?P<number>[[:digit:]]+)"
		)
	};
}

#[macro_export]
macro_rules! PROCESS_INFO_ERROR_TEMPLATE {
	() => {
"
Error: When trying to meet the \"Project Owners\" approval requirements: this pull request does not belong to a project defined in {}.

Approval by \"Project Owners\" is only attempted if other means defined in the [criteria for merge](https://github.com/paritytech/parity-processbot#criteria-for-merge) are not satisfied first.

{}

{}
"
	}
}

#[macro_export]
macro_rules! WEBHOOK_PARSING_ERROR_TEMPLATE {
	() => {
		"Webhook event parsing failed due to:

```
{}
```

Payload:

```
{}
```
"
	};
}
