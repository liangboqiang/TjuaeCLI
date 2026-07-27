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
        if p.is_dir() { "存在" } else { "未找到" }
    }

    match user_skills_dir() {
        Some(dir) => println!("用户：  {}  ({})", dir.display(), status(&dir)),
        None => println!("用户：  <无法确定配置目录>"),
    }

    let cwd = env::current_dir().unwrap_or_default();
    let project_dirs = project_skills_dirs(&cwd);
    if project_dirs.is_empty() {
        println!("项目：  <未找到>");
    } else {
        for dir in &project_dirs {
            println!("项目：  {}  ({})", dir.display(), status(dir));
        }
    }
}
