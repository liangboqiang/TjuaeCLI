use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use futures::future::join_all;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tjuae_mcp::manager::McpManager;
use tjuae_types::runtime_asset::{RuntimeAssetRef, RuntimeSkillRef};

use crate::frontmatter::{FrontmatterError, parse_frontmatter_strict, parse_skill_fields};
use crate::mcp::load_mcp_skills;
use crate::paths::{additional_skills_dirs, project_skills_dirs, user_skills_dir};
use crate::types::{LoadedFrom, SkillMetadata, SkillSource};

const RUNTIME_SKILL_KIND: &str = "skill";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A loaded skill paired with its canonical filesystem path for deduplication.
pub struct LoadedSkill {
    pub metadata: SkillMetadata,
    /// Canonicalized path used for dedup (symlinks resolved, `.`/`..` removed).
    pub resolved_path: PathBuf,
}

/// Skills and runtime receipts produced by a single deterministic load.
pub struct SkillLoadResult {
    pub skills: Vec<SkillMetadata>,
    pub runtime_assets: Vec<RuntimeAssetRef>,
}

/// Unsafe or unreadable filesystem material in a skill definition.
#[derive(Debug, Error)]
pub enum SkillTreeError {
    #[error("the skill root is unavailable")]
    RootUnavailable,
    #[error("the skill tree escapes its declared root")]
    OutsideRoot,
    #[error("the skill tree contains a filesystem alias or cycle")]
    AliasOrCycle,
    #[error("the skill tree contains an unsupported filesystem entry")]
    UnsupportedEntry,
    #[error("the skill tree contains a path that is not valid UTF-8")]
    InvalidPathEncoding,
    #[error("the skill tree could not be read")]
    ReadFailed,
}

