//! `.glicol` voice patches as Bevy assets, with hot reload.
//!
//! Spawn a [`GlicolSynth`] (for example through [`glicol_musician`]); once its
//! patch has loaded it becomes a playing [`SynthNode`]. When the file changes
//! (with Bevy's `file_watcher` feature), the running synth switches to the new
//! patch: notes already sounding finish on the old one, new notes use the new
//! one. A patch with errors is reported and the synth keeps playing the last
//! good version.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use bevy_seedling::prelude::{AudioContext, FirewheelNode};

use super::glicol::{parse_patch, GlicolError};
use super::{swap_patch, SynthConfig, SynthInstrument, SynthNode, SynthPatch};
use crate::musicians::{MusicPlayer, Musician};

/// A parsed `.glicol` voice patch.
#[derive(Asset, TypePath, Debug, Clone)]
pub struct GlicolPatch(pub SynthPatch);

#[derive(Default, TypePath)]
pub struct GlicolPatchLoader;

impl AssetLoader for GlicolPatchLoader {
    type Asset = GlicolPatch;
    type Settings = ();
    type Error = GlicolError;

    async fn load(&self, reader: &mut dyn Reader, _: &(), load_context: &mut LoadContext<'_>) -> Result<GlicolPatch, GlicolError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|e| GlicolError(e.to_string()))?;
        let source = String::from_utf8(bytes).map_err(|e| GlicolError(e.to_string()))?;
        let path = load_context.path().path().to_path_buf();
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("glicol");
        parse_patch(&source, name)
            .map(GlicolPatch)
            .map_err(|e| GlicolError(format!("{}: {e}", path.display())))
    }

    fn extensions(&self) -> &[&str] {
        &["glicol"]
    }
}

/// A synth playing a `.glicol` patch asset.
#[derive(Component, Debug, Clone)]
pub struct GlicolSynth {
    pub patch: Handle<GlicolPatch>,
    pub voices: usize,
    pub volume_db: f32,
}

impl GlicolSynth {
    pub fn new(patch: Handle<GlicolPatch>) -> Self {
        Self { patch, voices: 8, volume_db: 0.0 }
    }

    pub fn volume_db(self, volume_db: f32) -> Self {
        Self { volume_db, ..self }
    }

    pub fn voices(self, voices: usize) -> Self {
        Self { voices, ..self }
    }
}

/// A musician playing a `.glicol` patch, `transpose` semitones from where the
/// player writes its notes. It rests until the patch has loaded.
pub fn glicol_musician(name: impl Into<String>, player: impl MusicPlayer + 'static, transpose: f32, synth: GlicolSynth) -> impl Bundle {
    (Musician::new(name.into(), player).with_instrument(SynthInstrument::transposed(transpose)), synth)
}

pub(super) fn plugin(app: &mut App) {
    app.init_asset::<GlicolPatch>()
        .init_asset_loader::<GlicolPatchLoader>()
        .add_systems(Update, (start_glicol_synths, reload_glicol_synths));
}

/// Turn loaded patches into synth nodes.
fn start_glicol_synths(mut commands: Commands, synths: Query<(Entity, &GlicolSynth), Without<SynthConfig>>, patches: Res<Assets<GlicolPatch>>) {
    for (entity, synth) in &synths {
        if let Some(GlicolPatch(patch)) = patches.get(&synth.patch) {
            let node = SynthNode::for_patch(patch).volume_db(synth.volume_db);
            let config = SynthConfig { patch: patch.clone(), voices: synth.voices };
            commands.entity(entity).insert((node, config));
        }
    }
}

/// Swap running synths onto edited patches, without rebuilding their audio nodes.
fn reload_glicol_synths(
    mut events: MessageReader<AssetEvent<GlicolPatch>>,
    patches: Res<Assets<GlicolPatch>>,
    mut synths: Query<(&GlicolSynth, &FirewheelNode, &mut SynthNode)>,
    mut audio: ResMut<AudioContext>,
) {
    for event in events.read() {
        let AssetEvent::Modified { id } = event else { continue };
        let Some(GlicolPatch(patch)) = patches.get(*id) else { continue };
        for (synth, node, mut params) in &mut synths {
            if synth.patch.id() != *id {
                continue;
            }
            // Knobs may have been added, removed or reordered: start them at the new defaults.
            let fresh = SynthNode::for_patch(patch);
            if params.params != fresh.params {
                params.params = fresh.params;
            }
            swap_patch(&mut audio, node, patch, synth.voices);
        }
    }
}
