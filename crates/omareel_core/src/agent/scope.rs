//! Holding an agent's `omareel` to the recording it was started on.
//!
//! The editor (and `omareel agent`) runs a coding agent on content that can carry prompt
//! injection: speech in the transcript, text on screen. The agent is treated as untrusted, so
//! the runner sets [`SCOPE_ENV`] (`<bundle>\n<scratch>`) in its environment, and every
//! `omareel` it starts, whichever one it finds on its `$PATH`, checks its command line here
//! before doing anything ([`refusal`], called first thing in `main`). Inside a scope only these
//! run:
//!
//! - `describe <bundle>`
//! - `apply <bundle> <ops> [--dry-run] [--describe]`, the operations inline, on stdin, or in a
//!   file inside the bundle or the scratch directory; files the operations name (`insert`,
//!   `music`, a background image) must be inside them too ([`Scope::check_file`])
//! - `frame <bundle> … -o <file in the scratch directory>`
//! - `transcribe <bundle> [--force] [--backend NAME]`
//! - `help`
//!
//! Everything else (the launcher, `record`, `export`, `edit`, `skill`, `agent`, the internal
//! commands, another bundle, an image outside the scratch directory) is refused with a message
//! saying why. Paths are compared after resolving symlinks and `..`.

use std::path::{Path, PathBuf};

/// The environment variable: the bundle's path, a newline, the scratch directory's path.
pub const SCOPE_ENV: &str = "OMAREEL_AGENT_SCOPE";

/// The bundle and scratch directory an agent's `omareel` may touch, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Scope {
    pub bundle: PathBuf,
    pub scratch: PathBuf,
}

/// The variable's value for a run on `bundle` writing to `scratch`.
pub fn value(bundle: &str, scratch: &Path) -> String {
    format!("{bundle}\n{}", scratch.display())
}

fn resolve(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|error| format!("{}: {error}", path.display()))
}

impl Scope {
    /// The scope in the environment: `Ok(None)` outside one; an error when it is set but its
    /// bundle or scratch directory can't be found (so everything is refused).
    pub fn from_env() -> Result<Option<Scope>, String> {
        match std::env::var_os(SCOPE_ENV) {
            None => Ok(None),
            Some(value) => Scope::parse(&value.to_string_lossy()).map(Some),
        }
    }

    pub fn parse(value: &str) -> Result<Scope, String> {
        let (bundle, scratch) = value.split_once('\n').ok_or_else(|| format!("{SCOPE_ENV} is not <bundle>\\n<scratch>"))?;
        if bundle.is_empty() || scratch.is_empty() {
            return Err(format!("{SCOPE_ENV} is not <bundle>\\n<scratch>"));
        }
        Ok(Scope { bundle: resolve(Path::new(bundle))?, scratch: resolve(Path::new(scratch))? })
    }

    /// `arguments` (after the program's name) from `cwd`, or why they are refused.
    pub fn check(&self, arguments: &[String], cwd: &Path) -> Result<(), String> {
        let Some(command) = arguments.first() else {
            return Err("the launcher doesn't run from an agent".into());
        };
        let rest = &arguments[1..];
        match command.as_str() {
            "help" | "--help" | "-h" => Ok(()),
            "describe" => match rest {
                [bundle] => self.check_bundle(bundle, cwd),
                _ => Err("describe takes only the bundle here".into()),
            },
            "apply" => {
                let positional: Vec<&String> = rest.iter().filter(|arg| !matches!(arg.as_str(), "--dry-run" | "--describe")).collect();
                let [bundle, ops] = positional.as_slice() else { return Err("apply takes the bundle and its operations here".into()) };
                self.check_bundle(bundle, cwd)?;
                if ops.as_str() != "-" && !ops.trim_start().starts_with(['[', '{']) {
                    self.check_file(&cwd.join(ops.as_str()))?;
                }
                Ok(())
            }
            "frame" => {
                let request = crate::export::still::parse_frame_args(rest)?;
                self.check_bundle(&request.bundle, cwd)?;
                let count = if request.sheet { 1 } else if request.count > 0 { request.count as usize } else { request.times.len() };
                for output in crate::export::still::output_paths(&request.output, count.max(1)) {
                    self.check_output(&cwd.join(output))?;
                }
                Ok(())
            }
            "transcribe" => {
                let mut bundles = Vec::new();
                let mut i = 0;
                while i < rest.len() {
                    match rest[i].as_str() {
                        "--force" => {}
                        "--backend" => i += 1,
                        option if option.starts_with('-') => return Err(format!("unknown transcribe option {option}")),
                        bundle => bundles.push(bundle),
                    }
                    i += 1;
                }
                match bundles.as_slice() {
                    [bundle] => self.check_bundle(bundle, cwd),
                    _ => Err("transcribe takes only the bundle here".into()),
                }
            }
            other => Err(format!("`omareel {other}` doesn't run from an agent")),
        }
    }

