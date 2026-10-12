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
    pub options: BTreeMap<String, OptionValue>,
}

impl Configuration {
    pub fn load<H: Host + ?Sized>(resolver: &Resolver<'_, H>, directory: &Path, question: &QueryContext) -> Result<Self, ()> {
        for ancestor in directory.ancestors() {
            spend(question)?;
            for name in &resolver.settings.config.files {
                let path = clean(&ancestor.join(&**name));
                if let Some(metadata) = resolver.metadata(&path, question)? {
                    return Self::inherit(resolver, path, metadata, question);
                }
            }
        }
        Ok(Self::default())
    }

    fn inherit<H: Host + ?Sized>(resolver: &Resolver<'_, H>, path: PathBuf, metadata: Json, question: &QueryContext) -> Result<Self, ()> {
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
                        frame.parents.pop();
                    }
                    Some(None) => return Err(()),
                    None => {
                        let path = path.clone();
                        let metadata = resolver.metadata(&path, question)?.ok_or(())?;
                        memo.insert(path.clone(), None);
                        stack.push(Frame::new(resolver, path, metadata, question)?);
                    }
                }
                continue;
            }
            let mut frame = stack.pop().ok_or(())?;
            frame.merged.options.extend(frame.own);
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
    merged: Configuration,
}

impl Frame {
    fn new<H: Host + ?Sized>(resolver: &Resolver<'_, H>, path: PathBuf, metadata: Json, question: &QueryContext) -> Result<Self, ()> {
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
                let candidate = clean(&directory.join(name));
                let mut choices = vec![candidate.clone()];
                for config in &resolver.settings.config.files {
                    if candidate.extension().is_none() {
                        if let Some(extension) = Path::new(&**config).extension() {
                            choices.push(candidate.with_extension(extension));
                        }
                    }
                    choices.push(candidate.join(&**config));
                }
                let mut found = None;
                for choice in choices {
                    spend(question)?;
                    if resolver.host.is_file(&choice, question).known().ok_or(())? {
                        found = Some(choice);
                        break;
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
        Ok(Self { path, parents, own, merged: Configuration::default() })
    }
}
