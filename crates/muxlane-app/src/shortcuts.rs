use crate::actions::{
    CloseTab, NewShellTab, NextTab, PreviousTab, SelectTab1, SelectTab2, SelectTab3, SelectTab4,
    SelectTab5, SelectTab6, SelectTab7, SelectTab8, SelectTab9, TogglePalette,
};
use gpui::{App, KeyBinding, Keystroke};
use muxlane_store::PersistedShortcutBindings;
use std::collections::HashMap;

#[cfg(target_os = "macos")]
pub(crate) const FIXED_CHORDS: &[&str] = &[
    "cmd-k",
    "ctrl-shift-t",
    "ctrl-tab",
    "ctrl-shift-tab",
    "cmd-1",
    "cmd-2",
    "cmd-3",
    "cmd-4",
    "cmd-5",
    "cmd-6",
    "cmd-7",
    "cmd-8",
    "cmd-9",
];

#[cfg(not(target_os = "macos"))]
pub(crate) const FIXED_CHORDS: &[&str] = &[
    "super-k",
    "ctrl-shift-t",
    "ctrl-tab",
    "ctrl-shift-tab",
    "super-1",
    "super-2",
    "super-3",
    "super-4",
    "super-5",
    "super-6",
    "super-7",
    "super-8",
    "super-9",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShortcutAction {
    CloseTab,
    PreviousTab,
    NextTab,
}

impl ShortcutAction {
    pub(crate) const ALL: [Self; 3] = [Self::CloseTab, Self::PreviousTab, Self::NextTab];

    pub(crate) fn build_binding(self, chord: &str) -> KeyBinding {
        match self {
            Self::CloseTab => KeyBinding::new(&expand_platform_chord(chord), CloseTab, None),
            Self::PreviousTab => KeyBinding::new(&expand_platform_chord(chord), PreviousTab, None),
            Self::NextTab => KeyBinding::new(&expand_platform_chord(chord), NextTab, None),
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::CloseTab => "settings.shortcut.close_tab",
            Self::PreviousTab => "settings.shortcut.previous_tab",
            Self::NextTab => "settings.shortcut.next_tab",
        }
    }

    pub(crate) fn binding(self, bindings: &PersistedShortcutBindings) -> &Option<String> {
        match self {
            Self::CloseTab => &bindings.close_tab,
            Self::PreviousTab => &bindings.previous_tab,
            Self::NextTab => &bindings.next_tab,
        }
    }

    pub(crate) fn set_binding(
        self,
        bindings: &mut PersistedShortcutBindings,
        value: Option<String>,
    ) {
        match self {
            Self::CloseTab => bindings.close_tab = value,
            Self::PreviousTab => bindings.previous_tab = value,
            Self::NextTab => bindings.next_tab = value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShortcutError {
    Invalid,
    MultipleChords,
    Conflict(String),
}

pub(crate) fn captured_chord(keystroke: &Keystroke) -> Result<String, ShortcutError> {
    canonical_chord(&keystroke.unparse())
}

fn canonical_chord(source: &str) -> Result<String, ShortcutError> {
    let mut chords = source.split_whitespace();
    let chord = chords.next().ok_or(ShortcutError::Invalid)?;
    if chords.next().is_some() {
        return Err(ShortcutError::MultipleChords);
    }
    let expanded = expand_platform_chord(chord);
    let parsed = Keystroke::parse(&expanded).map_err(|_| ShortcutError::Invalid)?;
    Ok(canonical_platform_chord(&parsed.unparse()))
}

fn expand_platform_chord(chord: &str) -> String {
    let Some(key) = chord.strip_prefix("platform-") else {
        return chord.to_string();
    };
    #[cfg(target_os = "macos")]
    let modifier = "cmd";
    #[cfg(not(target_os = "macos"))]
    let modifier = "super";
    format!("{modifier}-{key}")
}

fn canonical_platform_chord(chord: &str) -> String {
    #[cfg(target_os = "macos")]
    let modifier = "cmd-";
    #[cfg(not(target_os = "macos"))]
    let modifier = "super-";
    chord
        .strip_prefix(modifier)
        .map_or_else(|| chord.to_string(), |key| format!("platform-{key}"))
}

pub(crate) fn normalize(
    bindings: &PersistedShortcutBindings,
) -> Result<PersistedShortcutBindings, ShortcutError> {
    let mut normalized = bindings.clone();
    let fixed: HashMap<_, _> = FIXED_CHORDS
        .iter()
        .map(|chord| {
            (
                canonical_chord(chord).expect("fixed shortcut must parse"),
                *chord,
            )
        })
        .collect();
    let mut configured = HashMap::<String, ShortcutAction>::new();

    for action in ShortcutAction::ALL {
        let Some(source) = action.binding(bindings) else {
            continue;
        };
        let chord = canonical_chord(source)?;
        if fixed.contains_key(&chord) || configured.insert(chord.clone(), action).is_some() {
            return Err(ShortcutError::Conflict(chord));
        }
        action.set_binding(&mut normalized, Some(chord));
    }

    Ok(normalized)
}

pub(crate) fn install_binding(
    cx: &mut App,
    current: &PersistedShortcutBindings,
    action: ShortcutAction,
    binding: Option<String>,
) -> Result<PersistedShortcutBindings, ShortcutError> {
    let mut candidate = current.clone();
    action.set_binding(&mut candidate, binding);
    install_keymap(cx, &candidate)
}

pub(crate) fn install_keymap(
    cx: &mut App,
    bindings: &PersistedShortcutBindings,
) -> Result<PersistedShortcutBindings, ShortcutError> {
    let normalized = normalize(bindings)?;
    let keymap = build_keymap(&normalized);
    cx.clear_key_bindings();
    cx.bind_keys(keymap);
    Ok(normalized)
}

pub(crate) fn install_keymap_or_defaults(
    cx: &mut App,
    bindings: &PersistedShortcutBindings,
) -> PersistedShortcutBindings {
    install_keymap(cx, bindings).unwrap_or_else(|_| {
        install_keymap(cx, &PersistedShortcutBindings::default())
            .expect("default shortcuts must form a valid keymap")
    })
}

fn build_keymap(bindings: &PersistedShortcutBindings) -> Vec<KeyBinding> {
    let mut keymap = vec![
        KeyBinding::new(&expand_platform_chord("platform-k"), TogglePalette, None),
        KeyBinding::new("ctrl-shift-t", NewShellTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new(&expand_platform_chord("platform-1"), SelectTab1, None),
        KeyBinding::new(&expand_platform_chord("platform-2"), SelectTab2, None),
        KeyBinding::new(&expand_platform_chord("platform-3"), SelectTab3, None),
        KeyBinding::new(&expand_platform_chord("platform-4"), SelectTab4, None),
        KeyBinding::new(&expand_platform_chord("platform-5"), SelectTab5, None),
        KeyBinding::new(&expand_platform_chord("platform-6"), SelectTab6, None),
        KeyBinding::new(&expand_platform_chord("platform-7"), SelectTab7, None),
        KeyBinding::new(&expand_platform_chord("platform-8"), SelectTab8, None),
        KeyBinding::new(&expand_platform_chord("platform-9"), SelectTab9, None),
    ];

    for action in ShortcutAction::ALL {
        if let Some(chord) = action.binding(bindings) {
            keymap.push(action.build_binding(chord));
        }
    }
    keymap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_all_configurable_actions() {
        let defaults = normalize(&PersistedShortcutBindings::default()).unwrap();
        let chords: Vec<_> = ShortcutAction::ALL
            .into_iter()
            .map(|action| action.binding(&defaults).clone())
            .collect();
        assert_eq!(
            chords,
            [
                Some("ctrl-w".into()),
                Some("platform-left".into()),
                Some("platform-right".into())
            ]
        );
    }

    #[test]
    fn disabled_bindings_are_omitted_from_the_complete_keymap() {
        let bindings = PersistedShortcutBindings {
            close_tab: None,
            previous_tab: None,
            next_tab: None,
            ..Default::default()
        };
        let normalized = normalize(&bindings).unwrap();
        let remaining_configurable = ShortcutAction::ALL
            .into_iter()
            .filter(|action| action.binding(&normalized).is_some())
            .count();
        assert_eq!(
            build_keymap(&normalized).len(),
            FIXED_CHORDS.len() + remaining_configurable
        );
    }

    #[test]
    fn malformed_and_multiple_chords_are_rejected() {
        let bindings = PersistedShortcutBindings {
            close_tab: Some("ctrl--w".into()),
            ..Default::default()
        };
        assert_eq!(normalize(&bindings), Err(ShortcutError::Invalid));
        let bindings = PersistedShortcutBindings {
            close_tab: Some("ctrl-w ctrl-q".into()),
            ..Default::default()
        };
        assert_eq!(normalize(&bindings), Err(ShortcutError::MultipleChords));
    }

    #[test]
    fn configurable_duplicates_and_fixed_conflicts_are_rejected() {
        let bindings = PersistedShortcutBindings {
            next_tab: Some("platform-left".into()),
            ..Default::default()
        };
        assert_eq!(
            normalize(&bindings),
            Err(ShortcutError::Conflict("platform-left".into()))
        );
        let bindings = PersistedShortcutBindings {
            next_tab: Some("platform-k".into()),
            ..Default::default()
        };
        assert_eq!(
            normalize(&bindings),
            Err(ShortcutError::Conflict("platform-k".into()))
        );
    }

    #[test]
    fn candidate_update_is_validated_as_a_complete_set() {
        let current = PersistedShortcutBindings::default();
        let mut candidate = current.clone();
        ShortcutAction::CloseTab.set_binding(&mut candidate, Some("platform-k".into()));
        assert_eq!(
            normalize(&candidate),
            Err(ShortcutError::Conflict("platform-k".into()))
        );
        assert_eq!(current, PersistedShortcutBindings::default());
    }

    #[test]
    fn complete_keymap_always_contains_every_fixed_binding() {
        let bindings = PersistedShortcutBindings {
            close_tab: None,
            previous_workspace: None,
            next_workspace: None,
            previous_tab: None,
            next_tab: None,
        };
        let keymap = build_keymap(&bindings);
        assert_eq!(keymap.len(), FIXED_CHORDS.len());
        for chord in FIXED_CHORDS {
            assert!(
                Keystroke::parse(chord).is_ok(),
                "invalid fixed chord: {chord}"
            );
        }
    }
}
