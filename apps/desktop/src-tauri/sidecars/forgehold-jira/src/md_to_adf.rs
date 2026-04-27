//! Markdown → Atlassian Document Format (ADF) converter for the Jira sidecar.
//!
//! Jira Cloud's REST API stores rich text as ADF, not markdown. The CLI agent
//! invariably writes markdown (that's what the chat history looks like and
//! what every agent is trained on), so we translate at the boundary instead
//! of asking the agent to author ADF JSON by hand.
//!
//! Coverage: paragraphs, headings 1-6, bulleted + ordered lists (with nesting),
//! blockquotes, fenced + indented code blocks, inline code, bold, italic,
//! strikethrough, links, hard breaks, horizontal rules. Tables and footnotes
//! fall through to a single paragraph (ADF supports tables but the schema is
//! verbose and rarely worth the round-trip).
//!
//! Returns a serde_json::Value shaped like:
//!     { "type": "doc", "version": 1, "content": [...] }
//!
//! Always returns a non-empty `content` (an empty paragraph node) so Jira's
//! description / body validators don't reject the payload.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::{Value, json};

pub fn markdown_to_adf(src: &str) -> Value {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return empty_doc();
    }

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES); // parsed; we render as one paragraph below
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(trimmed, opts);
    let mut state = State::new();

    for event in parser {
        state.handle(event);
    }

    let mut content = state.finish();
    if content.is_empty() {
        content.push(empty_paragraph());
    }

    json!({ "type": "doc", "version": 1, "content": content })
}

fn empty_doc() -> Value {
    json!({
        "type": "doc",
        "version": 1,
        "content": [empty_paragraph()],
    })
}

fn empty_paragraph() -> Value {
    json!({ "type": "paragraph" })
}

/// A list level we're currently inside — `ordered` controls the ADF node
/// type, `_start` is the 1st-item index for ordered lists (Jira ignores
/// it for now but we honour the source).
#[derive(Debug)]
struct ListFrame {
    ordered: bool,
    _start: u64,
    items: Vec<Value>,
}

#[derive(Debug)]
struct ItemFrame {
    blocks: Vec<Value>,
}

/// Currently-open block — drives where text events get appended. `Inlines`
/// owns a flat array of inline ADF nodes (text + hardBreak); we only push
/// onto the topmost block until it closes.
#[derive(Debug)]
enum Block {
    Paragraph(Vec<Value>),
    Heading { level: u8, content: Vec<Value> },
    BlockQuote { content: Vec<Value> },
    CodeBlock { language: Option<String>, text: String },
}

#[derive(Debug, Default, Clone)]
struct InlineMarks {
    bold: u32,
    italic: u32,
    strike: u32,
    code: u32,
    link: Option<String>,
}

impl InlineMarks {
    fn to_marks(&self) -> Vec<Value> {
        let mut marks: Vec<Value> = Vec::new();
        if self.bold > 0 {
            marks.push(json!({ "type": "strong" }));
        }
        if self.italic > 0 {
            marks.push(json!({ "type": "em" }));
        }
        if self.strike > 0 {
            marks.push(json!({ "type": "strike" }));
        }
        if self.code > 0 {
            marks.push(json!({ "type": "code" }));
        }
        if let Some(href) = &self.link {
            marks.push(json!({
                "type": "link",
                "attrs": { "href": href }
            }));
        }
        marks
    }
}

struct State {
    /// Top-level finished blocks (paragraphs, headings, lists, …).
    out: Vec<Value>,
    /// Currently open block stack — usually one entry; nested blockquotes /
    /// list items can stack a few.
    blocks: Vec<Block>,
    /// Active list contexts (outer-most first).
    lists: Vec<ListFrame>,
    /// Active list item being filled — items can contain block-level content.
    items: Vec<ItemFrame>,
    /// Whether we're currently inside a list item (controls whether finished
    /// blocks land in `items.last_mut().blocks` instead of `out`).
    in_item_depth: usize,
    /// Active inline mark stack — `Strong` toggles `marks.bold` etc.
    marks: InlineMarks,
}

