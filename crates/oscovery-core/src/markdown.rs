//! A README as blocks the GTK side lays out: Markdown with GitHub's
//! extensions (tables, task lists, strikethrough, alerts, bare URLs, emoji
//! shortcodes) and the HTML READMEs lean on (centred logos, sized images,
//! badge rows, `<details>`, `<picture>` with a dark variant). Text comes out
//! as Pango markup, with links and images resolved against the repository
//! the way github.com resolves them.

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd, TextMergeStream,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// GitHub's `> [!NOTE]` and friends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        markup: String,
        /// What `#links` to it use: GitHub's slug.
        anchor: String,
        align: Align,
    },
    Paragraph {
        markup: String,
        align: Align,
    },
    /// Images in a row: a logo, a screenshot, a line of badges.
    Images {
        images: Vec<Image>,
        align: Align,
    },
    Code {
        lang: String,
        text: String,
    },
    Quote(Vec<Block>),
    Alert {
        kind: AlertKind,
        blocks: Vec<Block>,
    },
    List {
        /// The first number of an ordered list.
        start: Option<u64>,
        items: Vec<Item>,
    },
    Table {
        aligns: Vec<Align>,
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Details {
        summary: String,
        open: bool,
        blocks: Vec<Block>,
    },
    Rule,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// A task list item's box.
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// Which colour scheme an image is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    #[default]
    Any,
    Light,
    Dark,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    pub src: String,
    /// `<picture>`'s `prefers-color-scheme: dark` source.
    pub dark_src: Option<String>,
    /// `#gh-light-mode-only` / `#gh-dark-mode-only`.
    pub scheme: Scheme,
    pub alt: String,
    pub link: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl Image {
    /// The source to show in the given scheme.
    pub fn src_for(&self, dark: bool) -> &str {
        match &self.dark_src {
            Some(src) if dark => src,
            _ => &self.src,
        }
    }

    /// A status badge rather than a picture of the project.
    pub fn is_badge(&self) -> bool {
        let src = self.src.to_ascii_lowercase();
        ["shields.io", "badgen.net", "badge"]
            .iter()
            .any(|marker| src.contains(marker))
    }
}

/// Whether the README has a picture of the project in it, not just badges.
pub fn has_pictures(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Images { images, .. } => images.iter().any(|image| !image.is_badge()),
        Block::Quote(blocks) | Block::Alert { blocks, .. } | Block::Details { blocks, .. } => {
            has_pictures(blocks)
        }
        Block::List { items, .. } => items.iter().any(|item| has_pictures(&item.blocks)),
        _ => false,
    })
}

/// Where a README's relative links and images point: github.com's blob view
/// and raw.githubusercontent.com at the README's ref, and the README's own
/// directory in the repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Base {
    /// `https://github.com/owner/repo/blob/<ref>/`
    pub html: String,
    /// `https://raw.githubusercontent.com/owner/repo/<ref>/`
    pub raw: String,
    /// `""`, or `docs/` for a README in a subdirectory.
    pub dir: String,
}

impl Base {
    /// The repository at `slug`, on its default branch, README at the root.
    pub fn for_repo(slug: &str) -> Self {
        Base {
            html: format!("https://github.com/{slug}/blob/HEAD/"),
            raw: format!("https://raw.githubusercontent.com/{slug}/HEAD/"),
            dir: String::new(),
        }
    }

    /// A link's target; `#anchor` stays as it is.
    pub fn link(&self, href: &str) -> String {
        resolve(&self.html, &self.dir, href)
    }

    /// An image's URL, raw rather than github.com's page for the file.
    pub fn image(&self, src: &str) -> String {
        let url = resolve(&self.raw, &self.dir, src);
        raw_github(&url).unwrap_or(url)
    }
}

fn resolve(root: &str, dir: &str, href: &str) -> String {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') {
        return href.to_string();
    }
    if let Some(rest) = href.strip_prefix("//") {
        return format!("https://{rest}");
    }
    if has_scheme(href) || root.is_empty() {
        return href.to_string();
    }
    let split = href.find(['?', '#']).unwrap_or(href.len());
    let (path, suffix) = href.split_at(split);
    let joined = match path.strip_prefix('/') {
        Some(from_root) => from_root.to_string(),
        None => format!("{dir}{path}"),
    };
    format!("{root}{}{suffix}", normalize(&joined))
}

fn has_scheme(href: &str) -> bool {
    match href.find(':') {
        Some(colon) => {
            let scheme = &href[..colon];
            scheme.starts_with(|ch: char| ch.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        }
        None => false,
    }
}

/// `a/./b/../c` -> `a/c`, keeping a trailing slash.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let mut out = parts.join("/");
    if path.ends_with('/') && !out.is_empty() {
        out.push('/');
    }
    out
}

/// github.com's `blob/` or `raw/` page for a file, as its raw URL.
fn raw_github(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://github.com/")?;
    let mut parts = rest.splitn(4, '/');
    let (owner, repo, kind, tail) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if kind != "blob" && kind != "raw" {
        return None;
    }
    let tail = tail.split(['?', '#']).next().unwrap_or(tail);
    let fragment = url.find('#').map_or("", |at| &url[at..]);
    Some(format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{tail}{fragment}"
    ))
}