    fn check_bundle(&self, bundle: &str, cwd: &Path) -> Result<(), String> {
        match resolve(&cwd.join(bundle)) {
            Ok(path) if path == self.bundle => Ok(()),
            _ => Err(format!("only the recording at {} can be used here, not {bundle}", self.bundle.display())),
        }
    }

    /// A file the agent's `omareel` may read: one inside the bundle or the scratch directory.
    pub fn check_file(&self, file: &Path) -> Result<(), String> {
        match resolve(file) {
            Ok(path) if path.starts_with(&self.bundle) || path.starts_with(&self.scratch) => Ok(()),
            Ok(_) => Err(format!("{} is outside the recording and the scratch directory", file.display())),
            Err(error) => Err(error),
        }
    }

    /// A file the agent's `omareel` may write: in the scratch directory (or a directory in it),
    /// and not a symlink.
    fn check_output(&self, file: &Path) -> Result<(), String> {
        let refused = || format!("images go only to {}, not {}", self.scratch.display(), file.display());
        let (Some(directory), Some(name)) = (file.parent(), file.file_name()) else { return Err(refused()) };
        if name == ".." || name == "." {
            return Err(refused());
        }
        let directory = resolve(directory).map_err(|_| refused())?;
        if !directory.starts_with(&self.scratch) {
            return Err(refused());
        }
        if std::fs::symlink_metadata(directory.join(name)).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!("{} is a symlink", file.display()));
        }
        Ok(())
    }
}