impl State {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            blocks: Vec::new(),
            lists: Vec::new(),
            items: Vec::new(),
            in_item_depth: 0,
            marks: InlineMarks::default(),
        }
    }

    fn finish(mut self) -> Vec<Value> {
        // Flush any unclosed block that's still open.
        while let Some(b) = self.blocks.pop() {
            let node = block_to_json(b);
            self.push_block(node);
        }
        self.out
    }

    fn push_block(&mut self, node: Value) {
        if self.in_item_depth > 0 {
            if let Some(item) = self.items.last_mut() {
                item.blocks.push(node);
                return;
            }
        }
        self.out.push(node);
    }

    fn push_inline(&mut self, node: Value) {
        match self.blocks.last_mut() {
            Some(Block::Paragraph(c)) => c.push(node),
            Some(Block::Heading { content, .. }) => content.push(node),
            Some(Block::BlockQuote { content }) => content.push(node),
            Some(Block::CodeBlock { text, .. }) => {
                if let Some(t) = node.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                }
            }
            None => {
                // Inline outside any block — open an implicit paragraph.
                self.blocks.push(Block::Paragraph(vec![node]));
            }
        }
    }

    fn open(&mut self, b: Block) {
        self.blocks.push(b);
    }

    fn close(&mut self) -> Option<Value> {
        let b = self.blocks.pop()?;
        Some(block_to_json(b))
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                let s = t.into_string();
                // Code blocks accumulate raw text; everything else gets a
                // marked text node.
                if matches!(self.blocks.last(), Some(Block::CodeBlock { .. })) {
                    self.push_inline(json!({ "text": s }));
                } else {
                    self.push_inline(text_node(&s, &self.marks));
                }
            }
            Event::Code(c) => {
                let s = c.into_string();
                let mut marks = self.marks.clone();
                marks.code += 1;
                self.push_inline(text_node(&s, &marks));
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                // Drop raw HTML — ADF has no equivalent escape hatch and
                // smuggling it in via text mostly produces noise.
            }
            Event::SoftBreak => {
                // Source line wraps inside a paragraph become a single space
                // (Markdown semantics). Hard breaks (two trailing spaces)
                // come through as Event::HardBreak and produce a hardBreak
                // node instead.
                if matches!(self.blocks.last(), Some(Block::CodeBlock { .. })) {
                    self.push_inline(json!({ "text": "\n" }));
                } else {
                    self.push_inline(text_node(" ", &self.marks));
                }
            }
            Event::HardBreak => {
                if matches!(self.blocks.last(), Some(Block::CodeBlock { .. })) {
                    self.push_inline(json!({ "text": "\n" }));
                } else {
                    self.push_inline(json!({ "type": "hardBreak" }));
                }
            }
            Event::Rule => {
                self.push_block(json!({ "type": "rule" }));
            }
            Event::TaskListMarker(checked) => {
                let mark = if checked { "[x] " } else { "[ ] " };
                self.push_inline(text_node(mark, &self.marks));
            }
            Event::FootnoteReference(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                // No ADF equivalent; drop silently rather than corrupting
                // the doc with raw markup.
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.open(Block::Paragraph(Vec::new())),
            Tag::Heading { level, .. } => self.open(Block::Heading {
                level: heading_level(level),
                content: Vec::new(),
            }),
            Tag::BlockQuote(_) => self.open(Block::BlockQuote { content: Vec::new() }),
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(s) => {
                        let s = s.into_string();
                        if s.is_empty() { None } else { Some(s) }
                    }
                    CodeBlockKind::Indented => None,
                };
                self.open(Block::CodeBlock { language, text: String::new() });
            }
            Tag::List(start) => {
                let ordered = start.is_some();
                self.lists.push(ListFrame {
                    ordered,
                    _start: start.unwrap_or(1),
                    items: Vec::new(),
                });
            }
            Tag::Item => {
                self.items.push(ItemFrame { blocks: Vec::new() });
                self.in_item_depth += 1;
            }
            Tag::Strong => self.marks.bold += 1,
            Tag::Emphasis => self.marks.italic += 1,
            Tag::Strikethrough => self.marks.strike += 1,
            Tag::Link { dest_url, .. } => {
                self.marks.link = Some(dest_url.into_string());
            }
            Tag::Image { dest_url, .. } => {
                // No inline image support without media-storage uploads — fall
                // back to surfacing the URL as a link so the user still sees it.
                let url = dest_url.into_string();
                self.push_inline(text_node(&format!("[image: {}]", url), &self.marks));
            }
            Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell => {
                // Table support omitted — open an implicit paragraph so cell
                // text still flows somewhere.
                if !matches!(self.blocks.last(), Some(Block::Paragraph(_))) {
                    self.open(Block::Paragraph(Vec::new()));
                }
            }
            Tag::FootnoteDefinition(_) | Tag::DefinitionList | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition | Tag::HtmlBlock | Tag::MetadataBlock(_)
            | Tag::Superscript | Tag::Subscript => {
                // No first-class ADF equivalent — fall through to plain
                // paragraph/inline so the text content still surfaces.
                if !matches!(self.blocks.last(), Some(Block::Paragraph(_))) {
                    self.open(Block::Paragraph(Vec::new()));
                }
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock => {
                if let Some(node) = self.close() {
                    self.push_block(node);
                }
            }
            TagEnd::List(_) => {
                if let Some(frame) = self.lists.pop() {
                    let ty = if frame.ordered { "orderedList" } else { "bulletList" };
                    self.push_block(json!({
                        "type": ty,
                        "content": frame.items,
                    }));
                }
            }
            TagEnd::Item => {
                let item = self.items.pop().map(|f| f.blocks).unwrap_or_default();
                self.in_item_depth = self.in_item_depth.saturating_sub(1);
                let item_content = if item.is_empty() { vec![empty_paragraph()] } else { item };
                if let Some(list) = self.lists.last_mut() {
                    list.items.push(json!({
                        "type": "listItem",
                        "content": item_content,
                    }));
                }
            }
            TagEnd::Strong => self.marks.bold = self.marks.bold.saturating_sub(1),
            TagEnd::Emphasis => self.marks.italic = self.marks.italic.saturating_sub(1),
            TagEnd::Strikethrough => self.marks.strike = self.marks.strike.saturating_sub(1),
            TagEnd::Link => self.marks.link = None,
            _ => {}
        }
    }
}

