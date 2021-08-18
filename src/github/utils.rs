pub fn parse_issue_details_from_pr_html_url(
	pr_html_url: &str,
) -> Option<IssueDetails> {
	let re = Regex::new(PR_HTML_URL_REGEX!()).unwrap();
	let matches = re.captures(&pr_html_url)?;
	let owner = matches.name("owner")?.as_str().to_owned();
	let repo = matches.name("repo")?.as_str().to_owned();
	let number = matches
		.name("number")?
		.as_str()
		.to_owned()
		.parse::<usize>()
		.ok()?;
	Some((owner, repo, number))
}

pub fn parse_repository_full_name(full_name: &str) -> Option<(String, String)> {
	let parts: Vec<&str> = full_name.split("/").collect();
	parts
		.get(0)
		.map(|owner| {
			parts.get(1).map(|repo_name| {
				Some((owner.to_string(), repo_name.to_string()))
			})
		})
		.flatten()
		.flatten()
}

pub fn owner_from_html_url(url: &str) -> Option<&str> {
	url.split("/").skip(3).next()
}
