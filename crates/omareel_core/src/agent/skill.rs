//! The agent skill that ships inside omareel (skill/SKILL.md): `omareel skill` prints it, and
//! `omareel skill install` puts it where coding agents look for skills, the way Omarchy's own
//! skills are installed: the file in `~/.agents/skills/omareel`, linked from each agent's own
//! skills directory that exists.

use std::path::{Path, PathBuf};

/// The skill, as this version of omareel describes itself.
pub const SKILL: &str = include_str!("../../../../skill/SKILL.md");

/// The agents' skills directories Omarchy links its skills into, under the home directory.
/// `~/.agents/skills` itself holds the file.
const AGENT_SKILL_DIRS: [&str; 5] =
    [".claude/skills", ".codex/skills", ".pi/agent/skills", ".gemini/config/skills", ".hermes/skills"];

/// Writes the skill under `home` and links it into the agents' skills directories. Returns a
/// line for each place it went or was skipped.
pub fn install(home: &Path) -> Result<Vec<String>, String> {
    let directory = home.join(".agents/skills/omareel");
    // A link there is someone's own (a checkout they work on): writing through it would
    // change their file.
    if let Ok(target) = std::fs::read_link(&directory) {
        return Err(format!("{} is a link to {}; leaving it alone", directory.display(), target.display()));
    }
    std::fs::create_dir_all(&directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let file = directory.join("SKILL.md");
    std::fs::write(&file, SKILL).map_err(|error| format!("cannot write {}: {error}", file.display()))?;
    let mut report = vec![format!("installed {}", file.display())];
    for relative in AGENT_SKILL_DIRS {
        let skills = home.join(relative);
        if !skills.is_dir() {
            continue;
        }
        let link = skills.join("omareel");
        report.push(link_skill(&link, &directory));
    }
    Ok(report)
}

/// Points `link` at the installed skill. A link to an `.agents/skills/omareel` (this one, or one
/// from a home directory since moved) is replaced; anything else there is someone's own.
fn link_skill(link: &Path, target: &PathBuf) -> String {
    match std::fs::symlink_metadata(link) {
        Ok(metadata) if !metadata.file_type().is_symlink() => {
            return format!("skipped {} (something else is there)", link.display());
        }
        Ok(_) => {
            let current = std::fs::read_link(link).unwrap_or_default();
            if &current == target {
                return format!("linked {}", link.display());
            }
            if !current.ends_with(".agents/skills/omareel") {
                return format!("skipped {} (a link to {})", link.display(), current.display());
            }
            if let Err(error) = std::fs::remove_file(link) {
                return format!("skipped {}: {error}", link.display());
            }
        }
        Err(_) => {}
    }
    match std::os::unix::fs::symlink(target, link) {
        Ok(()) => format!("linked {}", link.display()),
        Err(error) => format!("skipped {}: {error}", link.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;

    #[test]
    fn the_skill_names_itself_and_its_commands() {
        assert!(SKILL.starts_with("---\nname: omareel\ndescription: |\n"));
        for command in ["omareel describe", "omareel apply", "omareel frame", "omareel export"] {
            assert!(SKILL.contains(command), "{command}");
        }
    }

    #[test]
    fn installs_once_and_links_where_agents_look() {
        let home = TempDir::new("skill-home");
        let home_path = Path::new(&home.0);
        std::fs::create_dir_all(home_path.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(home_path.join(".codex/skills/omareel")).unwrap();
        std::fs::create_dir_all(home_path.join(".pi/agent/skills")).unwrap();
        std::os::unix::fs::symlink("/somewhere/dev/omareel-skill", home_path.join(".pi/agent/skills/omareel")).unwrap();
        let report = install(home_path).unwrap();
        let installed = home_path.join(".agents/skills/omareel");
        assert_eq!(std::fs::read_to_string(installed.join("SKILL.md")).unwrap(), SKILL);
        assert_eq!(std::fs::read_link(home_path.join(".claude/skills/omareel")).unwrap(), installed);
        assert!(report.iter().any(|line| line.starts_with("skipped") && line.contains(".codex")), "{report:?}");
        assert_eq!(std::fs::read_link(home_path.join(".pi/agent/skills/omareel")).unwrap(), Path::new("/somewhere/dev/omareel-skill"));
        assert!(!home_path.join(".gemini/config/skills").exists());
        // Again: the same links, the file rewritten.
        std::fs::write(installed.join("SKILL.md"), "old").unwrap();
        install(home_path).unwrap();
        assert_eq!(std::fs::read_to_string(installed.join("SKILL.md")).unwrap(), SKILL);
        assert_eq!(std::fs::read_link(home_path.join(".claude/skills/omareel")).unwrap(), installed);
    }
}