/// Inline code: monospace on a tint that reads in light and dark alike.
pub const CODE_OPEN: &str = "<span font_family=\"monospace\" bgcolor=\"#808080\" bgalpha=\"20%\">";

/// Text made safe for Pango markup.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            ch => out.push(ch),
        }
    }
    out
}

/// Markup's text, without its tags.
pub fn plain(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut in_tag = false;
    for ch in markup.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            ch if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_entities(&out)
}

/// GitHub's anchor for a heading: lower case, spaces to hyphens, other
/// punctuation dropped.
pub fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                Some(ch)
            } else if ch.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest
            .bytes()
            .skip(1)
            .take(12)
            .position(|byte| byte == b';')
            .and_then(|end| entity(&rest[1..1 + end]).map(|ch| (ch, end + 2)));
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ensp" | "emsp" | "thinsp" => ' ',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "middot" => '·',
        "bull" => '•',
        "rarr" => '→',
        "larr" => '←',
        "times" => '×',
        _ => return None,
    })
}

/// The shortcodes READMEs use most; GitHub knows many more.
const EMOJI: &[(&str, &str)] = &[
    ("+1", "👍"),
    ("arrow_right", "➡️"),
    ("art", "🎨"),
    ("book", "📖"),
    ("books", "📚"),
    ("bug", "🐛"),
    ("bulb", "💡"),
    ("camera", "📷"),
    ("clipboard", "📋"),
    ("coffee", "☕"),
    ("computer", "💻"),
    ("construction", "🚧"),
    ("exclamation", "❗"),
    ("eyes", "👀"),
    ("fire", "🔥"),
    ("gear", "⚙️"),
    ("hammer", "🔨"),
    ("heart", "❤️"),
    ("heavy_check_mark", "✔️"),
    ("hourglass", "⌛"),
    ("information_source", "ℹ️"),
    ("keyboard", "⌨️"),
    ("link", "🔗"),
    ("lock", "🔒"),
    ("mag", "🔍"),
    ("memo", "📝"),
    ("movie_camera", "🎥"),
    ("package", "📦"),
    ("penguin", "🐧"),
    ("point_right", "👉"),
    ("pray", "🙏"),
    ("pushpin", "📌"),
    ("question", "❓"),
    ("rocket", "🚀"),
    ("smile", "😄"),
    ("sparkles", "✨"),
    ("star", "⭐"),
    ("tada", "🎉"),
    ("thumbsup", "👍"),
    ("warning", "⚠️"),
    ("wave", "👋"),
    ("white_check_mark", "✅"),
    ("wrench", "🔧"),
    ("x", "❌"),
    ("zap", "⚡"),
];

fn emoji(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(':') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let name_end = rest[1..].find(':').filter(|&end| {
            end > 0
                && end <= 32
                && rest[1..1 + end]
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-'))
        });
        match name_end.and_then(|end| {
            EMOJI
                .iter()
                .find(|(name, _)| *name == &rest[1..1 + end])
                .map(|(_, glyph)| (*glyph, end + 2))
        }) {
            Some((glyph, len)) => {
                out.push_str(glyph);
                rest = &rest[len..];
            }
            None => {
                out.push(':');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Split text at bare `http(s)://` URLs, which GitHub turns into links.
fn autolinks(text: &str) -> Vec<(&str, bool)> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some(at) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| rest.find(scheme))
        .min()
    {
        let len = rest[at..]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '>' | '"'))
            .unwrap_or(rest.len() - at);
        let mut url = &rest[at..at + len];
        loop {
            let trimmed = url.trim_end_matches(['.', ',', ':', ';', '!', '?', '\'', '"', '*', '_']);
            let trimmed = if trimmed.ends_with(')')
                && trimmed.matches(')').count() > trimmed.matches('(').count()
            {
                &trimmed[..trimmed.len() - 1]
            } else {
                trimmed
            };
            if trimmed == url {
                break;
            }
            url = trimmed;
        }
        if url.len() <= "https://".len() {
            pieces.push((&rest[..at + len], false));
            rest = &rest[at + len..];
            continue;
        }
        if at > 0 {
            pieces.push((&rest[..at], false));
        }
        pieces.push((url, true));
        rest = &rest[at + url.len()..];
    }
    if !rest.is_empty() {
        pieces.push((rest, false));
    }
    pieces
}

#[derive(Debug, PartialEq)]
enum Token {
    Open {
        name: String,
        attrs: Vec<(String, String)>,
    },
    Close(String),
    Text(String),
}

