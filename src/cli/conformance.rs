use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::{ConfigArg, Format, LintArgs, Output, Project, SourceRecord, Sources};
use crate::config::source::{normalize_path, Disk, SourceProvider};

#[derive(Deserialize)]
struct Request {
    id: String,
    config: String,
    sources: Vec<SourceRecord>,
}

#[derive(Serialize)]
struct Reply {
    id: Option<String>,
    code: u8,
    stdout: String,
    stderr: String,
}

fn project_matches(project: &Project) -> bool {
    project.config_ok && project.linter.sources.files().all(|(_, source)| {
        source.path.as_ref().is_none_or(|path| Disk.read(path).is_ok_and(|text| text.as_str() == &*source.text))
    })
}

fn request(output: &mut Output<'_>, config: &ConfigArg, path: &Path, cache: &mut Option<Project>, request: Request) -> u8 {
    let started = Instant::now();
    if request.sources.iter().any(|source| !source.path.is_absolute()) {
        super::render::error(output, "conformance source paths must be absolute");
        return 2;
    }
    let records = std::iter::once(SourceRecord { path: path.to_owned(), text: request.config }).chain(request.sources);
    let sources = match Sources::from_records(records, output.cwd) {
        Ok(sources) => sources,
        Err(error) => {
            super::render::error(output, &error);
            return 2;
        }
    };
    if !cache.as_ref().is_some_and(project_matches) {
        *cache = None;
        match super::load_for_lint(output, config) {
            Ok(project) => *cache = Some(project),
            Err(code) => return code,
        }
    }
    let project = cache.as_mut().expect("prepared project");
    project.linter.reset_runtime();
    let code = super::lint::prepared(output, LintArgs {
        paths: Vec::new(), sources: None, jobs: std::num::NonZeroUsize::new(1),
        fix: false, fix_preview: true, format: Format::Jsonl,
        stats: false, profile: false, no_cache: true,
    }, project, Some(&sources), started);
    if !project.config_ok { *cache = None; }
    code
}

pub(super) fn run(output: &mut Output<'_>, config: &ConfigArg, input: impl Read) -> u8 {
    let Some(path) = config.config.as_ref() else {
        super::render::error(output, "conformance requires --config");
        return 2;
    };
    let path = normalize_path(&output.cwd.join(path));
    let mut cache = None;
    for line in BufReader::new(input).lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                super::render::error(output, &error.to_string());
                return 2;
            }
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut plain_errors = anstream::StripStream::new(&mut stderr);
        let mut captured = Output { cwd: output.cwd, stdout: &mut stdout, stderr: &mut plain_errors };
        let parsed = serde_json::from_str::<serde_json::Value>(&line);
        let mut id = parsed.as_ref().ok().and_then(|value| value.get("id")).and_then(|id| id.as_str()).map(str::to_owned);
        let code = match parsed.and_then(serde_json::from_value::<Request>) {
            Ok(frame) => {
                id = Some(frame.id.clone());
                request(&mut captured, config, &path, &mut cache, frame)
            }
            Err(error) => {
                super::render::error(&mut captured, &error.to_string());
                2
            }
        };
        drop(plain_errors);
        let reply = Reply {
            id, code,
            stdout: String::from_utf8(stdout).expect("lint output is utf8"),
            stderr: String::from_utf8(stderr).expect("lint errors are utf8"),
        };
        output.out(format_args!("{}", serde_json::to_string(&reply).expect("serialise reply")));
        if let Err(error) = output.stdout.flush() {
            super::render::error(output, &error.to_string());
            return 2;
        }
    }
    0
}
