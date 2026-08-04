use super::*;
use std::fs;
use tempfile::TempDir;
use tjuae_types::runtime_asset::{RuntimeAssetRef, RuntimeSkillRef};

fn write_skill(dir: &Path, rel_path: &str, content: &str) {
    let full = dir.join(rel_path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, content).unwrap();
}

fn managed_skill(root: &Path, local_asset_id: &str) -> RuntimeSkillRef {
    RuntimeSkillRef {
        asset: RuntimeAssetRef {
            local_asset_id: local_asset_id.to_string(),
            kind: "skill".to_string(),
            local_definition_digest: format!("sha256-{}", "a".repeat(64)),
            runtime_content_digest: "caller-supplied-digest".to_string(),
            upstream_package: None,
            upstream_asset_id: None,
            upstream_version: None,
            upstream_revision: None,
        },
        root: root.to_path_buf(),
    }
}

// --- build_namespace ---

#[test]
fn test_build_namespace_simple() {
    let base = Path::new("/skills");
    let target = Path::new("/skills/my-skill");
    assert_eq!(build_namespace(base, target), "my-skill");
}

#[test]
fn test_build_namespace_nested() {
    let base = Path::new("/skills");
    let target = Path::new("/skills/db/migrate");
    assert_eq!(build_namespace(base, target), "db:migrate");
}

#[test]
fn test_build_namespace_three_levels() {
    let base = Path::new("/skills");
    let target = Path::new("/skills/a/b/c");
    assert_eq!(build_namespace(base, target), "a:b:c");
}

#[test]
fn test_build_namespace_same_dir() {
    let base = Path::new("/skills");
    // target == base → empty string
    let result = build_namespace(base, base);
    assert_eq!(result, "");
}

// --- try_canonicalize ---

#[test]
fn test_try_canonicalize_existing_path() {
    let tmp = TempDir::new().unwrap();
    let result = try_canonicalize(tmp.path());
    assert!(result.is_some());
}

#[test]
fn test_try_canonicalize_nonexistent_returns_none() {
    let result = try_canonicalize(Path::new("/nonexistent/path/xyz"));
    assert!(result.is_none());
}

// --- deduplicate ---

#[test]
fn test_deduplicate_removes_duplicates() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("skill.md");
    fs::write(&file, "").unwrap();
    let canonical = std::fs::canonicalize(&file).unwrap();

    let fm = crate::types::FrontmatterData::default();
    let make_meta =
        || crate::frontmatter::parse_skill_fields(&fm, "", "test", SkillSource::User, LoadedFrom::Skills, None);

    let skills = vec![
        LoadedSkill {
            metadata: make_meta(),
            resolved_path: canonical.clone(),
        },
        LoadedSkill {
            metadata: make_meta(),
            resolved_path: canonical.clone(),
        },
    ];

    let result = deduplicate(skills);
    assert_eq!(result.len(), 1);
}

#[test]
fn test_deduplicate_different_paths_preserved() {
    let tmp = TempDir::new().unwrap();
    let file1 = tmp.path().join("skill1.md");
    let file2 = tmp.path().join("skill2.md");
    fs::write(&file1, "").unwrap();
    fs::write(&file2, "").unwrap();

    let fm = crate::types::FrontmatterData::default();
    let make_meta =
        || crate::frontmatter::parse_skill_fields(&fm, "", "test", SkillSource::User, LoadedFrom::Skills, None);

    let skills = vec![
        LoadedSkill {
            metadata: make_meta(),
            resolved_path: std::fs::canonicalize(&file1).unwrap(),
        },
        LoadedSkill {
            metadata: make_meta(),
            resolved_path: std::fs::canonicalize(&file2).unwrap(),
        },
    ];

    let result = deduplicate(skills);
    assert_eq!(result.len(), 2);
}

// --- load_skills_from_dir ---

#[tokio::test]
async fn test_load_skills_from_dir_basic() {
    let tmp = TempDir::new().unwrap();
    write_skill(
        tmp.path(),
        "my-skill/SKILL.md",
        "---\nname: my-skill\ndescription: A test skill\n---\n# Body\n",
    );

    let skills = load_skills_from_dir(tmp.path(), SkillSource::User, LoadedFrom::Skills).await;
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].metadata.name, "my-skill");
}

#[tokio::test]
async fn test_load_skills_from_dir_nested_namespace() {
    let tmp = TempDir::new().unwrap();
    write_skill(tmp.path(), "db/migrate/SKILL.md", "---\ndescription: Migrate DB\n---\n");

    let skills = load_skills_from_dir(tmp.path(), SkillSource::User, LoadedFrom::Skills).await;
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].metadata.name, "db:migrate");
}

#[tokio::test]
async fn test_load_skills_from_dir_case_sensitive_skill_md() {
    let tmp = TempDir::new().unwrap();
    // Only lowercase "skill.md" — should NOT be loaded
    write_skill(tmp.path(), "my-skill/skill.md", "---\n---\n# Body\n");

    let skills = load_skills_from_dir(tmp.path(), SkillSource::User, LoadedFrom::Skills).await;
    assert!(skills.is_empty(), "skill.md (lowercase) should not be loaded");
}

#[tokio::test]
async fn test_load_skills_from_dir_empty_dir() {
    let tmp = TempDir::new().unwrap();
    let skills = load_skills_from_dir(tmp.path(), SkillSource::User, LoadedFrom::Skills).await;
    assert!(skills.is_empty());
}

