//! A small, deterministic document printer. Width is a target, not a limit on
//! literal data or comments.
#[derive(Clone, Debug)]
pub enum Document {
    Text(String),
    Suffix(String),
    FlatText(&'static str),
    BrokenText(&'static str),
    Line(&'static str),
    ConditionalLine,
    Branch(Box<Document>),
    Conditional {
        inner: Box<Document>,
        continuation: bool,
    },
    HardLine,
    BlankLine,
    Concat(Vec<Document>),
    Indent(Box<Document>),
    Align(Box<Document>),
    Group(Box<Document>),
}

impl Document {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }
    pub fn concat(parts: impl IntoIterator<Item = Self>) -> Self {
        Self::Concat(parts.into_iter().collect())
    }
    pub fn indent(self) -> Self {
        Self::Indent(Box::new(self))
    }
    pub fn group(self) -> Self {
        Self::Group(Box::new(self))
    }
    pub fn flat_width(&self) -> Option<usize> {
        match self {
            Self::BrokenText(_) => Some(0),
            Self::FlatText(text) => Some(text.len()),
            Self::Suffix(_) => Some(0),
            Self::Text(text) if !text.contains(['\n', '\r']) => Some(text.chars().count()),
            Self::Text(_) | Self::HardLine | Self::BlankLine => None,
            Self::Line(flat) => Some(flat.len()),
            Self::ConditionalLine => Some(1),
            Self::Concat(parts) => {
                let mut width = 0usize;
                let mut ended = false;
                for part in parts {
                    let next = part.flat_width()?;
                    if ended && next > 0 {
                        return None;
                    }
                    width = width.checked_add(next)?;
                    ended |= part.ends_line();
                }
                Some(width)
            }
            Self::Indent(inner)
            | Self::Align(inner)
            | Self::Branch(inner)
            | Self::Group(inner)
            | Self::Conditional { inner, .. } => inner.flat_width(),
        }
    }
    fn ends_line(&self) -> bool {
        match self {
            Self::Suffix(_) | Self::HardLine | Self::BlankLine => true,
            Self::Concat(parts) => parts.iter().any(Self::ends_line),
            Self::Indent(inner)
            | Self::Align(inner)
            | Self::Branch(inner)
            | Self::Group(inner)
            | Self::Conditional { inner, .. } => inner.ends_line(),
            _ => false,
        }
    }
}

pub fn render(document: &Document) -> String {
    let mut printer = Printer {
        output: String::new(),
        column: 0,
        pending_indent: 0,
        conditional_flat: None,
        force_branch_break: false,
        branch_broke: false,
    };
    printer.write(document, 0, false, 0);
    if !printer.output.ends_with('\n') {
        printer.output.push('\n');
    }
    printer.output
}

struct Printer {
    output: String,
    column: usize,
    pending_indent: usize,
    conditional_flat: Option<bool>,
    force_branch_break: bool,
    branch_broke: bool,
}

impl Printer {
    fn write(&mut self, document: &Document, indent: usize, flat: bool, following: usize) {
        match document {
            Document::Conditional {
                inner,
                continuation,
            } => {
                self.conditional(inner, *continuation, indent, following);
            }
            Document::Branch(inner) => {
                let fits = !self.force_branch_break
                    && inner.flat_width().is_some_and(|width| {
                        self.column.max(self.pending_indent) + width + following <= 100
                    });
                self.branch_broke |= !fits;
                self.write(inner, indent, fits, following);
            }
            Document::ConditionalLine => {
                let separator = if self.conditional_flat.unwrap_or(flat) {
                    Document::text(" ")
                } else {
                    Document::HardLine
                };
                self.write(&separator, indent, flat, following);
            }
            Document::BrokenText(text) => {
                if !flat {
                    self.text(text);
                }
            }
            Document::FlatText(text) => {
                if flat {
                    self.text(text);
                }
            }
            Document::Text(text) => self.text(text),
            Document::Suffix(text) => {
                self.text(" ");
                self.text(text);
                self.output.push('\n');
                self.column = 0;
                self.pending_indent = indent;
            }
            Document::Line(text) if flat => self.text(text),
            Document::BlankLine => {
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                if !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                self.column = 0;
                self.pending_indent = indent;
            }
            Document::Line(_) | Document::HardLine => {
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.column = 0;
                self.pending_indent = indent;
            }
            Document::Concat(parts) => {
                for (index, part) in parts.iter().enumerate() {
                    let (width, ends_line) = first_line(&parts[index + 1..], flat);
                    self.write(
                        part,
                        indent,
                        flat,
                        width + if ends_line { 0 } else { following },
                    );
                }
            }
            Document::Indent(inner) => self.write(inner, indent + 4, flat, following),
            Document::Align(inner) => {
                let column = self.column.max(self.pending_indent);
                self.write(inner, column, flat, following);
            }
            Document::Group(inner) => {
                let column = self.column.max(self.pending_indent);
                let fits = inner
                    .flat_width()
                    .is_some_and(|width| column + width + following <= 100);
                self.write(inner, indent, fits, following);
            }
        }
    }

    fn conditional(
        &mut self,
        inner: &Document,
        continuation: bool,
        indent: usize,
        following: usize,
    ) {
        if continuation && let Some(flat) = self.conditional_flat {
            self.write(inner, indent, flat, following);
            return;
        }
        let previous = (
            self.conditional_flat,
            self.force_branch_break,
            self.branch_broke,
        );
        let checkpoint = (self.output.len(), self.column, self.pending_indent);
        let fits = inner
            .flat_width()
            .is_some_and(|width| self.column.max(self.pending_indent) + width + following <= 100);
        self.conditional_flat = Some(fits);
        self.force_branch_break = false;
        self.branch_broke = false;
        // First measure branches at their actual columns. If one breaks, print
        // the entire chain again with all branch bodies on indented lines.
        self.write(inner, indent, fits, following);
        if self.branch_broke {
            self.output.truncate(checkpoint.0);
            self.column = checkpoint.1;
            self.pending_indent = checkpoint.2;
            self.conditional_flat = Some(false);
            self.force_branch_break = true;
            self.write(inner, indent, false, following);
        }
        (
            self.conditional_flat,
            self.force_branch_break,
            self.branch_broke,
        ) = previous;
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.column == 0 {
            self.output
                .extend(std::iter::repeat_n(' ', self.pending_indent));
            self.column = self.pending_indent;
            self.pending_indent = 0;
        }
        self.output.push_str(text);
        self.column = text
            .rsplit_once('\n')
            .map_or(self.column + text.chars().count(), |(_, last)| {
                last.chars().count()
            });
    }
}

fn first_line(parts: &[Document], flat: bool) -> (usize, bool) {
    let mut total = 0;
    for part in parts {
        let (width, ends_line) = match part {
            Document::Text(text) => text
                .split_once('\n')
                .map_or((text.chars().count(), false), |(first, _)| {
                    (first.chars().count(), true)
                }),
            Document::Suffix(_) | Document::HardLine | Document::BlankLine => (0, true),
            Document::ConditionalLine => (if flat { 1 } else { 0 }, !flat),
            Document::Line(text) => {
                if flat {
                    (text.len(), false)
                } else {
                    (0, true)
                }
            }
            Document::FlatText(text) => (if flat { text.len() } else { 0 }, false),
            Document::BrokenText(text) => (if flat { 0 } else { text.len() }, false),
            Document::Concat(children) => first_line(children, flat),
            Document::Indent(inner)
            | Document::Align(inner)
            | Document::Branch(inner)
            | Document::Group(inner)
            | Document::Conditional { inner, .. } => {
                first_line(std::slice::from_ref(inner.as_ref()), flat)
            }
        };
        total += width;
        if ends_line {
            return (total, true);
        }
    }
    (total, false)
}
