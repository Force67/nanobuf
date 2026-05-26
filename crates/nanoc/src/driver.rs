//! Loads a schema file and its transitive imports into parsed units.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use nanobuf_diagnostics::{Diagnostic, Diagnostics, FileId, SourceMap, Span};
use nanobuf_schema::Unit;

/// Parses `root` and everything it imports, breadth-first, so the root's
/// package comes first in generated output. Import paths resolve relative
/// to the importing file. Files are loaded once each; cycles are fine.
pub fn load(root: &Path, sources: &mut SourceMap, diags: &mut Diagnostics) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<(PathBuf, Option<(FileId, Span)>)> =
        VecDeque::from([(root.to_path_buf(), None)]);

    while let Some((path, imported_from)) = queue.pop_front() {
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(key) {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                let mut diag =
                    Diagnostic::error("E0300", format!("cannot read `{}`: {err}", path.display()));
                if let Some((file, span)) = imported_from {
                    diag = diag.with_label(file, span, "imported here");
                }
                diags.push(diag);
                continue;
            }
        };
        let file = sources.add(path.display().to_string(), text.clone());
        let Some(ast) = nanobuf_syntax::parse(file, &text, diags) else {
            continue;
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        for import in &ast.imports {
            queue.push_back((dir.join(&import.path), Some((file, import.span))));
        }
        units.push(Unit { file, ast });
    }
    units
}
