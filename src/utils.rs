use crate::{CommentCommand, MergeCommentCommand};

pub fn parse_bot_comment_from_text(text: &str) -> Option<CommentCommand> {
	let text = text.to_lowercase();
	let text = text.trim();

	let cmd = match text {
		"bot merge" => CommentCommand::Merge(MergeCommentCommand::Normal),
		"bot merge force" => CommentCommand::Merge(MergeCommentCommand::Force),
		"bot merge cancel" => CommentCommand::CancelMerge,
		"bot rebase" => CommentCommand::Rebase,
		"bot burnin" => CommentCommand::BurninRequest,
		"bot compare substrate" => CommentCommand::CompareReleaseRequest,
		_ => return None,
	};

	Some(cmd)
}