impl Token {
    fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        attrs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// HTML into tags and text; comments and doctypes dropped.
fn tokenize(html: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut text_start = 0;
    while let Some(offset) = html[i..].find('<') {
        let at = i + offset;
        let rest = &html[at..];
        let (len, token) = if rest.starts_with("<!--") {
            (rest.find("-->").map_or(rest.len(), |end| end + 3), None)
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            (rest.find('>').map_or(rest.len(), |end| end + 1), None)
        } else if let Some((len, token)) = parse_tag(rest) {
            (len, Some(token))
        } else {
            i = at + 1;
            continue;
        };
        if text_start < at {
            tokens.push(Token::Text(html[text_start..at].to_string()));
        }
        tokens.extend(token);
        i = at + len;
        text_start = i;
    }
    if text_start < html.len() {
        tokens.push(Token::Text(html[text_start..].to_string()));
    }
    tokens
}

/// A tag at the start of `s`, and its length.
fn parse_tag(s: &str) -> Option<(usize, Token)> {
    let b = s.as_bytes();
    let mut i = 1;
    let closing = b.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    let start = i;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-') {
        i += 1;
    }
    if i == start || !b[start].is_ascii_alphabetic() {
        return None;
    }
    let name = s[start..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        match b.get(i)? {
            b'>' => {
                i += 1;
                break;
            }
            b'=' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        let key_start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let key = s[key_start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = "";
        if b.get(i) == Some(&b'=') {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let quote = *b.get(i)?;
            if quote == b'"' || quote == b'\'' {
                let end = s[i + 1..].find(quote as char)?;
                value = &s[i + 1..i + 1 + end];
                i += end + 2;
            } else {
                let value_start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = &s[value_start..i];
            }
        }
        attrs.push((key, decode_entities(value)));
    }
    let token = if closing {
        Token::Close(name)
    } else {
        Token::Open { name, attrs }
    };
    Some((i, token))
}

/// Parse a README into blocks.
pub fn parse(markdown: &str, base: &Base) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let mut builder = Builder::new(base);
    for event in TextMergeStream::new(Parser::new_ext(markdown, options)) {
        builder.event(event);
    }
    builder.finish()
}

enum Run {
    /// Balanced markup.
    Text(String),
    Image(Image),
    Break,
}

#[derive(Clone, PartialEq)]
struct Fmt {
    /// What closes it: a Markdown tag or an HTML element name.
    key: String,
    open: String,
    close: &'static str,
    href: Option<String>,
}

/// The paragraph being read: runs of text and images. Formats can open in
/// one piece of text and close in another, so each run of text opens what
/// is in force and closes it again, and its markup is always balanced.
#[derive(Default)]
struct Inline {
    runs: Vec<Run>,
    text: String,
    fmt: Vec<Fmt>,
    opened: Vec<Fmt>,
}

impl Inline {
    fn push_text(&mut self, markup: &str) {
        let markup = if self.at_line_start() {
            markup.trim_start()
        } else {
            markup
        };
        if markup.is_empty() {
            return;
        }
        let common = self
            .opened
            .iter()
            .zip(&self.fmt)
            .take_while(|(a, b)| a == b)
            .count();
        for fmt in self.opened.drain(common..).rev() {
            self.text.push_str(fmt.close);
        }
        for fmt in &self.fmt[common..] {
            self.text.push_str(&fmt.open);
            self.opened.push(fmt.clone());
        }
        self.text.push_str(markup);
    }

    fn close_run(&mut self) {
        for fmt in self.opened.drain(..).rev() {
            self.text.push_str(fmt.close);
        }
        if !self.text.is_empty() {
            self.runs.push(Run::Text(std::mem::take(&mut self.text)));
        }
    }

    fn push(&mut self, run: Run) {
        self.close_run();
        self.runs.push(run);
    }

    fn take(&mut self) -> Vec<Run> {
        self.close_run();
        std::mem::take(&mut self.runs)
    }

    /// Nothing on the current line yet, so leading space is dropped.
    fn at_line_start(&self) -> bool {
        self.text.is_empty() && !matches!(self.runs.last(), Some(Run::Text(_)))
    }

    fn link(&self) -> Option<String> {
        self.fmt.iter().rev().find_map(|fmt| fmt.href.clone())
    }

    fn push_fmt(&mut self, key: &str, open: &str, close: &'static str) {
        self.fmt.push(Fmt {
            key: key.to_string(),
            open: open.to_string(),
            close,
            href: None,
        });
    }

    fn push_link(&mut self, key: &str, href: Option<String>) {
        // Pango has no nested links; an inner one, or an anchor without an
        // href, only has to close again.
        let href = href.filter(|href| !href.is_empty() && self.link().is_none());
        self.fmt.push(Fmt {
            key: key.to_string(),
            open: href
                .as_ref()
                .map_or_else(String::new, |href| format!("<a href=\"{}\">", escape(href))),
            close: if href.is_some() { "</a>" } else { "" },
            href,
        });
    }

    fn pop_fmt(&mut self, key: &str) {
        if let Some(at) = self.fmt.iter().rposition(|fmt| fmt.key == key) {
            self.fmt.truncate(at);
        }
    }
}

enum Kind {
    Root,
    Quote(Option<AlertKind>),
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Item(Option<bool>),
    Details {
        summary: String,
        open: bool,
    },
}

struct Container {
    kind: Kind,
    blocks: Vec<Block>,
}

#[derive(Default)]
struct Table {
    aligns: Vec<Align>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
}

#[derive(Default)]
struct Picture {
    light: Option<String>,
    dark: Option<String>,
}

struct Builder<'b> {
    base: &'b Base,
    containers: Vec<Container>,
    inline: Inline,
    /// Alignment from `<p align>`, `<div align>`, `<center>`.
    aligns: Vec<Align>,
    /// Open HTML block elements, and whether each pushed an alignment.
    html_open: Vec<(String, bool)>,
    heading: Option<u8>,
    code: Option<(String, String)>,
    /// A Markdown image, collecting its alt text.
    image: Option<Image>,
    table: Option<Table>,
    /// Block HTML arrives a line at a time; it is read whole.
    html: Option<String>,
    summary: bool,
    picture: Option<Picture>,
    /// Inside `<script>` or `<style>`.
    skip: usize,
    anchors: HashMap<String, usize>,
}

