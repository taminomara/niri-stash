//! The desired-state file (plan D-13).
//!
//! The file says what each tracked window *should* be, never what it was last seen as. It is
//! written before the first niri command of anything that changes it.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::geometry::{Edge, Geometry};
use crate::core::WindowId;

/// What a tracked window should be. Serialised flat into its entry as `"desired":"stashed"` plus
/// `"edge":…`, or `"desired":"restored"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "desired", rename_all = "lowercase")]
pub(crate) enum DesiredState {
    Stashed { edge: Edge },
    Restored,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct WindowEntry {
    pub(crate) original: Geometry,
    #[serde(flatten)]
    pub(crate) desired: DesiredState,
}

/// The whole file. `windows` is keyed by the niri window id (serialised as a decimal string).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StateFile {
    pub(crate) instance: String,
    pub(crate) windows: BTreeMap<WindowId, WindowEntry>,
}

/// What `load` found.
#[derive(Debug, PartialEq)]
pub(crate) enum LoadOutcome {
    /// The file was read and belongs to this niri instance.
    Usable(StateFile),
    /// No file, an unreadable file, or a file from another niri instance; start empty.
    /// The string says why, for the log.
    StartEmpty(String),
}

/// Read the file at `path` for niri instance `instance`.
pub(crate) fn load(path: &Path, instance: &str) -> LoadOutcome {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return LoadOutcome::StartEmpty(format!("no state file at {}", path.display()))
        }
        Err(e) => return LoadOutcome::StartEmpty(format!("cannot read {}: {e}", path.display())),
    };
    match serde_json::from_str::<StateFile>(&text) {
        Ok(state) if state.instance != instance => LoadOutcome::StartEmpty(format!(
            "state file belongs to niri instance {}, current is {instance}",
            state.instance
        )),
        Ok(state) => LoadOutcome::Usable(state),
        Err(e) => LoadOutcome::StartEmpty(format!("cannot parse {}: {e}", path.display())),
    }
}

/// Write `state` to `path` atomically: write `<path>.tmp`, then rename over `path`. No fsync.
pub(crate) fn save(path: &Path, state: &StateFile) -> io::Result<()> {
    let json = serde_json::to_string(state)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = tmp_path(path);
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// `<path>.tmp`: the same directory, the file name plus ".tmp".
fn tmp_path(path: &Path) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const INSTANCE: &str = "/run/user/1000/niri.wayland-1.1572.sock";

    /// A state-file path inside a fresh directory unique to this test and process.
    fn temp_state_path(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("niri-stash-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("state.json")
    }

    fn geo(x: f64, y: f64, w: f64, h: f64) -> Geometry {
        Geometry { x, y, w, h }
    }

    fn example_state() -> StateFile {
        let mut windows = BTreeMap::new();
        windows.insert(
            12,
            WindowEntry {
                original: geo(540.0, 300.0, 800.0, 500.0),
                desired: DesiredState::Stashed { edge: Edge::Left },
            },
        );
        StateFile {
            instance: INSTANCE.to_string(),
            windows,
        }
    }

    #[test]
    fn saved_file_loads_back_as_current_with_equal_contents() {
        let path = temp_state_path("roundtrip");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let state = example_state();
        save(&path, &state).unwrap();
        assert_eq!(load(&path, INSTANCE), LoadOutcome::Usable(state));
    }

    #[test]
    fn missing_file_loads_fresh_naming_the_path() {
        let path = temp_state_path("missing");
        let expected = format!("no state file at {}", path.display());
        assert_eq!(load(&path, INSTANCE), LoadOutcome::StartEmpty(expected));
    }

    #[test]
    fn file_from_another_instance_loads_fresh_naming_both_instances() {
        let path = temp_state_path("other-instance");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        save(&path, &example_state()).unwrap();
        let current = "/run/user/1000/niri.wayland-1.2000.sock";
        let expected =
            format!("state file belongs to niri instance {INSTANCE}, current is {current}");
        assert_eq!(load(&path, current), LoadOutcome::StartEmpty(expected));
    }

    #[test]
    fn unparseable_file_loads_fresh() {
        let path = temp_state_path("garbage");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        match load(&path, INSTANCE) {
            LoadOutcome::StartEmpty(reason) => assert!(
                reason.contains(&path.display().to_string()),
                "reason should name the path: {reason}"
            ),
            other => panic!("expected StartEmpty, got {other:?}"),
        }
    }

    #[test]
    fn save_creates_the_parent_dir_and_leaves_no_tmp_file() {
        let path = temp_state_path("tmpfile");
        assert!(!path.parent().unwrap().exists());
        save(&path, &example_state()).unwrap();
        assert!(path.is_file(), "state file should exist");
        let tmp = path.with_file_name("state.json.tmp");
        assert!(!tmp.exists(), "tmp file should have been renamed away");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["state.json".to_string()]);
    }

    #[test]
    fn worked_example_serialises_byte_for_byte() {
        const EXAMPLE: &str = r#"{"instance":"/run/user/1000/niri.wayland-1.1572.sock","windows":{"12":{"original":{"x":540.0,"y":300.0,"w":800.0,"h":500.0},"desired":"stashed","edge":"left"}}}"#;
        assert_eq!(serde_json::to_string(&example_state()).unwrap(), EXAMPLE);

        let path = temp_state_path("example");
        save(&path, &example_state()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);

        let mut restored = example_state();
        let entry = restored.windows.get_mut(&12).unwrap();
        entry.desired = DesiredState::Restored;
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            r#"{"instance":"/run/user/1000/niri.wayland-1.1572.sock","windows":{"12":{"original":{"x":540.0,"y":300.0,"w":800.0,"h":500.0},"desired":"restored"}}}"#
        );
    }
}
