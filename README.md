# rusty_music
A dynamic improvisational music player for Bevy

Check out the simple example to see how to add some drums, bass and soloists.

This is not a complete implementation and is a work in progress. 
The easiest things to change are bpms, beats per measure and note_type. 

BTW, I don't understand any of those concepts, you have to live with that.

It also adds an intensity resource that is supposed to be an f32 between 0 and 1,
and if you change that one, as in the example, the point is that you can 
intensify the music.

So you could control intensity by counting the number of enemies, level of health,
whatever. It's pretty cool actually. 

NOTICE: I have added support for bevy_fundsp, however the author had,
at the time I wrote this plugin, not updated to bevy 0.13 - so I forked it and
updated it myself. I have submitted that update as a PR to the author of
bevy_fundsp, if he accepts it or updates the crate himself I will of course
change the dependency to the official crate instead.

All the samples included in this repository are royalty-free that I downloaded
from some music site with sample packs. 