impl<'b> Builder<'b> {
    fn new(base: &'b Base) -> Self {
        Builder {
            base,
            containers: vec![Container {
                kind: Kind::Root,
                blocks: Vec::new(),
            }],
            inline: Inline::default(),
            aligns: vec![Align::Start],
            html_open: Vec::new(),
            heading: None,
            code: None,
            image: None,
            table: None,
            html: None,
            summary: false,
            picture: None,
            skip: 0,
            anchors: HashMap::new(),
        }
    }

    fn finish(mut self) -> Vec<Block> {
        if let Some(html) = self.html.take() {
            self.feed_html(&html);
        }
        self.flush();
        while self.containers.len() > 1 {
            let container = self.containers.pop().expect("a container");
            self.fold(container);
        }
        self.containers
            .pop()
            .map(|root| root.blocks)
            .unwrap_or_default()
    }

    fn event(&mut self, event: Event) {
        if let Some(html) = self.html.as_mut() {
            match event {
                Event::Html(text) | Event::InlineHtml(text) => {
                    html.push_str(&text);
                    return;
                }
                Event::End(TagEnd::HtmlBlock) => {
                    let html = self.html.take().unwrap_or_default();
                    self.feed_html(&html);
                    self.flush();
                    return;
                }
                _ => {
                    let html = self.html.take().unwrap_or_default();
                    self.feed_html(&html);
                }
            }
        }
        if let Some((lang, text)) = self.code.as_mut() {
            match event {
                Event::Text(chunk) => text.push_str(&chunk),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, mut text) = (std::mem::take(lang), std::mem::take(text));
                    self.code = None;
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    self.emit(Block::Code { lang, text });
                }
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.markdown_text(&text),
            Event::Code(code) => {
                self.inline.push_fmt("md-code", CODE_OPEN, "</span>");
                self.push_text(&code);
                self.inline.pop_fmt("md-code");
            }
            Event::Html(html) | Event::InlineHtml(html) => self.feed_html(&html),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.inline.push(Run::Break),
            Event::Rule => {
                self.flush();
                self.emit(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Container {
                    kind: Kind::Item(state),
                    ..
                }) = self.containers.last_mut()
                {
                    *state = Some(checked);
                }
            }
            Event::FootnoteReference(name) => self.push_text(&format!("[^{name}]")),
            Event::InlineMath(text) | Event::DisplayMath(text) => self.push_text(&text),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.flush(),
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level as u8);
            }
            Tag::BlockQuote(kind) => self.open(Kind::Quote(kind.map(|kind| match kind {
                BlockQuoteKind::Note => AlertKind::Note,
                BlockQuoteKind::Tip => AlertKind::Tip,
                BlockQuoteKind::Important => AlertKind::Important,
                BlockQuoteKind::Warning => AlertKind::Warning,
                BlockQuoteKind::Caution => AlertKind::Caution,
            }))),
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split([' ', ',', '{'])
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => {
                self.flush();
                self.html = Some(String::new());
            }
            Tag::List(start) => self.open(Kind::List {
                start,
                items: Vec::new(),
            }),
            Tag::Item => self.open(Kind::Item(None)),
            Tag::Table(aligns) => {
                self.flush();
                self.table = Some(Table {
                    aligns: aligns
                        .iter()
                        .map(|align| match align {
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::End,
                            Alignment::Left | Alignment::None => Align::Start,
                        })
                        .collect(),
                    ..Default::default()
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.row.clear();
                }
            }
            Tag::Emphasis => self.inline.push_fmt("md-em", "<i>", "</i>"),
            Tag::Strong => self.inline.push_fmt("md-strong", "<b>", "</b>"),
            Tag::Strikethrough => self.inline.push_fmt("md-del", "<s>", "</s>"),
            Tag::Superscript => self.inline.push_fmt("md-sup", "<sup>", "</sup>"),
            Tag::Subscript => self.inline.push_fmt("md-sub", "<sub>", "</sub>"),
            Tag::Link { dest_url, .. } => {
                let href = self.base.link(&dest_url);
                self.inline.push_link("md-link", Some(href));
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some(self.image_for(&dest_url));
            }
            _ => self.flush(),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                let level = self.heading.take();
                self.flush_as(level);
            }
            TagEnd::BlockQuote(_) => self.close(|kind| matches!(kind, Kind::Quote(_))),
            TagEnd::List(_) => self.close(|kind| matches!(kind, Kind::List { .. })),
            TagEnd::Item => self.close(|kind| matches!(kind, Kind::Item(_))),
            TagEnd::TableCell => {
                let runs = self.inline.take();
                if let Some(table) = self.table.as_mut() {
                    table.row.push(runs_markup(runs));
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.header = std::mem::take(&mut table.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.emit(Block::Table {
                        aligns: table.aligns,
                        header: table.header,
                        rows: table.rows,
                    });
                }
            }
            TagEnd::Emphasis => self.inline.pop_fmt("md-em"),
            TagEnd::Strong => self.inline.pop_fmt("md-strong"),
            TagEnd::Strikethrough => self.inline.pop_fmt("md-del"),
            TagEnd::Superscript => self.inline.pop_fmt("md-sup"),
            TagEnd::Subscript => self.inline.pop_fmt("md-sub"),
            TagEnd::Link => self.inline.pop_fmt("md-link"),
            TagEnd::Image => {
                if let Some(image) = self.image.take() {
                    self.inline.push(Run::Image(image));
                }
            }
            _ => self.flush(),
        }
    }

    fn image_for(&self, src: &str) -> Image {
        let mut image = Image {
            src: self.base.image(src),
            link: self.inline.link(),
            ..Default::default()
        };
        if image.src.ends_with("#gh-dark-mode-only") {
            image.scheme = Scheme::Dark;
        } else if image.src.ends_with("#gh-light-mode-only") {
            image.scheme = Scheme::Light;
        }
        image
    }

    fn push_text(&mut self, text: &str) {
        if let Some(image) = self.image.as_mut() {
            image.alt.push_str(text);
        } else if self.skip == 0 {
            self.inline.push_text(&escape(text));
        }
    }

    /// Markdown text: shortcodes become emoji, bare URLs links.
    fn markdown_text(&mut self, text: &str) {
        if self.image.is_some() {
            return self.push_text(text);
        }
        let text = emoji(text);
        if self.inline.link().is_some() {
            return self.push_text(&text);
        }
        for (piece, is_url) in autolinks(&text) {
            if is_url {
                self.inline
                    .push_link("md-autolink", Some(piece.to_string()));
                self.push_text(piece);
                self.inline.pop_fmt("md-autolink");
            } else {
                self.push_text(piece);
            }
        }
    }

    fn html_text(&mut self, text: &str) {
        // HTML whitespace: any run of it is one space.
        let text = decode_entities(text);
        let words: Vec<&str> = text.split_ascii_whitespace().collect();
        let mut collapsed = String::with_capacity(text.len());
        if text.starts_with(|ch: char| ch.is_ascii_whitespace()) {
            collapsed.push(' ');
        }
        collapsed.push_str(&words.join(" "));
        if !words.is_empty() && text.ends_with(|ch: char| ch.is_ascii_whitespace()) {
            collapsed.push(' ');
        }
        self.push_text(&collapsed);
    }

    fn feed_html(&mut self, html: &str) {
        for token in tokenize(html) {
            match token {
                Token::Open { name, .. } if self.skip > 0 => {
                    if matches!(name.as_str(), "script" | "style") {
                        self.skip += 1;
                    }
                }
                Token::Close(name) if self.skip > 0 => {
                    if matches!(name.as_str(), "script" | "style") {
                        self.skip -= 1;
                    }
                }
                Token::Text(_) if self.skip > 0 => {}
                Token::Text(text) => self.html_text(&text),
                Token::Open { name, attrs } => self.open_tag(&name, &attrs),
                Token::Close(name) => self.close_tag(&name),
            }
        }
    }

    fn open_tag(&mut self, name: &str, attrs: &[(String, String)]) {
        let attr = |key: &str| Token::attr(attrs, key);
        match name {
            "script" | "style" => self.skip += 1,
            "img" => {
                let picture = self.picture.as_ref();
                let src = attr("src")
                    .filter(|src| !src.is_empty())
                    .map(str::to_string)
                    .or_else(|| picture.and_then(|picture| picture.light.clone()));
                let Some(src) = src else { return };
                let dark = picture.and_then(|picture| picture.dark.clone());
                let mut image = self.image_for(&src);
                image.dark_src = dark;
                image.alt = attr("alt").unwrap_or_default().to_string();
                image.width = attr("width").and_then(pixels);
                image.height = attr("height").and_then(pixels);
                self.inline.push(Run::Image(image));
            }
            "source" => {
                let srcset = attr("srcset")
                    .and_then(|set| set.split([' ', ',']).find(|url| !url.is_empty()))
                    .map(|url| self.base.image(url));
                if let (Some(picture), Some(url)) = (self.picture.as_mut(), srcset) {
                    if attr("media").is_some_and(|media| media.contains("dark")) {
                        picture.dark = Some(url);
                    } else {
                        picture.light = Some(url);
                    }
                }
            }
            "picture" => self.picture = Some(Picture::default()),
            "br" => self.inline.push(Run::Break),
            "hr" => {
                self.flush();
                self.emit(Block::Rule);
            }
            "a" => {
                let href = attr("href").map(|href| self.base.link(href));
                self.inline.push_link("a", href);
            }
            "b" | "strong" => self.inline.push_fmt(name, "<b>", "</b>"),
            "i" | "em" | "cite" | "var" => self.inline.push_fmt(name, "<i>", "</i>"),
            "s" | "del" | "strike" => self.inline.push_fmt(name, "<s>", "</s>"),
            "u" | "ins" => self.inline.push_fmt(name, "<u>", "</u>"),
            "code" | "kbd" | "tt" | "samp" => self.inline.push_fmt(name, CODE_OPEN, "</span>"),
            "sub" => self.inline.push_fmt(name, "<sub>", "</sub>"),
            "sup" => self.inline.push_fmt(name, "<sup>", "</sup>"),
            "small" => self.inline.push_fmt(name, "<small>", "</small>"),
            "p" | "div" | "center" | "section" | "header" | "footer" | "article" | "figure" => {
                self.block_open(name, attr("align"));
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.block_open(name, attr("align"));
                self.heading = name[1..].parse().ok();
            }
            "details" => self.open(Kind::Details {
                summary: String::new(),
                open: attr("open").is_some(),
            }),
            "summary" => {
                self.flush();
                self.summary = true;
            }
            "li" => {
                self.flush();
                self.push_text("• ");
            }
            "ul" | "ol" | "table" | "thead" | "tbody" | "tr" | "td" | "th" | "dl" | "dt" | "dd"
            | "pre" | "blockquote" => self.flush(),
            _ => {}
        }
    }

    fn close_tag(&mut self, name: &str) {
        match name {
            "a" | "b" | "strong" | "i" | "em" | "cite" | "var" | "s" | "del" | "strike" | "u"
            | "ins" | "code" | "kbd" | "tt" | "samp" | "sub" | "sup" | "small" => {
                self.inline.pop_fmt(name);
            }
            "p" | "div" | "center" | "section" | "header" | "footer" | "article" | "figure" => {
                self.flush();
                self.block_close(name);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = self.heading.take();
                self.flush_as(level);
                self.block_close(name);
            }
            "details" => self.close(|kind| matches!(kind, Kind::Details { .. })),
            "summary" if self.summary => {
                self.summary = false;
                let summary = runs_markup(self.inline.take());
                if let Some(Container {
                    kind: Kind::Details { summary: slot, .. },
                    ..
                }) = self.containers.last_mut()
                {
                    *slot = summary;
                }
            }
            "picture" => self.picture = None,
            "li" | "ul" | "ol" | "table" | "thead" | "tbody" | "tr" | "td" | "th" | "dl" | "dt"
            | "dd" | "pre" | "blockquote" => self.flush(),
            _ => {}
        }
    }

    fn block_open(&mut self, name: &str, align: Option<&str>) {
        self.flush();
        let align = match align.map(str::to_ascii_lowercase).as_deref() {
            Some("center") => Some(Align::Center),
            Some("right") => Some(Align::End),
            Some("left") => Some(Align::Start),
            _ if name == "center" => Some(Align::Center),
            _ => None,
        };
        if let Some(align) = align {
            self.aligns.push(align);
        }
        self.html_open.push((name.to_string(), align.is_some()));
    }

    fn block_close(&mut self, name: &str) {
        let Some(at) = self.html_open.iter().rposition(|(open, _)| open == name) else {
            return;
        };
        for (_, pushed) in self.html_open.drain(at..) {
            if pushed && self.aligns.len() > 1 {
                self.aligns.pop();
            }
        }
    }

    fn align(&self) -> Align {
        self.aligns.last().copied().unwrap_or_default()
    }

    fn emit(&mut self, block: Block) {
        if let Some(container) = self.containers.last_mut() {
            container.blocks.push(block);
        }
    }

    fn flush(&mut self) {
        self.flush_as(None);
    }

    /// The paragraph so far as blocks: text as a paragraph (or a heading),
    /// images as rows of their own.
    fn flush_as(&mut self, heading: Option<u8>) {
        if self.summary || self.table.is_some() {
            return;
        }
        let runs = self.inline.take();
        let mut text = String::new();
        let mut images = Vec::new();
        for run in runs {
            match run {
                Run::Text(markup) => {
                    if images.is_empty() || !is_blank(&markup) {
                        self.emit_images(&mut images);
                        text.push_str(&markup);
                    }
                }
                Run::Image(image) => {
                    self.emit_text(&mut text, heading);
                    images.push(image);
                }
                Run::Break if !images.is_empty() => self.emit_images(&mut images),
                Run::Break => text.push('\n'),
            }
        }
        self.emit_images(&mut images);
        self.emit_text(&mut text, heading);
    }

    fn emit_images(&mut self, images: &mut Vec<Image>) {
        if !images.is_empty() {
            let align = self.align();
            self.emit(Block::Images {
                images: std::mem::take(images),
                align,
            });
        }
    }

    fn emit_text(&mut self, text: &mut String, heading: Option<u8>) {
        let markup = std::mem::take(text);
        if is_blank(&markup) {
            return;
        }
        let markup = markup.trim().to_string();
        let align = self.align();
        let block = match heading {
            Some(level) => {
                let mut anchor = slug(&plain(&markup));
                let seen = self.anchors.entry(anchor.clone()).or_insert(0);
                if *seen > 0 {
                    anchor = format!("{anchor}-{seen}");
                }
                *seen += 1;
                Block::Heading {
                    level,
                    markup,
                    anchor,
                    align,
                }
            }
            None => Block::Paragraph { markup, align },
        };
        self.emit(block);
    }

    fn open(&mut self, kind: Kind) {
        self.flush();
        self.containers.push(Container {
            kind,
            blocks: Vec::new(),
        });
    }

    /// Close the innermost container `is_it` matches, and any left open
    /// inside it.
    fn close(&mut self, is_it: impl Fn(&Kind) -> bool) {
        let Some(at) = self.containers.iter().rposition(|c| is_it(&c.kind)) else {
            return;
        };
        if at == 0 {
            return;
        }
        self.flush();
        while self.containers.len() > at {
            let container = self.containers.pop().expect("a container");
            self.fold(container);
        }
    }

    /// A closed container, as a block of its parent.
    fn fold(&mut self, container: Container) {
        let blocks = container.blocks;
        match container.kind {
            Kind::Root => {}
            Kind::Quote(None) => self.emit(Block::Quote(blocks)),
            Kind::Quote(Some(kind)) => self.emit(Block::Alert { kind, blocks }),
            Kind::List { start, items } => {
                self.emit(Block::List { start, items });
                for block in blocks {
                    self.emit(block);
                }
            }
            Kind::Item(checked) => match self.containers.last_mut() {
                Some(Container {
                    kind: Kind::List { items, .. },
                    ..
                }) => items.push(Item { checked, blocks }),
                _ => {
                    for block in blocks {
                        self.emit(block);
                    }
                }
            },
            Kind::Details { summary, open } => self.emit(Block::Details {
                summary,
                open,
                blocks,
            }),
        }
    }
}

