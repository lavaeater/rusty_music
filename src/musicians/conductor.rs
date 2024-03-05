use bevy::prelude::Resource;
use crate::musicians::{Chord};

#[derive(Resource)]
pub struct Conductor {
    pub chords: Vec<Chord>
}
