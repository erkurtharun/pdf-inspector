//! Backslash escapes for source text written into Markdown.

use std::borrow::Cow;

/// Whether `c` can start inline Markdown or GFM syntax: emphasis, code
/// spans, links, HTML, entities, table pipes or strikethrough.
fn is_inline_special(c: char) -> bool {
    matches!(
        c,
        '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '&' | '|' | '~'
    )
}

/// `text` with a backslash before each character that Markdown could read
/// as inline syntax. Text without such characters comes back borrowed.
pub(crate) fn escape_inline(text: &str) -> Cow<'_, str> {
    if !text.contains(is_inline_special) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if is_inline_special(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    Cow::Owned(escaped)
}

/// `line` with a backslash before a leading character that would open a
/// block: a heading (`#`), a block quote (`>`), a bullet or thematic break
/// (`+`, `-`), a setext underline (`=`), or the `.` or `)` of an ordered
/// list marker (`1. `, `12)`). Other lines come back borrowed.
pub(crate) fn escape_block_start(line: &str) -> Cow<'_, str> {
    if line.starts_with(['#', '>', '+', '-', '=']) {
        return Cow::Owned(format!("\\{line}"));
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &line[digits..];
    if (1..=9).contains(&digits)
        && rest.starts_with(['.', ')'])
        && rest[1..].chars().next().is_none_or(char::is_whitespace)
    {
        return Cow::Owned(format!("{}\\{rest}", &line[..digits]));
    }
    Cow::Borrowed(line)
}

/// `text` with [`escape_block_start`] applied to each line after a line
/// break it carries, as a multi-line form value or ActualText can.
pub(crate) fn escape_inner_line_starts(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 4);
    for (i, line) in text.split_inclusive(['\n', '\r']).enumerate() {
        if i == 0 {
            escaped.push_str(line);
            continue;
        }
        let body = line.trim_start_matches([' ', '\t']);
        escaped.push_str(&line[..line.len() - body.len()]);
        escaped.push_str(&escape_block_start(body));
    }
    escaped
}

/// Heading text with a backslash before a closing run of `#`, which an ATX
/// heading would otherwise drop: `Section ##` becomes `Section \##`, while
/// `C#` comes back borrowed.
pub(crate) fn escape_closing_hashes(text: &str) -> Cow<'_, str> {
    let body = text.trim_end_matches('#');
    if body.len() == text.len() || !(body.is_empty() || body.ends_with([' ', '\t'])) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(format!("{body}\\{}", &text[body.len()..]))
}

/// Whether `line` opens a code fence, outside one, or closes it, inside
/// one: a closing fence is a run of backticks alone, so a fence line that
/// carries an info string inside a code block is code.
fn toggles_fence(line: &str, in_fence: bool) -> bool {
    if in_fence {
        let line = line.trim_end();
        line.len() >= 3 && line.bytes().all(|b| b == b'`')
    } else {
        line.starts_with("```")
    }
}

/// Whether the end of `markdown` lies inside a code fence, judging only the
/// lines `markdown` ends with a line break.
pub(crate) fn ends_in_fence(markdown: &str) -> bool {
    markdown
        .split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
        .fold(false, |in_fence, line| {
            in_fence ^ toggles_fence(line, in_fence)
        })
}

/// `markdown` with the backslash of each escape outside a code fence
/// removed, as a parser reads its text, and for each byte of that text the
/// offset in `markdown` where its character starts, escape included,
/// followed by `markdown.len()`.
pub(crate) fn unescape_with_offsets(markdown: &str) -> (String, Vec<usize>) {
    let mut text = String::with_capacity(markdown.len());
    let mut offsets = Vec::with_capacity(markdown.len() + 1);
    let mut in_fence = false;
    let mut line_start = 0;
    for line in markdown.split_inclusive('\n') {
        let fence = toggles_fence(line, in_fence);
        let mut escape = None;
        for (i, c) in line.char_indices() {
            let at = line_start + i;
            if !(in_fence || fence)
                && escape.is_none()
                && c == '\\'
                && line[i + 1..].starts_with(|next: char| next.is_ascii_punctuation())
            {
                escape = Some(at);
                continue;
            }
            offsets.push(escape.take().unwrap_or(at));
            offsets.extend(at + 1..at + c.len_utf8());
            text.push(c);
        }
        in_fence ^= fence;
        line_start += line.len();
    }
    offsets.push(markdown.len());
    (text, offsets)
}