/// Managed skill input rejected before engine startup.
#[derive(Debug, Error)]
pub enum ManagedSkillError {
    #[error("managed asset id must not be empty")]
    EmptyAssetId,
    #[error("managed asset {local_asset_id} has unsupported kind {kind}")]
    UnsupportedKind { local_asset_id: String, kind: String },
    #[error("managed asset id {local_asset_id} is duplicated")]
    DuplicateAssetId { local_asset_id: String },
    #[error("managed skill root for {local_asset_id} is duplicated")]
    DuplicateRoot { local_asset_id: String },
    #[error("managed skill name {name} is duplicated")]
    DuplicateName { name: String },
    #[error("managed skill {local_asset_id} does not contain an exact SKILL.md file")]
    MissingDefinition { local_asset_id: String },
    #[error("managed skill {local_asset_id} has invalid frontmatter: {source}")]
    InvalidFrontmatter {
        local_asset_id: String,
        #[source]
        source: FrontmatterError,
    },
    #[error("managed skill {local_asset_id} has an unsafe definition tree: {source}")]
    UnsafeTree {
        local_asset_id: String,
        #[source]
        source: SkillTreeError,
    },
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load all skills from the filesystem and optionally from MCP servers.
///
/// Priority order (highest first): managed → MCP → user → project → additional.
/// Deduplicates first by canonical path (symlinks resolved), then by name (first wins).
///
/// If `bare` is true, only `add_dirs` are consulted (used for isolated
/// environments where the user/project directories should be ignored).
/// Process-local managed skills are still included in bare mode.
///
/// Pass `mcp_manager: Some(&manager)` to include MCP-discovered skills.
pub async fn load_all_skills(
    cwd: &Path,
    add_dirs: &[PathBuf],
    bare: bool,
    mcp_manager: Option<&McpManager>,
) -> Vec<SkillMetadata> {
    match load_all_skills_with_managed(cwd, add_dirs, bare, mcp_manager, &[]).await {
        Ok(result) => result.skills,
        Err(error) => {
            tracing::error!(
                target: "tjuae_skills",
                error = %error,
                "空 managed 技能集合出现不变量错误"
            );
            Vec::new()
        }
    }
}

/// Load skills with an exact, process-local set of Core-managed definitions.
///
/// Managed definitions are loaded individually rather than discovered by
/// scanning their parent directory. Their priority is higher than every other
/// source, including MCP and filesystem skills.
pub async fn load_all_skills_with_managed(
    cwd: &Path,
    add_dirs: &[PathBuf],
    bare: bool,
    mcp_manager: Option<&McpManager>,
    managed_skills: &[RuntimeSkillRef],
) -> Result<SkillLoadResult, ManagedSkillError> {
    let (mut all, runtime_assets) = load_managed_skills(managed_skills).await?;

    if bare {
        // Bare mode: only load from explicit add_dirs
        let dirs = additional_skills_dirs(add_dirs);
        let futures: Vec<_> = dirs
            .iter()
            .map(|d| load_skills_from_dir(d, SkillSource::Project, LoadedFrom::Skills))
            .collect();
        for batch in join_all(futures).await {
            all.extend(batch);
        }
        return Ok(SkillLoadResult {
            skills: deduplicate_by_name(deduplicate(all)),
            runtime_assets,
        });
    }

    // MCP skills are below managed, but above filesystem skills.
    if let Some(manager) = mcp_manager {
        all.extend(load_mcp_skills(manager).await);
    }

    // User-level skills.
    if let Some(dir) = user_skills_dir()
        && dir.is_dir()
    {
        all.extend(load_skills_from_dir(&dir, SkillSource::User, LoadedFrom::Skills).await);
    }

    // Project-level skills (parallel across all dirs).
    let project_dirs = project_skills_dirs(cwd);
    let futures: Vec<_> = project_dirs
        .iter()
        .map(|d| load_skills_from_dir(d, SkillSource::Project, LoadedFrom::Skills))
        .collect();
    for batch in join_all(futures).await {
        all.extend(batch);
    }

    // Additional dirs from --add-dir.
    let add_skill_dirs = additional_skills_dirs(add_dirs);
    let futures: Vec<_> = add_skill_dirs
        .iter()
        .map(|d| load_skills_from_dir(d, SkillSource::Project, LoadedFrom::Skills))
        .collect();
    for batch in join_all(futures).await {
        all.extend(batch);
    }

    // Path-based dedup first (handles symlinked duplicates), then name-based
    // dedup to enforce managed vs. every other source.
    Ok(SkillLoadResult {
        skills: deduplicate_by_name(deduplicate(all)),
        runtime_assets,
    })
}

async fn load_managed_skills(
    managed_skills: &[RuntimeSkillRef],
) -> Result<(Vec<LoadedSkill>, Vec<RuntimeAssetRef>), ManagedSkillError> {
    let mut loaded = Vec::with_capacity(managed_skills.len());
    let mut receipts = Vec::with_capacity(managed_skills.len());
    let mut asset_ids = HashSet::new();
    let mut roots = HashSet::new();
    let mut names = HashSet::new();

    for managed in managed_skills {
        let local_asset_id = managed.asset.local_asset_id.clone();
        if local_asset_id.trim().is_empty() {
            return Err(ManagedSkillError::EmptyAssetId);
        }
        if !managed.asset.kind.eq_ignore_ascii_case(RUNTIME_SKILL_KIND) {
            return Err(ManagedSkillError::UnsupportedKind {
                local_asset_id,
                kind: managed.asset.kind.clone(),
            });
        }
        if !asset_ids.insert(local_asset_id.clone()) {
            return Err(ManagedSkillError::DuplicateAssetId { local_asset_id });
        }

        let canonical_root =
            tokio::fs::canonicalize(&managed.root)
                .await
                .map_err(|_| ManagedSkillError::UnsafeTree {
                    local_asset_id: local_asset_id.clone(),
                    source: SkillTreeError::RootUnavailable,
                })?;
        let root_metadata = tokio::fs::metadata(&canonical_root)
            .await
            .map_err(|_| ManagedSkillError::UnsafeTree {
                local_asset_id: local_asset_id.clone(),
                source: SkillTreeError::RootUnavailable,
            })?;
        if !root_metadata.is_dir() {
            return Err(ManagedSkillError::UnsafeTree {
                local_asset_id,
                source: SkillTreeError::RootUnavailable,
            });
        }
        if !roots.insert(canonical_root.clone()) {
            return Err(ManagedSkillError::DuplicateRoot { local_asset_id });
        }

        let skill_file =
            find_exact_file(&canonical_root, "SKILL.md")
                .await
                .ok_or_else(|| ManagedSkillError::MissingDefinition {
                    local_asset_id: local_asset_id.clone(),
                })?;
        let runtime_content_digest = compute_skill_definition_digest(&canonical_root)
            .await
            .map_err(|source| ManagedSkillError::UnsafeTree {
                local_asset_id: local_asset_id.clone(),
                source,
            })?;
        let content = tokio::fs::read_to_string(&skill_file)
            .await
            .map_err(|_| ManagedSkillError::UnsafeTree {
                local_asset_id: local_asset_id.clone(),
                source: SkillTreeError::ReadFailed,
            })?;
        let parsed = parse_frontmatter_strict(&content).map_err(|source| ManagedSkillError::InvalidFrontmatter {
            local_asset_id: local_asset_id.clone(),
            source,
        })?;
        let resolved_name = canonical_root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ManagedSkillError::MissingDefinition {
                local_asset_id: local_asset_id.clone(),
            })?
            .to_owned();
        if !names.insert(resolved_name.clone()) {
            return Err(ManagedSkillError::DuplicateName { name: resolved_name });
        }

        let skill_root = canonical_root.to_string_lossy().into_owned();
        let metadata = parse_skill_fields(
            &parsed.frontmatter,
            &parsed.content,
            &resolved_name,
            SkillSource::Managed,
            LoadedFrom::Managed,
            Some(&skill_root),
        );
        let resolved_path = tokio::fs::canonicalize(&skill_file)
            .await
            .map_err(|_| ManagedSkillError::UnsafeTree {
                local_asset_id: local_asset_id.clone(),
                source: SkillTreeError::ReadFailed,
            })?;
        loaded.push(LoadedSkill {
            metadata,
            resolved_path,
        });

        let mut receipt = managed.asset.clone();
        receipt.kind = RUNTIME_SKILL_KIND.to_string();
        receipt.runtime_content_digest = runtime_content_digest;
        receipts.push(receipt);
    }

