use std::env;
use std::path::Path;

use tjuae_skills::paths::{project_skills_dirs, user_skills_dir};

use crate::cli::SkillsAction;

pub(crate) fn run(action: SkillsAction) -> anyhow::Result<()> {
    match action {
        SkillsAction::Path => print_skills_paths(),
    }
    Ok(())
}

fn print_skills_paths() {
    fn status(p: &Path) -> &'static str {
        if p.is_dir() { "exists" } else { "not found" }
    }

    match user_skills_dir() {
        Some(dir) => println!("User:    {}  ({})", dir.display(), status(&dir)),
        None => println!("User:    <unable to determine config directory>"),
    }

    let cwd = env::current_dir().unwrap_or_default();
    let project_dirs = project_skills_dirs(&cwd);
    if project_dirs.is_empty() {
        println!("Project: <none found>");
    } else {
        for dir in &project_dirs {
            println!("Project: {}  ({})", dir.display(), status(dir));
        }
    }
}
