use bevy::prelude::Resource;
use crate::musicians::{Chord, MusicPlayer};

#[derive(Resource)]
pub struct Conductor {
    pub chords: Vec<Chord>
}