fn pixels(value: &str) -> Option<u32> {
    value.trim().trim_end_matches("px").parse().ok()
}

fn is_blank(markup: &str) -> bool {
    plain(markup).trim().is_empty()
}

/// Runs as one piece of markup, for a table cell or a summary: images by
/// their alt text.
fn runs_markup(runs: Vec<Run>) -> String {
    let mut markup = String::new();
    for run in runs {
        match run {
            Run::Text(text) => markup.push_str(&text),
            Run::Image(image) => markup.push_str(&escape(&image.alt)),
            Run::Break => markup.push('\n'),
        }
    }
    markup.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Base {
        Base {
            html: "https://github.com/o/r/blob/main/".into(),
            raw: "https://raw.githubusercontent.com/o/r/main/".into(),
            dir: String::new(),
        }
    }

    fn blocks(markdown: &str) -> Vec<Block> {
        parse(markdown, &base())
    }

    #[test]
    fn links_and_images_resolve_like_github() {
        let base = Base {
            dir: "docs/".into(),
            ..base()
        };
        assert_eq!(
            base.link("../LICENSE"),
            "https://github.com/o/r/blob/main/LICENSE"
        );
        assert_eq!(
            base.link("/src/main.rs#L3"),
            "https://github.com/o/r/blob/main/src/main.rs#L3"
        );
        assert_eq!(base.link("#install"), "#install");
        assert_eq!(base.link("https://x.org/a"), "https://x.org/a");
        assert_eq!(base.link("mailto:a@b.c"), "mailto:a@b.c");
        assert_eq!(
            base.image("./shot.png"),
            "https://raw.githubusercontent.com/o/r/main/docs/shot.png"
        );
        assert_eq!(
            base.image("https://github.com/o/r/blob/main/a.png?raw=true"),
            "https://raw.githubusercontent.com/o/r/main/a.png"
        );
        assert_eq!(
            base.image("https://github.com/user-attachments/assets/abc"),
            "https://github.com/user-attachments/assets/abc"
        );
    }

    #[test]
    fn markdown_becomes_blocks() {
        let parsed = blocks(
            "# Hello *world*\n\nSome **bold** & `code`.\n\n- one\n- [x] two\n\n```sh\nmake\n```\n",
        );
        assert_eq!(
            parsed[0],
            Block::Heading {
                level: 1,
                markup: "Hello <i>world</i>".into(),
                anchor: "hello-world".into(),
                align: Align::Start,
            }
        );
        assert_eq!(
            parsed[1],
            Block::Paragraph {
                markup: format!("Some <b>bold</b> &amp; {CODE_OPEN}code</span>."),
                align: Align::Start,
            }
        );
        let Block::List { start, items } = &parsed[2] else {
            panic!("a list: {:?}", parsed[2]);
        };
        assert_eq!(*start, None);
        assert_eq!(items[0].checked, None);
        assert_eq!(items[1].checked, Some(true));
        assert_eq!(
            parsed[3],
            Block::Code {
                lang: "sh".into(),
                text: "make".into()
            }
        );
    }

    #[test]
    fn a_centred_html_header_with_badges() {
        let parsed = blocks(concat!(
            "<p align=\"center\">\n",
            "  <img src=\"assets/logo.png\" width=\"200\" alt=\"Logo\">\n",
            "</p>\n\n",
            "<h1 align=\"center\">Thing</h1>\n\n",
            "<div align=\"center\">\n\n",
            "[![CI](https://img.shields.io/ci.svg)](https://ci) [![MIT](https://img.shields.io/mit.svg)](LICENSE)\n\n",
            "</div>\n\n",
            "After.\n",
        ));
        let Block::Images { images, align } = &parsed[0] else {
            panic!("images: {:?}", parsed[0]);
        };
        assert_eq!(*align, Align::Center);
        assert_eq!(
            images[0].src,
            "https://raw.githubusercontent.com/o/r/main/assets/logo.png"
        );
        assert_eq!(images[0].width, Some(200));
        assert_eq!(images[0].alt, "Logo");
        assert!(matches!(
            &parsed[1],
            Block::Heading { markup, align: Align::Center, .. } if markup == "Thing"
        ));
        let Block::Images { images, align } = &parsed[2] else {
            panic!("badges: {:?}", parsed[2]);
        };
        assert_eq!(*align, Align::Center);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].link.as_deref(), Some("https://ci"));
        assert_eq!(
            images[1].link.as_deref(),
            Some("https://github.com/o/r/blob/main/LICENSE")
        );
        assert!(images.iter().all(Image::is_badge));
        assert_eq!(
            parsed[3],
            Block::Paragraph {
                markup: "After.".into(),
                align: Align::Start,
            }
        );
        assert!(has_pictures(&parsed));
    }

    #[test]
    fn alerts_details_and_tables() {
        let parsed = blocks(concat!(
            "> [!WARNING]\n> Back up first.\n\n",
            "<details>\n<summary>More</summary>\n\nHidden *text*.\n\n</details>\n\n",
            "| a | b |\n|:-:|--:|\n| 1 | **2** |\n",
        ));
        assert!(matches!(
            &parsed[0],
            Block::Alert { kind: AlertKind::Warning, blocks } if blocks.len() == 1
        ));
        assert_eq!(
            parsed[1],
            Block::Details {
                summary: "More".into(),
                open: false,
                blocks: vec![Block::Paragraph {
                    markup: "Hidden <i>text</i>.".into(),
                    align: Align::Start,
                }],
            }
        );
        assert_eq!(
            parsed[2],
            Block::Table {
                aligns: vec![Align::Center, Align::End],
                header: vec!["a".into(), "b".into()],
                rows: vec![vec!["1".into(), "<b>2</b>".into()]],
            }
        );
    }

    #[test]
    fn markup_stays_balanced_across_html_and_images() {
        let parsed = blocks("<b>bold <img src=\"x.png\"> still</b> plain <i>open\n");
        for block in &parsed {
            if let Block::Paragraph { markup, .. } = block {
                assert_eq!(
                    markup.matches("<b>").count(),
                    markup.matches("</b>").count()
                );
                assert_eq!(
                    markup.matches("<i>").count(),
                    markup.matches("</i>").count()
                );
            }
        }
        assert!(matches!(&parsed[0], Block::Paragraph { markup, .. } if markup == "<b>bold </b>"));
        assert!(matches!(&parsed[1], Block::Images { .. }));
        assert!(
            matches!(&parsed[2], Block::Paragraph { markup, .. } if markup == "<b>still</b> plain <i>open</i>")
        );
    }

    #[test]
    fn pictures_and_scheme_only_images() {
        let parsed = blocks(concat!(
            "<picture>\n",
            "  <source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.png\">\n",
            "  <img src=\"light.png\" alt=\"shot\">\n",
            "</picture>\n\n",
            "![a](d.png#gh-dark-mode-only)\n",
        ));
        let Block::Images { images, .. } = &parsed[0] else {
            panic!("{parsed:?}");
        };
        assert_eq!(
            images[0].src_for(true),
            "https://raw.githubusercontent.com/o/r/main/dark.png"
        );
        assert_eq!(
            images[0].src_for(false),
            "https://raw.githubusercontent.com/o/r/main/light.png"
        );
        let Block::Images { images, .. } = &parsed[1] else {
            panic!("{parsed:?}");
        };
        assert_eq!(images[0].scheme, Scheme::Dark);
    }

    #[test]
    fn headings_get_githubs_anchors() {
        let parsed = blocks("## Install & Use!\n\n## Install & Use!\n\n[go](#install--use)\n");
        let anchors: Vec<_> = parsed
            .iter()
            .filter_map(|block| match block {
                Block::Heading { anchor, .. } => Some(anchor.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(anchors, ["install--use", "install--use-1"]);
        assert!(
            matches!(&parsed[2], Block::Paragraph { markup, .. } if markup == "<a href=\"#install--use\">go</a>")
        );
    }

    #[test]
    fn bare_urls_and_shortcodes() {
        let parsed = blocks("See https://example.com/a_(b). :rocket: :nope:\n");
        assert_eq!(
            parsed[0],
            Block::Paragraph {
                markup: "See <a href=\"https://example.com/a_(b)\">https://example.com/a_(b)</a>. 🚀 :nope:".into(),
                align: Align::Start,
            }
        );
    }

    #[test]
    fn html_comments_and_entities() {
        let parsed = blocks("<!-- hidden -->\n\n<p>a&nbsp;&amp;&#x41;</p>\n");
        assert_eq!(
            parsed,
            vec![Block::Paragraph {
                markup: "a\u{a0}&amp;A".into(),
                align: Align::Start,
            }]
        );
    }

    #[test]
    fn nested_lists_keep_their_items() {
        let parsed = blocks("1. one\n   - inner\n2. two\n");
        let Block::List { start, items } = &parsed[0] else {
            panic!("{parsed:?}");
        };
        assert_eq!(*start, Some(1));
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].blocks[1], Block::List { .. }));
    }
}