#[tokio::test]
async fn test_load_skills_from_dir_nonexistent_silently_skipped() {
    let skills = load_skills_from_dir(Path::new("/nonexistent/path"), SkillSource::User, LoadedFrom::Skills).await;
    assert!(skills.is_empty());
}

// --- load_all_skills ---

#[tokio::test]
async fn test_load_all_skills_bare_mode() {
    let tmp = TempDir::new().unwrap();
    // Create .tjuae/skills/ under the add_dir
    let skills_dir = tmp.path().join(".tjuae").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    write_skill(&skills_dir, "my-skill/SKILL.md", "---\n---\n");

    let result = load_all_skills(Path::new("/nonexistent"), &[tmp.path().to_owned()], true, None).await;
    assert!(result.iter().any(|skill| skill.name == "my-skill"));
}

#[tokio::test]
async fn test_load_all_skills_deduplicates() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    // Create git root
    fs::create_dir(root.join(".git")).unwrap();

    // Create same skill in project dir (will appear twice due to walk)
    let skills_dir = root.join(".tjuae").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    write_skill(&skills_dir, "my-skill/SKILL.md", "---\n---\n");

    let result = load_all_skills(root, &[], false, None).await;
    let names: Vec<_> = result.iter().map(|s| s.name.as_str()).collect();
    let count = names.iter().filter(|&&n| n == "my-skill").count();
    assert_eq!(count, 1, "skill should appear exactly once after dedup");
}

#[tokio::test]
async fn managed_skill_has_highest_priority_and_returns_actual_receipt() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(project.join(".git")).unwrap();
    let project_skills = project.join(".tjuae").join("skills");
    write_skill(
        &project_skills,
        "shared/SKILL.md",
        "---\ndescription: project copy\n---\nproject",
    );

    let managed_root = tmp.path().join("managed").join("shared");
    write_skill(
        managed_root.parent().unwrap(),
        "shared/SKILL.md",
        "---\ndescription: managed copy\n---\nmanaged",
    );
    write_skill(
        managed_root.parent().unwrap(),
        "not-selected/SKILL.md",
        "---\ndescription: must stay isolated\n---\n",
    );

    let loaded = load_all_skills_with_managed(&project, &[], false, None, &[managed_skill(&managed_root, "local-1")])
        .await
        .expect("managed skill should load");

    let shared = loaded
        .skills
        .iter()
        .find(|skill| skill.name == "shared")
        .expect("shared skill should be present");
    assert_eq!(shared.source, SkillSource::Managed);
    assert!(shared.content.contains("managed"));
    assert!(!loaded.skills.iter().any(|skill| skill.name == "not-selected"));
    assert_eq!(loaded.runtime_assets.len(), 1);
    assert_eq!(loaded.runtime_assets[0].local_asset_id, "local-1");
    assert!(loaded.runtime_assets[0].runtime_content_digest.starts_with("sha256-"));
    assert_ne!(
        loaded.runtime_assets[0].runtime_content_digest,
        "caller-supplied-digest"
    );
    assert_eq!(
        loaded.runtime_assets[0].local_definition_digest,
        format!("sha256-{}", "a".repeat(64))
    );
}

#[tokio::test]
async fn strict_loader_rejects_silently_ignored_permission_fields() {
    let tmp = TempDir::new().unwrap();
    write_skill(
        tmp.path(),
        "unsafe/SKILL.md",
        "---\nallowedTools: [Read]\npermissions: {}\n---\nbody",
    );

    let skills = load_skills_from_dir(tmp.path(), SkillSource::User, LoadedFrom::Skills).await;

    assert!(skills.is_empty());
}

#[tokio::test]
async fn definition_digest_changes_when_resource_changes() {
    let tmp = TempDir::new().unwrap();
    write_skill(tmp.path(), "skill/SKILL.md", "---\n---\nbody");
    write_skill(tmp.path(), "skill/reference.txt", "first");
    let skill_root = tmp.path().join("skill");

    let before = compute_skill_definition_digest(&skill_root).await.unwrap();
    fs::write(skill_root.join("reference.txt"), "second").unwrap();
    let after = compute_skill_definition_digest(&skill_root).await.unwrap();

    assert_ne!(before, after);
}

#[test]
fn containment_does_not_accept_a_sibling_with_the_same_prefix() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("skill");
    let sibling = tmp.path().join("skill-escape");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&sibling).unwrap();
    let canonical_root = fs::canonicalize(root).unwrap();
    let canonical_sibling = fs::canonicalize(sibling).unwrap();

    assert!(!path_is_within(&canonical_root, &canonical_sibling));
}

#[cfg(unix)]
#[tokio::test]
async fn definition_tree_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    write_skill(tmp.path(), "skill/SKILL.md", "---\n---\nbody");
    write_skill(tmp.path(), "outside/private.txt", "private");
    let skill_root = tmp.path().join("skill");
    symlink(tmp.path().join("outside"), skill_root.join("escape")).unwrap();

    let error = compute_skill_definition_digest(&skill_root)
        .await
        .expect_err("out-of-root symlink must be rejected");

    assert!(matches!(error, SkillTreeError::OutsideRoot));
}

#[cfg(unix)]
#[tokio::test]
async fn definition_tree_rejects_symlink_cycle() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    write_skill(tmp.path(), "skill/SKILL.md", "---\n---\nbody");
    let skill_root = tmp.path().join("skill");
    symlink(&skill_root, skill_root.join("loop")).unwrap();

    let error = compute_skill_definition_digest(&skill_root)
        .await
        .expect_err("filesystem cycle must be rejected");

    assert!(matches!(error, SkillTreeError::AliasOrCycle));
}
