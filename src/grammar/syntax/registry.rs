use std::collections::HashMap;

use crate::config::ast::ParserDecl;
use crate::config::error::ConfigError;

use super::{CompiledParser, Context, Registration};

pub struct DeclaredParser {
    pub parser: CompiledParser,
    pub registration: Registration,
}

impl DeclaredParser {
    pub fn context(&self, flags: &[(String, bool)]) -> Result<Context, String> {
        let mut context = Context::default();
        let mut supplied = 0u64;
        for (name, enabled) in flags {
            let index = self.parser.flags().iter().position(|flag| flag == name)
                .ok_or_else(|| format!("unknown declared parser flag `{name}`"))?;
            let shift = u32::try_from(index).map_err(|_| "declared parser flag index exceeds context bits")?;
            let bit = 1u64.checked_shl(shift).ok_or("declared parser flag index exceeds context bits")?;
            if supplied & bit != 0 {
                return Err(format!("duplicate declared parser flag `{name}`"));
            }
            supplied |= bit;
            if *enabled { context.flags |= bit }
        }
        Ok(context)
    }

    fn compile(definition: &crate::pattern::syntax::Definition) -> Result<Self, String> {
        let parser = CompiledParser::compile(definition)?;
        let matchers = (0..parser.nodes().len()).map(u32::try_from).collect::<Result<Vec<_>, _>>()
            .map_err(|_| "declared parser kind count exceeds node ids")?;
        let labels = parser.nodes().iter().map(|node| {
            (0..node.fields.len()).map(u16::try_from).collect::<Result<Vec<_>, _>>()
        }).collect::<Result<Vec<_>, _>>().map_err(|_| "declared parser field count exceeds edge labels")?;
        Ok(Self { parser, registration: Registration { matchers, labels, text_matcher: crate::grammar::TEXT } })
    }
}

#[derive(Default)]
pub struct Parsers {
    entries: HashMap<String, DeclaredParser>,
}

impl Parsers {
    pub fn build(declarations: &[ParserDecl], errors: &mut Vec<ConfigError>) -> Self {
        let mut entries = HashMap::new();
        let mut names = HashMap::new();
        for declaration in declarations {
            if let Some(first) = names.get(&declaration.name.text) {
                errors.push(ConfigError::new(declaration.name.span, format!("duplicate declared parser `{}`", declaration.name.text))
                    .related(*first, "first declared here"));
                continue;
            }
            names.insert(declaration.name.text.clone(), declaration.name.span);
            match DeclaredParser::compile(&declaration.definition) {
                Ok(parser) => { entries.insert(declaration.name.text.clone(), parser); }
                Err(error) => errors.push(ConfigError::new(declaration.name.span, error)),
            }
        }
        Self { entries }
    }

    pub fn get(&self, name: &str) -> Option<&DeclaredParser> {
        self.entries.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::Parsers;
    use crate::config::source::Disk;

    fn declarations() -> Vec<crate::config::ast::ParserDecl> {
        let loaded = crate::config::load_text("value grammar", r#"
            parser toy in js {
                flag uppercase
                token lower /[a-z]+/
                token upper /[A-Z]+/
                line_break /[\r\n]/
                goal code { lower upper }
                node root { value: text }
                root entry = emit(root, value:if(uppercase, upper, lower) end)
            }
        "#, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        loaded.config.parsers
    }

    #[test]
    fn declared_value_parsers_keep_flags_local_and_reject_unknown_flags() {
        let mut errors = Vec::new();
        let parsers = Parsers::build(&declarations(), &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let parser = parsers.get("toy").expect("declared parser");
        let upper = parser.context(&[("uppercase".into(), true)]).unwrap();
        let lower = parser.context(&[]).unwrap();
        assert!(parser.parser.parse_root("ABC", upper, &parser.registration).is_ok());
        assert!(parser.parser.parse_root("abc", lower, &parser.registration).is_ok());
        assert!(parser.parser.parse_root("ABC", lower, &parser.registration).is_err());
        assert!(parser.context(&[("unknown".into(), true)]).is_err());
        assert!(parsers.get("missing").is_none());
    }

    #[test]
    fn duplicate_declarations_and_invalid_grammars_are_configuration_errors() {
        let mut declarations = declarations();
        declarations.push(declarations[0].clone());
        let mut errors = Vec::new();
        Parsers::build(&declarations, &mut errors);
        assert!(!errors.is_empty());
        errors.clear();
        declarations.truncate(1);
        declarations[0].definition.root = "missing".into();
        Parsers::build(&declarations, &mut errors);
        assert!(!errors.is_empty());
    }
}
