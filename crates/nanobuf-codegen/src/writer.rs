/// Indentation-aware text builder used by all backends.
pub struct CodeWriter {
    out: String,
    indent: usize,
    unit: &'static str,
}

impl CodeWriter {
    /// `unit` is one indentation step, e.g. `"    "`.
    pub fn new(unit: &'static str) -> Self {
        CodeWriter {
            out: String::new(),
            indent: 0,
            unit,
        }
    }

    pub fn line(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        if text.is_empty() {
            self.out.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.out.push_str(self.unit);
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    pub fn blank(&mut self) {
        self.out.push('\n');
    }

    /// Writes a line and indents the following ones.
    pub fn open(&mut self, text: impl AsRef<str>) {
        self.line(text);
        self.indent += 1;
    }

    /// Dedents and writes a closing line.
    pub fn close(&mut self, text: impl AsRef<str>) {
        self.indent -= 1;
        self.line(text);
    }

    /// Emits doc comment lines with the given comment leader.
    pub fn doc(&mut self, docs: &[String], leader: &str) {
        for doc in docs {
            self.line(format!("{leader} {doc}"));
        }
    }

    pub fn finish(self) -> String {
        debug_assert_eq!(self.indent, 0, "unbalanced open/close");
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_and_dedents() {
        let mut w = CodeWriter::new("  ");
        w.open("fn main() {");
        w.line("body();");
        w.blank();
        w.close("}");
        assert_eq!(w.finish(), "fn main() {\n  body();\n\n}\n");
    }

    #[test]
    fn docs_use_leader() {
        let mut w = CodeWriter::new("    ");
        w.doc(&["First.".into(), "Second.".into()], "///");
        assert_eq!(w.finish(), "/// First.\n/// Second.\n");
    }
}