/// The text of `markdown` that text-quality checks judge: with its escapes
/// removed when `escaped`, as `escape_text` writes them, and `markdown`
/// itself otherwise.
pub(crate) fn unescaped_text(markdown: &str, escaped: bool) -> Cow<'_, str> {
    if escaped && markdown.contains('\\') {
        Cow::Owned(unescape_with_offsets(markdown).0)
    } else {
        Cow::Borrowed(markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_syntax_characters_are_escaped() {
        assert_eq!(
            escape_inline(r"a *b* c_d [e](f) <u>g</u> & h|i ~j~ `k` \l"),
            r"a \*b\* c\_d \[e\](f) \<u\>g\</u\> \& h\|i \~j\~ \`k\` \\l"
        );
    }

    #[test]
    fn inline_text_without_syntax_is_borrowed() {
        for text in ["", "plain text", "# 1. - + = (x) ! . ,", "naïve – 3"] {
            assert!(matches!(escape_inline(text), Cow::Borrowed(_)), "{text}");
        }
        assert_eq!(escape_inline("#1 > 2"), r"#1 \> 2");
    }

    #[test]
    fn block_markers_at_line_start_are_escaped() {
        assert_eq!(escape_block_start("# 3 items"), r"\# 3 items");
        assert_eq!(escape_block_start("> quoted"), r"\> quoted");
        assert_eq!(escape_block_start("+ item"), r"\+ item");
        assert_eq!(escape_block_start("- item"), r"\- item");
        assert_eq!(escape_block_start("---"), r"\---");
        assert_eq!(escape_block_start("==="), r"\===");
        assert_eq!(escape_block_start("1. Scope"), r"1\. Scope");
        assert_eq!(escape_block_start("123456789) x"), r"123456789\) x");
        assert_eq!(escape_block_start("2."), r"2\.");
    }

    #[test]
    fn lines_that_open_no_block_are_borrowed() {
        for line in [
            "",
            "plain",
            "a # b",
            "3.5 percent",
            "1234567890. ten digits",
            "2024)x",
            "(1) item",
            "a. item",
            r"\* item",
        ] {
            assert!(
                matches!(escape_block_start(line), Cow::Borrowed(_)),
                "{line}"
            );
        }
    }

    #[test]
    fn block_markers_after_a_line_break_are_escaped() {
        assert_eq!(
            escape_inner_line_starts("# a\n# b\r\n  - c\r2. d"),
            "# a\n\\# b\r\n  \\- c\r2\\. d"
        );
        assert_eq!(escape_inner_line_starts("# a"), "# a");
    }

    #[test]
    fn unescaping_removes_escapes_outside_code_fences() {
        let markdown = "\\# a\\_b \\\\x **c**\n```\n\\_\n```\n1\\. \\é";
        let (text, offsets) = unescape_with_offsets(markdown);
        assert_eq!(text, "# a_b \\x **c**\n```\n\\_\n```\n1. \\é");
        assert_eq!(offsets.len(), text.len() + 1);
        assert_eq!(&markdown[offsets[2]..offsets[5]], "a\\_b");
        assert_eq!(&markdown[offsets[0]..offsets[1]], "\\#");
        assert_eq!(offsets[text.len()], markdown.len());
        assert_eq!(unescaped_text(markdown, true), text);
        assert!(matches!(unescaped_text(markdown, false), Cow::Borrowed(_)));
        assert!(matches!(unescaped_text("a b", true), Cow::Borrowed(_)));
    }

    #[test]
    fn a_fence_line_with_an_info_string_does_not_close_a_code_block() {
        let markdown = "```\n```rust\n\\_x\n```\nafter \\_y\n";
        assert_eq!(
            unescape_with_offsets(markdown).0,
            "```\n```rust\n\\_x\n```\nafter _y\n"
        );
        assert!(ends_in_fence("a\n```\n```rust\nb"));
        assert!(!ends_in_fence("```\n```rust\n```\nb"));
        assert!(!ends_in_fence("```"));
    }

    #[test]
    fn closing_hash_run_of_a_heading_is_escaped() {
        assert_eq!(escape_closing_hashes("Section ##"), r"Section \##");
        assert_eq!(escape_closing_hashes("#"), r"\#");
        assert!(matches!(escape_closing_hashes("C#"), Cow::Borrowed(_)));
        assert!(matches!(escape_closing_hashes("Title"), Cow::Borrowed(_)));
    }
}