    Ok((loaded, receipts))
}

// ---------------------------------------------------------------------------
// Internal: load from skills/ directory (directory-only format)
// ---------------------------------------------------------------------------

/// Load skills from a `skills/` directory.
///
/// Only the directory format is supported: each direct or nested subdirectory
/// that contains a `SKILL.md` file (case-sensitive) is loaded.
/// The skill name is derived from the relative path using colon separators.
pub(crate) async fn load_skills_from_dir(
    base_dir: &Path,
    source: SkillSource,
    loaded_from: LoadedFrom,
) -> Vec<LoadedSkill> {
    let canonical_base = match tokio::fs::canonicalize(base_dir).await {
        Ok(path) => path,
        Err(_) => return Vec::new(),
    };
    let mut results = Vec::new();
    let mut visited = HashSet::from([canonical_base.clone()]);
    collect_skill_md(
        &canonical_base,
        &canonical_base,
        source,
        loaded_from,
        &mut visited,
        &mut results,
    )
    .await;
    results
}

/// Recursively scan `dir` for `SKILL.md` files.
// This is a recursive async function — we use a Box::pin to satisfy the compiler.
fn collect_skill_md<'a>(
    base_dir: &'a Path,
    dir: &'a Path,
    source: SkillSource,
    loaded_from: LoadedFrom,
    visited: &'a mut HashSet<PathBuf>,
    results: &'a mut Vec<LoadedSkill>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let mut read_dir = match tokio::fs::read_dir(dir).await {
            Ok(rd) => rd,
            Err(_) => return,
        };

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            let canonical_path = match tokio::fs::canonicalize(&path).await {
                Ok(path) => path,
                Err(_) => continue,
            };
            if !path_is_within(base_dir, &canonical_path) {
                tracing::warn!(
                    target: "tjuae_skills",
                    "技能目录包含越界文件系统链接，已跳过"
                );
                continue;
            }
            let metadata = match tokio::fs::metadata(&canonical_path).await {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };

            if !metadata.is_dir() {
                continue;
            }
            if !visited.insert(canonical_path.clone()) {
                tracing::warn!(
                    target: "tjuae_skills",
                    "技能目录包含文件系统循环或别名，已跳过"
                );
                continue;
            }

            // Check for SKILL.md directly inside this subdirectory using an
            // exact case-sensitive name comparison (important on
            // case-insensitive filesystems like macOS APFS).
            if let Some(skill_file) = find_exact_file(&canonical_path, "SKILL.md").await {
                if let Some(skill) = load_skill_file(&skill_file, base_dir, &canonical_path, source, loaded_from).await
                {
                    results.push(skill);
                }
            } else {
                // Recurse into subdirectory (namespace nesting).
                collect_skill_md(base_dir, &canonical_path, source, loaded_from, visited, results).await;
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Internal: load a single skill file
// ---------------------------------------------------------------------------

/// Read, parse, and return a `LoadedSkill` for a single Markdown file.
/// Returns `None` if the file cannot be read.
async fn load_skill_file(
    file_path: &Path,
    base_dir: &Path,
    skill_dir: &Path,
    source: SkillSource,
    loaded_from: LoadedFrom,
) -> Option<LoadedSkill> {
    if let Err(error) = compute_skill_definition_digest(skill_dir).await {
        tracing::warn!(
            target: "tjuae_skills",
            error = %error,
            "技能定义树未通过安全校验，已跳过"
        );
        return None;
    }
    let content = tokio::fs::read_to_string(file_path).await.ok()?;
    let parsed = match parse_frontmatter_strict(&content) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(
                target: "tjuae_skills",
                error = %error,
                "技能 frontmatter 无效，已跳过"
            );
            return None;
        }
    };

    let resolved_name = build_namespace(base_dir, skill_dir);
    // skill_root is the directory containing SKILL.md (i.e., skill_dir itself),
    // used for ${TJUAE_SKILL_DIR} variable substitution in skill content.
    let skill_root = Some(skill_dir.to_string_lossy().into_owned());

    let metadata = parse_skill_fields(
        &parsed.frontmatter,
        &parsed.content,
        &resolved_name,
        source,
        loaded_from,
        skill_root.as_deref(),
    );

    let resolved_path = try_canonicalize(file_path).unwrap_or_else(|| file_path.to_owned());

    Some(LoadedSkill {
        metadata,
        resolved_path,
    })
}

