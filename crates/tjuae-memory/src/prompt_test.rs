use super::*;

// ===========================================================================
// Unit tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- constants integrity -------------------------------------------------

    #[test]
    fn types_section_contains_all_four_types() {
        for ty in ["user", "feedback", "project", "reference"] {
            assert!(
                TYPES_SECTION.contains(&format!("<name>{ty}</name>")),
                "TYPES_SECTION missing type: {ty}"
            );
        }
    }

    #[test]
    fn types_section_has_no_scope_tags() {
        assert!(
            !TYPES_SECTION.contains("<scope>"),
            "individual-mode TYPES_SECTION should not contain <scope> tags"
        );
    }

    #[test]
    fn what_not_to_save_mentions_agents_md() {
        assert!(
            WHAT_NOT_TO_SAVE.contains("AGENTS.md"),
            "should reference AGENTS.md, not CLAUDE.md"
        );
    }

    #[test]
    fn what_not_to_save_no_claude_brand() {
        assert!(
            !WHAT_NOT_TO_SAVE.contains("CLAUDE.md"),
            "should not contain bb brand reference CLAUDE.md"
        );
    }

    #[test]
    fn frontmatter_example_has_all_fields() {
        assert!(FRONTMATTER_EXAMPLE.contains("name:"));
        assert!(FRONTMATTER_EXAMPLE.contains("description:"));
        assert!(FRONTMATTER_EXAMPLE.contains("type:"));
    }

    #[test]
    fn frontmatter_example_lists_all_types() {
        assert!(FRONTMATTER_EXAMPLE.contains("user"));
        assert!(FRONTMATTER_EXAMPLE.contains("feedback"));
        assert!(FRONTMATTER_EXAMPLE.contains("project"));
        assert!(FRONTMATTER_EXAMPLE.contains("reference"));
    }

    // -- how_to_save_section -------------------------------------------------

    #[test]
    fn how_to_save_references_entrypoint() {
        let section = how_to_save_section();
        assert!(section.contains(ENTRYPOINT_NAME));
    }

    #[test]
    fn how_to_save_mentions_max_lines() {
        let section = how_to_save_section();
        assert!(section.contains(&MAX_INDEX_LINES.to_string()));
    }

    #[test]
    fn how_to_save_describes_two_steps() {
        let section = how_to_save_section();
        assert!(section.contains("步骤 1"));
        assert!(section.contains("步骤 2"));
    }

    // -- build_memory_instructions -------------------------------------------

    #[test]
    fn instructions_contain_display_name() {
        let lines = build_memory_instructions(Path::new("/test/memory"));
        let joined = lines.join("\n");
        assert!(joined.contains(DISPLAY_NAME));
    }

    #[test]
    fn instructions_contain_memory_dir_path() {
        let lines = build_memory_instructions(Path::new("/custom/path/memory"));
        let joined = lines.join("\n");
        assert!(joined.contains("/custom/path/memory"));
    }

    #[test]
    fn instructions_contain_dir_exists_guidance() {
        let lines = build_memory_instructions(Path::new("/test/memory"));
        let joined = lines.join("\n");
        assert!(joined.contains("已经存在"));
    }

    #[test]
    fn instructions_contain_all_sections() {
        let lines = build_memory_instructions(Path::new("/test/memory"));
        let joined = lines.join("\n");
        assert!(joined.contains("## 记忆类型"));
        assert!(joined.contains("## 不应保存到记忆中的内容"));
        assert!(joined.contains("## 如何保存记忆"));
        assert!(joined.contains("## 何时访问记忆"));
        assert!(joined.contains("## 根据记忆提出建议前"));
        assert!(joined.contains("## 记忆与其他持久化方式"));
    }

    #[test]
    fn instructions_no_bb_brand() {
        let lines = build_memory_instructions(Path::new("/test/memory"));
        let joined = lines.join("\n");
        assert!(!joined.contains("~/.claude"), "should not reference bb config path");
        assert!(!joined.contains("CLAUDE.md"), "should not reference bb config file");
    }

    // -- memory_type_descriptions --------------------------------------------

    #[test]
    fn type_descriptions_returns_types_section() {
        let desc = memory_type_descriptions();
        assert!(desc.contains("<types>"));
        assert!(desc.contains("</types>"));
    }

    // -- build_memory_prompt (filesystem-dependent, basic validation) ---------

    #[test]
    fn prompt_with_nonexistent_dir_shows_empty_state() {
        let result = build_memory_prompt(Path::new("/nonexistent/memory/dir"));
        assert!(result.contains(ENTRYPOINT_NAME));
        assert!(result.contains("当前为空"));
    }

    #[test]
    fn prompt_with_existing_index() {
        let tmp = tempfile::tempdir().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        let index_path = mem_dir.join(ENTRYPOINT_NAME);
        std::fs::write(&index_path, "- [Role](user_role.md) \u{2014} user role info\n").unwrap();

        let result = build_memory_prompt(&mem_dir);
        assert!(result.contains("user_role.md"));
        assert!(result.contains("user role info"));
        assert!(!result.contains("当前为空"));
    }

    #[test]
    fn prompt_with_empty_index_file_shows_empty_state() {
        let tmp = tempfile::tempdir().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        let index_path = mem_dir.join(ENTRYPOINT_NAME);
        std::fs::write(&index_path, "").unwrap();

        let result = build_memory_prompt(&mem_dir);
        assert!(result.contains("当前为空"));
    }

    #[test]
    fn prompt_includes_instructions_before_index() {
        let tmp = tempfile::tempdir().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        let index_path = mem_dir.join(ENTRYPOINT_NAME);
        std::fs::write(&index_path, "- [A](a.md) \u{2014} test\n").unwrap();

        let result = build_memory_prompt(&mem_dir);

        // Instructions (type descriptions) should appear before the index content
        let types_pos = result.find("## 记忆类型").unwrap();
        let index_pos = result.find(&format!("## {ENTRYPOINT_NAME}")).unwrap();
        assert!(
            types_pos < index_pos,
            "instructions should appear before MEMORY.md content"
        );
    }

    #[test]
    fn prompt_truncates_large_index() {
        let tmp = tempfile::tempdir().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        let index_path = mem_dir.join(ENTRYPOINT_NAME);

        // Create an index with 250 lines
        let content: String = (0..250)
            .map(|i| format!("- [Item {i}](item_{i}.md) \u{2014} summary {i}\n"))
            .collect();
        std::fs::write(&index_path, &content).unwrap();

        let result = build_memory_prompt(&mem_dir);
        assert!(result.contains("警告"));
    }

    // -- build_memory_prompt_minimal -------------------------------------------

    #[test]
    fn minimal_prompt_contains_display_name() {
        let result = build_memory_prompt_minimal(Path::new("/test/memory"));
        assert!(result.contains(DISPLAY_NAME));
    }

    #[test]
    fn minimal_prompt_contains_dir_path() {
        let result = build_memory_prompt_minimal(Path::new("/custom/path/memory"));
        assert!(result.contains("/custom/path/memory"));
    }

    #[test]
    fn minimal_prompt_contains_compact_rules() {
        let result = build_memory_prompt_minimal(Path::new("/test/memory"));
        assert!(result.contains("记忆类型包括"), "should list memory types compactly");
        assert!(result.contains("MEMORY.md 是索引"), "should mention MEMORY.md role");
    }

    #[test]
    fn minimal_prompt_omits_full_type_taxonomy() {
        let result = build_memory_prompt_minimal(Path::new("/test/memory"));
        assert!(
            !result.contains("## 记忆类型"),
            "minimal prompt should NOT contain full type taxonomy heading"
        );
        assert!(
            !result.contains("<types>"),
            "minimal prompt should NOT contain XML type definitions"
        );
        assert!(
            !result.contains("## 不应保存到记忆中的内容"),
            "minimal prompt should NOT contain what-not-to-save section"
        );
        assert!(
            !result.contains("## 如何保存记忆"),
            "minimal prompt should NOT contain detailed save instructions"
        );
    }

    #[test]
    fn minimal_prompt_nonexistent_dir_shows_empty_state() {
        let result = build_memory_prompt_minimal(Path::new("/nonexistent/memory/dir"));
        assert!(result.contains("当前为空"));
    }

    #[test]
    fn minimal_prompt_with_existing_index() {
        let tmp = tempfile::tempdir().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(
            mem_dir.join(ENTRYPOINT_NAME),
            "- [Role](user_role.md) \u{2014} senior engineer\n",
        )
        .unwrap();

        let result = build_memory_prompt_minimal(&mem_dir);
        assert!(result.contains("user_role.md"));
        assert!(result.contains("senior engineer"));
        assert!(!result.contains("当前为空"));
    }

    #[test]
    fn minimal_prompt_much_shorter_than_full() {
        let tmp = tempfile::tempdir().unwrap();
        let mem_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&mem_dir).unwrap();
        std::fs::write(mem_dir.join(ENTRYPOINT_NAME), "- [A](a.md) \u{2014} test\n").unwrap();

        let full = build_memory_prompt(&mem_dir);
        let minimal = build_memory_prompt_minimal(&mem_dir);

        assert!(
            minimal.len() < full.len() / 2,
            "minimal ({} chars) should be less than half of full ({} chars)",
            minimal.len(),
            full.len()
        );
    }

    #[test]
    fn full_prompt_contains_full_taxonomy() {
        let result = build_memory_prompt(Path::new("/test/memory"));
        assert!(
            result.contains("## 记忆类型"),
            "full prompt should contain type taxonomy"
        );
        assert!(
            result.contains("<types>"),
            "full prompt should contain XML type definitions"
        );
        assert!(
            result.contains("## 不应保存到记忆中的内容"),
            "full prompt should contain what-not-to-save"
        );
        assert!(
            result.contains("## 如何保存记忆"),
            "full prompt should contain save instructions"
        );
    }

    // -- no hardcoded platform paths -----------------------------------------

    #[test]
    fn constants_no_hardcoded_home_paths() {
        let all_text = [
            TYPES_SECTION,
            WHAT_NOT_TO_SAVE,
            FRONTMATTER_EXAMPLE,
            WHEN_TO_ACCESS,
            BEFORE_RECOMMENDING,
            PERSISTENCE_SECTION,
        ];
        for text in all_text {
            assert!(
                !text.contains("~/.config/tjuae"),
                "should not hardcode platform-specific path"
            );
            assert!(!text.contains("~/.claude"), "should not contain bb brand path");
        }
    }
}
