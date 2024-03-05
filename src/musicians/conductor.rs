use bevy::prelude::Resource;
use crate::musicians::{Chord, MusicPlayer};

#[derive(Resource)]
pub struct Conductor {
    pub musicians: Vec<Box<dyn MusicPlayer>>,
    pub chords: Vec<Chord>
}
