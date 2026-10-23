use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

fn escape(text: &str, reserved: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        if reserved.contains(ch) {
            result.push('\\');
        }
        result.push(ch);
    }
    result
}

#[derive(Default)]
struct Writer {
    parts: Vec<String>,
    part: String,
}

impl Writer {
    fn text(&mut self, text: &str, styles: &[(String, String)], code: bool) {
        let opening: String = styles.iter().map(|s| s.0.as_str()).collect();
        let closing: String = styles.iter().rev().map(|s| s.1.as_str()).collect();
        let (opening, closing) = if opening.len() + closing.len() > 3000 {
            (String::new(), String::new())
        } else {
            (opening, closing)
        };
        let mut body = String::new();
        let reserved = if code { "\\`" } else { "\\_*[]()~`>#+-=|{}.!" };
        for ch in text.chars() {
            let escaped = escape(&ch.to_string(), reserved);
            if self.part.len() + opening.len() + body.len() + escaped.len() + closing.len() + 2
                > 3800
            {
                if !body.is_empty() {
                    self.part.push_str(&opening);
                    self.part.push_str(&body);
                    self.part.push_str(&closing);
                    body.clear();
                }
                if !self.part.is_empty() {
                    self.parts.push(std::mem::take(&mut self.part));
                }
            }
            body.push_str(&escaped);
        }
        if !body.is_empty() {
            if !self.part.is_empty() && !opening.is_empty() {
                self.part.push('\r');
            }
            self.part.push_str(&opening);
            self.part.push_str(&body);
            self.part.push_str(&closing);
            if !closing.is_empty() {
                self.part.push('\r');
            }
        }
    }

    fn newline(&mut self) {
        if !self.part.is_empty() && !self.part.ends_with('\n') {
            self.text("\n", &[], false);
        }
    }
}

pub fn chunks(markdown: &str) -> Vec<String> {
    let mut writer = Writer::default();
    let mut frames = Vec::new();
    let mut styles = Vec::new();
    let mut lists = Vec::new();
    let mut code = false;
    for event in Parser::new_ext(
        markdown,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        match event {
            Event::Start(tag) => {
                frames.push((styles.clone(), code));
                match tag {
                    Tag::Strong | Tag::Heading { .. } => {
                        if !styles.iter().any(|s: &(String, String)| s.0 == "*") {
                            styles.push(("*".into(), "*".into()));
                        }
                    }
                    Tag::Emphasis | Tag::Strikethrough => {
                        let marker = if matches!(tag, Tag::Emphasis) {
                            "_"
                        } else {
                            "~"
                        };
                        if !styles.iter().any(|s| s.0 == marker) {
                            styles.push((marker.into(), marker.into()));
                        }
                    }
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                        styles.push(("[".into(), format!("]({})", escape(&dest_url, "\\)"))));
                    }
                    Tag::CodeBlock(kind) => {
                        writer.newline();
                        code = true;
                        let language = match kind {
                            CodeBlockKind::Fenced(info) => info
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .chars()
                                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                                .collect::<String>(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        styles = vec![(format!("```{language}\n"), "\n```".into())];
                    }
                    Tag::List(start) => {
                        writer.newline();
                        lists.push(start);
                    }
                    Tag::Item => {
                        writer.newline();
                        let prefix = match lists.last_mut() {
                            Some(Some(number)) => {
                                let prefix = format!("{number}. ");
                                *number += 1;
                                prefix
                            }
                            _ => "• ".into(),
                        };
                        writer.text(&prefix, &[], false);
                    }
                    Tag::BlockQuote(_) => {
                        writer.newline();
                        writer.text("❯ ", &[], false);
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                if let Some(previous) = frames.pop() {
                    (styles, code) = previous;
                }
                if matches!(tag, TagEnd::List(_)) {
                    lists.pop();
                }
                if matches!(
                    tag,
                    TagEnd::Paragraph
                        | TagEnd::Heading(_)
                        | TagEnd::CodeBlock
                        | TagEnd::Item
                        | TagEnd::List(_)
                        | TagEnd::BlockQuote(_)
                ) {
                    writer.newline();
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                writer.text(&text, &styles, code)
            }
            Event::Code(text) => writer.text(&text, &[("`".into(), "`".into())], true),
            Event::SoftBreak | Event::HardBreak => writer.text("\n", &[], false),
            Event::Rule => {
                writer.newline();
                writer.text("———\n", &[], false);
            }
            Event::TaskListMarker(done) => writer.text(if done { "☑ " } else { "☐ " }, &[], false),
            _ => {}
        }
    }
    if !writer.part.is_empty() {
        writer.parts.push(writer.part);
    }
    writer.parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_common_markdown_and_escapes_literals() {
        let output = chunks("# Title\n\n**bold** and *italic* and ~~gone~~.\n\n[Link](https://example.com/a_(b))\n\n`x_*`\n\n```rust\nlet x = `a`;\n```\n\n- one\n- two").concat();
        assert!(output.contains("*Title*"));
        assert!(output.contains("*bold*"));
        assert!(output.contains("_italic_"));
        assert!(output.contains("~gone~"));
        assert!(output.contains("\\."));
        assert!(output.contains("[Link](https://example.com/a_(b\\))"));
        assert!(output.contains("`x_*`"));
        assert!(output.contains("```rust\nlet x = \\`a\\`;"));
        assert!(output.contains("• one\n• two"));
    }

    #[test]
    fn splits_long_formatted_unicode_with_closed_formatting() {
        for (source, opening, closing) in [
            (format!("**{}**", "😀!".repeat(4000)), "*", "*"),
            (
                format!("```\n{}\n```", "😀`".repeat(4000)),
                "```\n",
                "\n```",
            ),
        ] {
            let parts = chunks(&source);
            assert!(parts.len() > 1);
            for part in parts {
                assert!(part.len() <= 3800);
                let part = part.trim_matches(['\r', '\n']);
                assert!(part.starts_with(opening));
                assert!(part.ends_with(closing));
            }
        }
    }
}
