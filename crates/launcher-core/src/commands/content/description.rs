use ammonia::{Builder, UrlRelative};
use pulldown_cmark::{html, Options, Parser};
use std::{borrow::Cow, collections::HashSet};

// Parse mixed HTML/CommonMark with real parsers, then keep only the rich-text
// subset the native viewer needs. No script, CSS, remote stylesheet or local URL.
pub(super) fn render(body: &str) -> String {
    // Keep the IPC response and rich-text parser bounded even for hostile projects.
    let mut end = body.len().min(512 * 1024);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < body.len();
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut html = String::new();
    html::push_html(&mut html, Parser::new_ext(&body[..end], options));
    let tags: HashSet<_> = [
        "a",
        "p",
        "br",
        "hr",
        "div",
        "span",
        "center",
        "font",
        "b",
        "strong",
        "i",
        "em",
        "u",
        "s",
        "del",
        "sup",
        "sub",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "blockquote",
        "pre",
        "code",
        "ul",
        "ol",
        "li",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
        "img",
    ]
    .into_iter()
    .collect();
    let mut cleaner = Builder::default();
    cleaner
        .tags(tags)
        .url_schemes(["https", "http"].into_iter().collect())
        .url_relative(UrlRelative::Deny)
        .add_tag_attributes("font", &["size"])
        .add_generic_attributes(&["align"])
        .attribute_filter(|tag, attr, value| match attr {
            "src" | "href" => {
                let url = url::Url::parse(value).ok()?;
                if !["http", "https"].contains(&url.scheme())
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    None
                } else {
                    Some(Cow::Borrowed(value))
                }
            }
            "width" | "height" | "style" | "class" | "title" => None,
            "align" if ["left", "center", "right"].contains(&value) => Some(Cow::Borrowed(value)),
            "align" => None,
            "size" if tag == "font" && ["1", "2", "3", "4", "5", "6", "7"].contains(&value) => {
                Some(Cow::Borrowed(value))
            }
            "size" => None,
            _ => Some(Cow::Borrowed(value)),
        });
    let mut clean = cleaner.clean(&html).to_string();
    if truncated {
        clean
            .push_str("<p>Описание сокращено. Полная версия — на странице проекта в Modrinth.</p>");
    }
    // QTextDocument understands aligned divs; nested HTML5 center/p tags do not
    // need to be delegated to its Markdown importer anymore.
    clean
        .replace("<center>", "<div align=\"center\">")
        .replace("</center>", "</div>")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_html_preserves_lists_entities_and_text_after_embeds() {
        let html = render("<p><center><font size=\"5\">Heading</font></center></p>\n<iframe src=\"https://www.youtube.com/embed/test\"></iframe>\n<ul><li>Pok&eacute;mon <strong>feature</strong></li><li>Second item</li></ul>\n\n## Details\n\n**Bold** and [link](https://modrinth.com)");
        for part in [
            "Heading",
            "Pokémon",
            "Second item",
            "<h2>Details</h2>",
            "<strong>Bold</strong>",
        ] {
            assert!(html.contains(part), "{html}");
        }
        assert!(!html.contains("iframe"));
    }
    #[test]
    fn drops_scripts_css_local_urls_and_fixed_image_geometry() {
        let html = render("<script>alert(1)</script><style>body{background:white}</style><img src='file:///C:/secret' width='90000' onerror='run()'><img src='https://cdn.modrinth.com/banner.png' width='838' height='32'><a href='javascript:run()'>name</a><p style='color:white'>Visible</p>");
        for forbidden in [
            "script",
            "file:",
            "onerror",
            "width=",
            "height=",
            "style=",
            "background:",
        ] {
            assert!(!html.contains(forbidden), "{html}");
        }
        assert!(html.contains("https://cdn.modrinth.com/banner.png"));
        assert!(html.contains("Visible"));
    }
}
