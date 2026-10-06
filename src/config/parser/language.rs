//! Language-owned option declarations and scoped settings.

use super::{LanguageOptions, Parent, Parser, Tk};
use crate::config::error::ConfigError;

impl Parser<'_> {
    pub(super) fn language_options(&mut self) {
        let start = self.bump().expect("language item").span;
        let parents = self.parent_list();
        let [Parent::Lang(language)] = parents.as_slice() else {
            self.errors.push(ConfigError::new(start, "language settings need one language, like `.js`"));
            return;
        };
        let mut block = LanguageOptions {
            language: language.clone(), options: Vec::new(), settings: Vec::new(),
            files: Vec::new(), ignore: Vec::new(), span: start,
        };
        self.skip(false, true);
        if self.expect(Tk::LBrace, "`{` after the language").is_none() { return; }
        loop {
            let Some(token) = self.peek() else {
                self.errors.push(ConfigError::new(start, "unclosed language settings"));
                break;
            };
            if token.kind == Tk::RBrace {
                block.span = start.to(self.bump().expect("closing brace").span);
                break;
            }
            let before = self.errors.len();
            match (token.kind, &*token.text) {
                (Tk::Ident, "option") => {
                    self.bump();
                    if let Some(option) = self.option_line() {
                        block.options.push(option);
                    }
                }
                (Tk::Ident, "set") => {
                    self.bump();
                    if let Some(name) = self.ident("an option name") {
                        if let Some(value) = self.expr() {
                            block.settings.push((name, value));
                        }
                    }
                }
                (Tk::Ident, "files" | "ignore") => {
                    self.bump();
                    let globs = self.globs("a path glob");
                    if token.is_ident("files") { block.files.extend(globs); }
                    else { block.ignore.extend(globs); }
                }
                (Tk::Ident, _) if self.ts.expand(&mut self.errors) => continue,
                _ => {
                    self.errors.push(ConfigError::new(token.span, "language clauses are option, set, files and ignore"));
                    self.bump();
                }
            }
            if self.errors.len() > before { self.recover(); }
            else { self.end_of_item(); }
        }
        self.config.language_options.push(block);
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{load_text, source::Disk};

    #[test]
    fn language_options_accept_declarations_and_scoped_settings() {
        let loaded = load_text("test.zlint", r#"
lang js "*.js"
language .js {
  option globals { [key: string]: "readonly" | "writable" | "off" } = {}
  option sourceType "script" | "module" | "commonjs" | null = null
}
language .js {
  files "scripts/**"
  ignore "scripts/generated/**"
  set globals { app: "readonly" }
  set sourceType "commonjs"
}
"#, &Disk);
        assert!(loaded.errors.is_empty(), "{:#?}", loaded.errors);
    }

    #[test]
    fn invalid_language_clauses_preserve_following_items() {
        let loaded = load_text("test.zlint", r#"
lang js "*.js"
language .js { when true }
in .js rule later warn "present"
"#, &Disk);
        assert!(!loaded.errors.is_empty());
        assert!(loaded.config.rules.iter().any(|rule| rule.name.text == "later"));
    }
}