/// Why `arguments` (after the program's name) may not run in the scope the environment sets;
/// empty when they may, or when there is no scope.
pub fn refusal(arguments: &[String]) -> String {
    let check = || -> Result<(), String> {
        let Some(scope) = Scope::from_env()? else { return Ok(()) };
        let cwd = std::env::current_dir().map_err(|error| format!("no working directory: {error}"))?;
        scope.check(arguments, &cwd)
    };
    match check() {
        Ok(()) => String::new(),
        Err(error) => format!(
            "omareel: refused: {error}. A coding agent started by omareel may only run describe, apply, \
             frame (into its scratch directory) and transcribe on its recording, and help.\n"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;

    struct Fixture {
        _dir: TempDir,
        root: PathBuf,
        scope: Scope,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new("agent-scope");
        let root = PathBuf::from(dir.0.clone());
        for sub in ["Demo.omareel/media", "Other.omareel", "scratch/sheets", "outside"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("Demo.omareel/ops.json"), "[]").unwrap();
        std::fs::write(root.join("outside/ops.json"), "[]").unwrap();
        // A link from the bundle to the other one, and a link in the scratch directory out of it.
        std::os::unix::fs::symlink(root.join("Other.omareel"), root.join("Demo.omareel/link")).unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("scratch/out")).unwrap();
        std::os::unix::fs::symlink(root.join("outside/x.jpg"), root.join("scratch/trap.jpg")).unwrap();
        let scope = Scope::parse(&value(&root.join("Demo.omareel").to_string_lossy(), &root.join("scratch"))).unwrap();
        Fixture { _dir: dir, root, scope }
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    impl Fixture {
        fn check(&self, text: &str) -> Result<(), String> {
            self.scope.check(&args(&text.replace("$ROOT", &self.root.to_string_lossy())), &self.root.join("Demo.omareel"))
        }
    }

    #[test]
    fn allows_editing_this_recording() {
        let f = fixture();
        for allowed in [
            "help",
            "--help",
            "describe .",
            "describe $ROOT/Demo.omareel",
            "describe ../Demo.omareel/",
            "apply . -",
            "apply . [{\"op\":\"delete\"}]",
            "apply . ops.json --dry-run",
            "apply --describe . $ROOT/scratch/sheets/../../Demo.omareel/ops.json",
            "frame . --count 9 --sheet -o $ROOT/scratch/sheet.jpg",
            "frame . --at 1 --at 2 -o $ROOT/scratch/sheets/f.png",
            "frame . --at 1 -o ../scratch/f-%d.png",
            "transcribe .",
            "transcribe --force . --backend whisper",
        ] {
            assert_eq!(f.check(allowed), Ok(()), "{allowed}");
        }
    }

    #[test]
    fn refuses_everything_else() {
        let f = fixture();
        for (refused, why) in [
            ("", "launcher"),
            ("record", "`omareel record`"),
            ("record --region", "`omareel record`"),
            ("export . -o $ROOT/scratch/x.mp4", "`omareel export`"),
            ("edit .", "`omareel edit`"),
            ("skill install", "`omareel skill`"),
            ("skill", "`omareel skill`"),
            ("agent . polish", "`omareel agent`"),
            ("__record-gsr -w screen", "`omareel __record-gsr`"),
            ("__record-daemon", "`omareel __record-daemon`"),
            ("probe .", "`omareel probe`"),
            // Other bundles, however they are named.
            ("describe ../Other.omareel", "only the recording"),
            ("apply ../Other.omareel -", "only the recording"),
            ("apply link -", "only the recording"),
            ("apply ./link/.. -", "only the recording"),
            ("transcribe $ROOT/Other.omareel", "only the recording"),
            ("frame ../Other.omareel --at 1 -o $ROOT/scratch/a.png", "only the recording"),
            ("describe . extra", "only the bundle"),
            // Operations read from outside.
            ("apply . $ROOT/outside/ops.json", "outside the recording"),
            ("apply . ../outside/ops.json", "outside the recording"),
            // Images outside the scratch directory, through .. or a symlink.
            ("frame . --at 1 -o $ROOT/outside/a.png", "images go only to"),
            ("frame . --at 1 -o a.png", "images go only to"),
            ("frame . --at 1 -o $ROOT/scratch/../outside/a.png", "images go only to"),
            ("frame . --at 1 -o $ROOT/scratch/out/a.png", "images go only to"),
            ("frame . --at 1 -o $ROOT/scratch/trap.jpg", "is a symlink"),
            ("frame . --at 1 -o $ROOT/scratch/nowhere/a.png", "images go only to"),
            ("transcribe . --model x", "unknown transcribe option"),
        ] {
            let error = f.check(refused).expect_err(refused);
            assert!(error.contains(why), "{refused}: {error}");
        }
    }

    #[test]
    fn files_the_operations_name_stay_inside() {
        let f = fixture();
        assert!(f.scope.check_file(&f.root.join("Demo.omareel/ops.json")).is_ok());
        assert!(f.scope.check_file(&f.root.join("outside/ops.json")).is_err());
        assert!(f.scope.check_file(&f.root.join("scratch/sheets/../../outside/ops.json")).is_err());
        assert!(f.scope.check_file(&f.root.join("Demo.omareel/link")).is_err());
        assert!(f.scope.check_file(&f.root.join("missing.mp4")).is_err());
    }

    #[test]
    fn a_broken_scope_refuses_everything() {
        assert!(Scope::parse("just-one-path").is_err());
        assert!(Scope::parse("/nonexistent/bundle\n/nonexistent/scratch").is_err());
        assert!(Scope::parse("\n").is_err());
    }
}
