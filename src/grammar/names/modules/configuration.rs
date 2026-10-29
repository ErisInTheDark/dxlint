use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::metadata::Json;
use super::resolution::{clean, spend, Host, Resolver};
use super::super::question::QueryContext;

#[derive(Clone)]
pub struct OptionValue {
    pub value: Json,
    pub directory: PathBuf,
}

#[derive(Clone, Default)]
pub struct Configuration {
    pub path: Option<PathBuf>,
    pub options: BTreeMap<String, OptionValue>,
    pub fields: BTreeMap<String, OptionValue>,
    pub references: Vec<PathBuf>,
}

impl Configuration {
    pub fn load<H: Host + ?Sized>(resolver: &Resolver<'_, H>, directory: &Path, question: &QueryContext) -> Result<Self, ()> {
        for ancestor in directory.ancestors() {
            spend(question)?;
            for name in &resolver.settings.config.files {
                let path = clean(&ancestor.join(&**name));
                if let Some(metadata) = resolver.metadata(&path, question)? {
                    return Self::inherit(resolver, path, &metadata, question);
                }
            }
        }
        Ok(Self::default())
    }

    pub fn at<H: Host + ?Sized>(resolver: &Resolver<'_, H>, path: PathBuf, question: &QueryContext) -> Result<Self, ()> {
        let metadata = resolver.metadata(&path, question)?.ok_or(())?;
        Self::inherit(resolver, path, &metadata, question)
    }

    fn inherit<H: Host + ?Sized>(resolver: &Resolver<'_, H>, path: PathBuf, metadata: &Json, question: &QueryContext) -> Result<Self, ()> {
        let mut memo = BTreeMap::<PathBuf, Option<Self>>::new();
        memo.insert(path.clone(), None);
        let root = path.clone();
        let mut stack = vec![Frame::new(resolver, path, metadata, question)?];
        while let Some(frame) = stack.last_mut() {
            spend(question)?;
            if let Some(path) = frame.parents.last() {
                match memo.get(path) {
                    Some(Some(config)) => {
                        frame.merged.options.extend(config.options.clone());
                        frame.merged.fields.extend(config.fields.clone());
                        frame.parents.pop();
                    }
                    Some(None) => return Err(()),
                    None => {
                        let path = path.clone();
                        let metadata = resolver.metadata(&path, question)?.ok_or(())?;
                        memo.insert(path.clone(), None);
                        stack.push(Frame::new(resolver, path, &metadata, question)?);
                    }
                }
                continue;
            }
            let mut frame = stack.pop().ok_or(())?;
            frame.merged.options.extend(frame.own);
            frame.merged.fields.extend(frame.fields);
            frame.merged.path = Some(frame.path.clone());
            frame.merged.references = frame.references;
            memo.insert(frame.path, Some(frame.merged));
        }
        memo.remove(&root).flatten().ok_or(())
    }

    pub fn base(&self, key: &str) -> Result<Option<PathBuf>, ()> {
        self.options.get(key).map(|value| {
            let relative = value.value.text().ok_or(())?;
            Ok(clean(&value.directory.join(relative)))
        }).transpose()
    }
}

struct Frame {
    path: PathBuf,
    parents: Vec<PathBuf>,
    own: BTreeMap<String, OptionValue>,
    fields: BTreeMap<String, OptionValue>,
    references: Vec<PathBuf>,
    merged: Configuration,
}

impl Frame {
    fn new<H: Host + ?Sized>(resolver: &Resolver<'_, H>, path: PathBuf, metadata: &Json, question: &QueryContext) -> Result<Self, ()> {
        if !matches!(metadata, Json::Object(_)) { return Err(()) }
        let directory = path.parent().ok_or(())?;
        let mut parents = Vec::new();
        if let Some(extends) = metadata.get(&resolver.settings.config.extends) {
            let inherited: Vec<&Json> = match extends {
                Json::Array(values) => values.iter().collect(),
                Json::Text(_) => vec![extends],
                _ => return Err(()),
            };
            for value in inherited {
                spend(question)?;
                let name = value.text().ok_or(())?;
                // A relative or rooted name is a path from this file; any
                // other names a package's file, looked up in `node_modules`
                // like an import.
                let relative = Path::new(name).is_absolute() || [".", ".."].iter()
                    .any(|dots| name == *dots || name.strip_prefix(dots).is_some_and(|rest| rest.starts_with(['/', '\\'])));
                let candidates: Vec<PathBuf> = if relative {
                    vec![clean(&directory.join(name))]
                } else {
                    directory.ancestors().map(|ancestor| clean(&ancestor.join("node_modules").join(name))).collect()
                };
                let mut found = None;
                'candidates: for candidate in candidates {
                    let mut choices = vec![candidate.clone()];
                    for config in &resolver.settings.config.files {
                        if candidate.extension().is_none() {
                            if let Some(extension) = Path::new(&**config).extension() {
                                choices.push(candidate.with_extension(extension));
                            }
                        }
                        choices.push(candidate.join(&**config));
                    }
                    for choice in choices {
                        spend(question)?;
                        if resolver.host.is_file(&choice, question).known().ok_or(())? {
                            found = Some(choice);
                            break 'candidates;
                        }
                    }
                }
                parents.push(found.ok_or(())?);
            }
        }
        parents.reverse();
        let own = match metadata.get(&resolver.settings.config.options) {
            Some(Json::Object(entries)) => entries.iter().map(|(key, value)| (key.clone(), OptionValue { value: value.clone(), directory: directory.to_path_buf() })).collect(),
            None => BTreeMap::new(),
            _ => return Err(()),
        };
        let config = &resolver.settings.config;
        let fields = [&config.source_files, &config.include, &config.exclude].into_iter().filter(|key| !key.is_empty()).filter_map(|key| {
            metadata.get(key).map(|value| (key.to_string(), OptionValue { value: value.clone(), directory: directory.to_path_buf() }))
        }).collect();
        let references = if resolver.settings.globals.references {
            metadata.project(&resolver.settings.config.references, question)?.into_iter().map(|value| {
                value.text().map(|text| clean(&directory.join(text))).ok_or(())
            }).collect::<Result<_, _>>()?
        } else { Vec::new() };
        Ok(Self { path, parents, own, fields, references, merged: Configuration::default() })
    }
}