fn block_to_json(b: Block) -> Value {
    match b {
        Block::Paragraph(content) => {
            if content.is_empty() {
                empty_paragraph()
            } else {
                json!({ "type": "paragraph", "content": content })
            }
        }
        Block::Heading { level, content } => json!({
            "type": "heading",
            "attrs": { "level": level.clamp(1, 6) },
            "content": content,
        }),
        Block::BlockQuote { content } => {
            // ADF blockquote children must be block nodes — wrap any inline
            // residue in a paragraph (mirrors how cmark emits the inner
            // paragraph for typical `> quote` lines, but covers the edge
            // case where quoted content was empty too).
            let inner = if content.is_empty() {
                vec![empty_paragraph()]
            } else if content.iter().any(|n| n.get("type").and_then(|t| t.as_str()) == Some("paragraph")) {
                content
            } else {
                vec![json!({ "type": "paragraph", "content": content })]
            };
            json!({ "type": "blockquote", "content": inner })
        }
        Block::CodeBlock { language, text } => {
            // Drop a single trailing newline (cmark's convention) so the
            // rendered block doesn't grow an extra blank line.
            let text = text.trim_end_matches('\n').to_string();
            let mut node = json!({
                "type": "codeBlock",
                "content": [{ "type": "text", "text": text }],
            });
            if let Some(lang) = language {
                node["attrs"] = json!({ "language": lang });
            }
            node
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn text_node(text: &str, marks: &InlineMarks) -> Value {
    let mut node = json!({ "type": "text", "text": text });
    let m = marks.to_marks();
    if !m.is_empty() {
        node["marks"] = Value::Array(m);
    }
    node
}