// ---------------------------------------------------------------------------
// Internal: namespace building
// ---------------------------------------------------------------------------

/// Build a colon-separated namespace from a directory hierarchy.
///
/// Examples:
/// - base=`<config_dir>/tjuae/skills`, target=`<config_dir>/tjuae/skills/db/migrate` → `"db:migrate"`
/// - base=`<config_dir>/tjuae/skills`, target=`<config_dir>/tjuae/skills/my-skill` → `"my-skill"`
pub(crate) fn build_namespace(base_dir: &Path, target_dir: &Path) -> String {
    match target_dir.strip_prefix(base_dir) {
        Ok(relative) => relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(":"),
        Err(_) => target_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Internal: deduplication
// ---------------------------------------------------------------------------

/// Deduplicate loaded skills by canonical path. First occurrence wins.
fn deduplicate(skills: Vec<LoadedSkill>) -> Vec<SkillMetadata> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut result = Vec::new();

    for skill in skills {
        if seen.insert(skill.resolved_path) {
            result.push(skill.metadata);
        }
    }

    result
}

/// Deduplicate by skill name (case-sensitive). First occurrence wins.
///
/// Called after path-based dedup to enforce priority between managed, MCP, and
/// filesystem skills that share the same name but have different paths.
fn deduplicate_by_name(skills: Vec<SkillMetadata>) -> Vec<SkillMetadata> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut result = Vec::new();

    for skill in skills {
        if seen.insert(skill.name.clone(), ()).is_none() {
            result.push(skill);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Internal: safe tree inspection and canonicalization
// ---------------------------------------------------------------------------

/// Compute the deterministic digest of a complete skill definition tree.
///
/// Every directory and file is canonicalized and must remain contained by the
/// declared root. Repeated canonical targets are rejected so symlink or
/// junction aliases cannot introduce cycles or ambiguous definitions.
pub(crate) async fn compute_skill_definition_digest(root: &Path) -> Result<String, SkillTreeError> {
    let canonical_root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| SkillTreeError::RootUnavailable)?;
    let metadata = tokio::fs::metadata(&canonical_root)
        .await
        .map_err(|_| SkillTreeError::RootUnavailable)?;
    if !metadata.is_dir() {
        return Err(SkillTreeError::RootUnavailable);
    }

    let mut visited_dirs = HashSet::from([canonical_root.clone()]);
    let mut visited_files = HashSet::new();
    let mut files = Vec::new();
    collect_definition_files(
        &canonical_root,
        &canonical_root,
        &mut visited_dirs,
        &mut visited_files,
        &mut files,
    )
    .await?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    hasher.update(b"tjuae-runtime-skill-v1\0");
    for (relative_path, canonical_path) in files {
        let content = tokio::fs::read(canonical_path)
            .await
            .map_err(|_| SkillTreeError::ReadFailed)?;
        hash_length_prefixed(&mut hasher, relative_path.as_bytes());
        hash_length_prefixed(&mut hasher, &content);
    }

    let digest = hasher.finalize();
    Ok(format!("sha256-{digest:x}"))
}

type DefinitionFile = (String, PathBuf);

fn collect_definition_files<'a>(
    root: &'a Path,
    directory: &'a Path,
    visited_dirs: &'a mut HashSet<PathBuf>,
    visited_files: &'a mut HashSet<PathBuf>,
    files: &'a mut Vec<DefinitionFile>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), SkillTreeError>> + Send + 'a>> {
    Box::pin(async move {
        let mut entries = tokio::fs::read_dir(directory)
            .await
            .map_err(|_| SkillTreeError::ReadFailed)?;
        while let Some(entry) = entries.next_entry().await.map_err(|_| SkillTreeError::ReadFailed)? {
            let canonical_path = tokio::fs::canonicalize(entry.path())
                .await
                .map_err(|_| SkillTreeError::ReadFailed)?;
            if !path_is_within(root, &canonical_path) {
                return Err(SkillTreeError::OutsideRoot);
            }

            let metadata = tokio::fs::metadata(&canonical_path)
                .await
                .map_err(|_| SkillTreeError::ReadFailed)?;
            if metadata.is_dir() {
                if !visited_dirs.insert(canonical_path.clone()) {
                    return Err(SkillTreeError::AliasOrCycle);
                }
                collect_definition_files(root, &canonical_path, visited_dirs, visited_files, files).await?;
            } else if metadata.is_file() {
                if !visited_files.insert(canonical_path.clone()) {
                    return Err(SkillTreeError::AliasOrCycle);
                }
                files.push((relative_utf8_path(root, &canonical_path)?, canonical_path));
            } else {
                return Err(SkillTreeError::UnsupportedEntry);
            }
        }
        Ok(())
    })
}

fn relative_utf8_path(root: &Path, path: &Path) -> Result<String, SkillTreeError> {
    let relative = path.strip_prefix(root).map_err(|_| SkillTreeError::OutsideRoot)?;
    let components = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_owned)
                .ok_or(SkillTreeError::InvalidPathEncoding)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(components.join("/"))
}

fn hash_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(not(windows))]
fn path_is_within(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(windows)]
fn path_is_within(root: &Path, path: &Path) -> bool {
    let normalized_root = root.to_string_lossy().to_lowercase();
    let normalized_path = path.to_string_lossy().to_lowercase();
    normalized_path == normalized_root
        || normalized_path
            .strip_prefix(&normalized_root)
            .is_some_and(|suffix| suffix.starts_with(['\\', '/']))
}

/// Canonicalize a path, returning `None` if the path does not exist.
/// Never panics.
pub(crate) fn try_canonicalize(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Find a file with an exact case-sensitive name inside `dir`.
///
/// On case-insensitive filesystems (e.g., macOS APFS), `Path::is_file()` may
/// return `true` for `SKILL.md` even when only `skill.md` exists.  This
/// function reads the directory entries and performs a byte-for-byte name
/// comparison to avoid false positives.
///
/// Returns `None` if no entry with that exact name exists or if the directory
/// cannot be read.
async fn find_exact_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut rd = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = rd.next_entry().await {
        if entry.file_name().to_string_lossy() == name {
            let path = entry.path();
            let ft = entry.file_type().await.ok()?;
            if ft.is_file() {
                return Some(path);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "loader_test.rs"]
mod loader_test;

#[cfg(test)]
#[path = "loader_supplemental_test.rs"]
mod loader_supplemental_test;
