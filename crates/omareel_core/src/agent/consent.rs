//! Which coding agents the person let the editor run on their recordings: the first Polish
//! (the top bar's button, Ctrl+Shift+P) only opens the drawer, which says which agent runs and
//! what goes to its provider, with a Start button; once started, later runs of that agent
//! start straight from the shortcut, until the drawer's "Ask first" takes the permission back.
//!
//! Kept in `$XDG_CONFIG_HOME/omareel/agent.json`: `{"allowed": ["claude"]}`.

use crate::compat::qdir;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::qstandardpaths::{self, Location};

/// `omareel/agent.json` in the configuration directory.
pub fn path() -> String {
    qdir::file_path(&qstandardpaths::writable_location(Location::GenericConfig), "omareel/agent.json")
}

fn read(file: &str) -> Object {
    std::fs::read(file).ok().and_then(|bytes| qjson::object_from_json(&bytes)).unwrap_or_default()
}

fn allowed_list(object: &Object) -> Vec<String> {
    object.value("allowed").as_array().unwrap_or(&[]).iter().filter_map(|id| id.as_str().map(str::to_owned)).collect()
}

/// Whether the agent `id` may start without asking.
pub fn allowed(id: &str) -> bool {
    allowed_in(&path(), id)
}

pub fn allowed_in(file: &str, id: &str) -> bool {
    allowed_list(&read(file)).iter().any(|allowed| allowed == id)
}

/// Lets the agent `id` start without asking, or takes that back.
pub fn set_allowed(id: &str, allowed: bool) -> Result<(), String> {
    set_allowed_in(&path(), id, allowed)
}

pub fn set_allowed_in(file: &str, id: &str, allowed: bool) -> Result<(), String> {
    let mut object = read(file);
    let mut list = allowed_list(&object);
    list.retain(|known| known != id);
    if allowed {
        list.push(id.to_owned());
    }
    object.insert("allowed", Value::array(list.into_iter().map(Value::from).collect()));
    if let Some(directory) = std::path::Path::new(file).parent() {
        std::fs::create_dir_all(directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    }
    crate::compat::qsavefile::save(file, object.to_json(Format::Indented).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;

    #[test]
    fn remembers_each_agent_until_taken_back() {
        let dir = TempDir::new("agent-consent");
        let file = dir.join("config/omareel/agent.json");
        assert!(!allowed_in(&file, "claude"));
        set_allowed_in(&file, "claude", true).unwrap();
        assert!(allowed_in(&file, "claude"));
        assert!(!allowed_in(&file, "codex"), "permission is per agent");
        set_allowed_in(&file, "codex", true).unwrap();
        set_allowed_in(&file, "claude", false).unwrap();
        assert!(!allowed_in(&file, "claude"));
        assert!(allowed_in(&file, "codex"));
        // Other keys are kept; a broken file allows nothing.
        std::fs::write(&file, br#"{"allowed": ["codex"], "other": 1}"#).unwrap();
        set_allowed_in(&file, "claude", true).unwrap();
        let object = read(&file);
        assert_eq!(object.value("other").to_int(0), 1);
        std::fs::write(&file, b"{not json").unwrap();
        assert!(!allowed_in(&file, "codex"));
    }
}
