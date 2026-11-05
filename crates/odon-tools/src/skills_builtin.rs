//! Skills that ship inside the ODON binary.
//!
//! `.odon/skills` is gitignored, so a built-in skill stored there would never
//! travel with the app. These are compiled in with `include_str!` and surfaced
//! by `skill_list` / `skill_run`; a user skill with the same slug always wins,
//! so an operator can override the house procedure without touching the code.

#[derive(Debug, Clone, Copy)]
pub struct BuiltinSkill {
    pub slug: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub body: &'static str,
}

pub const BUILTIN_SKILLS: &[BuiltinSkill] = &[
    BuiltinSkill {
        slug: "game-project",
        title: "Building a playable browser game as a workspace project",
        description: "Scaffold and ship a runnable browser game: project layout, CORS preview server, Three.js/canvas rules, assets, and the QA checklist.",
        body: include_str!("../assets/skills/game-project.md"),
    },
    BuiltinSkill {
        slug: "procedural-3d-gltf",
        title: "Procedural game models as glTF",
        description: "Build deterministic low-poly game assets with model3d_create: part grammar, Y-up metres, vertex colours, recipes, and GLTFLoader usage.",
        body: include_str!("../assets/skills/procedural-3d-gltf.md"),
    },
    BuiltinSkill {
        slug: "agents-office-app",
        title: "Building a standalone ODON Agents Office app",
        description: "Map real ODON sessions/subagents to an isometric office, adopt the loopback token, consume SSE, and avoid inventing departments or status.",
        body: include_str!("../assets/skills/agents-office-app.md"),
    },
];

pub fn builtin_skills() -> &'static [BuiltinSkill] {
    BUILTIN_SKILLS
}

pub fn find_builtin_skill(slug: &str) -> Option<&'static BuiltinSkill> {
    BUILTIN_SKILLS.iter().find(|skill| skill.slug == slug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_skills_are_complete_and_unique() {
        let mut slugs = HashSet::new();
        for skill in builtin_skills() {
            assert!(slugs.insert(skill.slug), "duplicate slug: {}", skill.slug);
            assert!(!skill.title.trim().is_empty());
            assert!(!skill.description.trim().is_empty());
            assert!(
                skill.body.len() > 500,
                "{} body looks truncated",
                skill.slug
            );
            assert!(
                skill.body.starts_with('#'),
                "{} must begin with a markdown heading",
                skill.slug
            );
        }
    }

    #[test]
    fn builtin_lookup_is_exact() {
        assert!(find_builtin_skill("game-project").is_some());
        assert!(find_builtin_skill("procedural-3d-gltf").is_some());
        assert!(find_builtin_skill("agents-office-app").is_some());
        assert!(find_builtin_skill("Game Project").is_none());
        assert!(find_builtin_skill("../../etc/passwd").is_none());
    }
}
