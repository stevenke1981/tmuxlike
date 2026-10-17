//! Resume commands: turn a saved agent command line into text that is typed into a new pane.

/// Shell command line that resumes an agent, or `None` when it must not be typed.
///
/// Supported shells are POSIX sh-family shells and PowerShell. cmd.exe is not supported: its
/// quoting rules differ, so a line produced here must not be typed into it.
///
/// Each argument is single-quoted unless it is plain. An argument that starts with `@`, `=` or
/// `~` is always quoted, because PowerShell and POSIX shells expand those prefixes. Rejected: an
/// empty or path-like first argument, any apostrophe (including PowerShell's typographic ones),
/// and control characters.
pub(super) fn quote_resume(args: &[String]) -> Option<String> {
    let first = args.first()?;
    if first.is_empty() || first.contains(['/', '\\']) {
        return None;
    }
    if args
        .iter()
        .any(|a| a.contains(APOSTROPHES) || a.chars().any(char::is_control))
    {
        return None;
    }
    Some(
        args.iter()
            .map(|a| quote_arg(a))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Apostrophe-like characters. PowerShell treats the typographic ones as quotes too.
const APOSTROPHES: [char; 5] = ['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];

/// Prefixes that shells expand (`@` splatting in PowerShell, `~` home expansion, `=` in zsh).
const EXPANDING_PREFIXES: [char; 3] = ['@', '=', '~'];

fn quote_arg(arg: &str) -> String {
    let plain = !arg.is_empty()
        && !arg.starts_with(EXPANDING_PREFIXES)
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=,@+".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{arg}'")
    }
}

#[cfg(test)]
mod tests {
    use super::quote_resume;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_and_spaced_arguments() {
        assert_eq!(
            quote_resume(&args(&["claude", "--resume", "abc123"])).as_deref(),
            Some("claude --resume abc123")
        );
        assert_eq!(
            quote_resume(&args(&["claude", "my file"])).as_deref(),
            Some("claude 'my file'")
        );
        assert_eq!(
            quote_resume(&args(&["tool", "a;rm -rf x", ""])).as_deref(),
            Some("tool 'a;rm -rf x' ''")
        );
    }

    #[test]
    fn rejects_empty_path_like_apostrophe_and_control_input() {
        assert_eq!(quote_resume(&args(&[])), None);
        assert_eq!(quote_resume(&args(&["", "x"])), None);
        assert_eq!(quote_resume(&args(&["./claude"])), None);
        assert_eq!(quote_resume(&args(&["/usr/bin/claude"])), None);
        assert_eq!(quote_resume(&args(&["C:\\tools\\claude.exe"])), None);
        assert_eq!(quote_resume(&args(&["claude", "it's"])), None);
        assert_eq!(quote_resume(&args(&["claude", "\u{2019}x"])), None);
        assert_eq!(quote_resume(&args(&["claude", "a\nb"])), None);
    }

    #[test]
    fn quotes_expanding_prefixes() {
        assert_eq!(
            quote_resume(&args(&["tool", "@file"])).as_deref(),
            Some("tool '@file'")
        );
        assert_eq!(
            quote_resume(&args(&["tool", "=x"])).as_deref(),
            Some("tool '=x'")
        );
        assert_eq!(
            quote_resume(&args(&["tool", "~user"])).as_deref(),
            Some("tool '~user'")
        );
        assert_eq!(
            quote_resume(&args(&["tool", "@"])).as_deref(),
            Some("tool '@'")
        );
        assert_eq!(
            quote_resume(&args(&["tool", "~/x"])).as_deref(),
            Some("tool '~/x'")
        );
    }

    #[test]
    fn prefix_rule_applies_only_at_the_start() {
        assert_eq!(
            quote_resume(&args(&["tool", "a@b=c"])).as_deref(),
            Some("tool a@b=c")
        );
        assert_eq!(
            quote_resume(&args(&["@tool", "x"])).as_deref(),
            Some("'@tool' x"),
            "a leading @ in the first argument is quoted"
        );
    }
}
