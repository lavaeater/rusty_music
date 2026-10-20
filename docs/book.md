14.1 ALL THAT JAZZ
We tend to think of music in terms of composers. Composers are
everywhere—in pop music, classical music, game music, electronic dance
music, and more. Composers create a piece of music for a set of musicians
(or machines) and then usually record a performance. When we hear a
piece of music, it’s usually the same every time, unless someone else is
performing it, remixing it, or rearranging it. This works fine with the way
we typically consume music, which is in a linear manner—listening to a
record, dancing in a club, or attending a performance.
235
236    ◾    Game Audio Programming 3
Video games and other interactive experiences are non-linear. Although
many interactive experiences utilize cutscenes and “baked” animation to
great effect, the player certainly notices when the game is not responding
to them. Providing nothing but pre-composed music in an experience is
akin to providing nothing but cutscenes. We probably need to look else-
where for inspiration for our interactive music systems.
Thankfully we have many forms of non-linear music in our music his-
tory corpus. Baroque keyboardists such as Bach were well known for their
ability to improvise and embellish while performing, and even written
counterpoint is thought to be heavily influenced by on-the-spot music
generation [1]. Hip hop artists—MCs, DJs, and dancers alike—have used
improvisation to great effect throughout the music’s history [2]. Perhaps
the most well-known modern example of a music which emphasizes
improvisation is jazz music, which itself is influenced by West African
improvisation techniques [3]. Jazz composers typically write a static piece
of music—a “head”—and then expect the musicians performing to impro-
vise the rest.
Music improvisation is the embellishment of musical ideas. Those ideas
can come from a composer, other musicians, or several musical “games”
used to generate the basis for the improvisation. You can hear many exam-
ples of this embellishment by searching for a famous jazz piece and listen-
ing to each musician’s version. Try “Autumn Leaves” by Joseph Kosma for
total overload.
14.2 MUSIC SYSTEM FOUNDATIONS, LIGHTNING ROUND
Music improvisation is not possible without some foundational systems.
Creating all these supporting systems could be its own chapter [4]. We
will briefly cover some of them in this section. Each example is provided
as pseudocode in hopes of easing the process of applying it to a given envi-
ronment. An implementation using Unity3d is provided in the supple-
mental materials at https://www.routledge.com/9780367348045.
14.2.1 Sound Generator
In order to make music, you’ll have to make some sound. We use all kinds
of sound generators—horns, bells, drums, synthesizers, and a huge variety
of software instruments.
There are many kinds of samplers with myriad settings. To demon-
strate this system, all you need is what I like to call the “one-shot” sampler.
Improvisational Music   ◾    237
A one-shot sampler takes a single audio file and plays the file at different
speeds based on incoming musical notes.
Let’s assume we always create tonal audio files at middle C (261.626 Hz,
MIDI note 60). If we wanted our sampler to play the file one octave above
middle C (523.251 Hz, MIDI note 72), we would tell it to play the file two
times as fast. Similarly, if we wanted to play one octave below middle C
(130.813 Hz, MIDI note 48), we would tell the sampler to play the file half
as fast. The formula for determining the playback speed based on a MIDI
note is
speed = 2(midiNote−60) /12
Every interactive audio engine you’ll encounter provides a way to play a
sound file at some pitch. In fact, most interactive audio engines are essen-
tially very complex samplers. To make a sampler instrument, you need
only to set up the existing audio file playback to respond to notes and scale
the speed like this:
Sampler:
SoundFile file
FilePlayer player
function Play(int noteNumber, float scheduledTime):
player.pitch = pow(2, (noteNumber – 60) / 12)
player.play(file, scheduledTime)
14.2.2 Clock
Most interactive music systems require something to tell time or to send
a signal when a musical interval is encountered. We generally call this a
clock or metronome (timer tends to imply something that’s not precise
enough for audio or musical timing). These musical clocks can be either
discrete or continuous—a decision which affects the design of the music
system.
For example, a traditional DAW timeline is typically continuous.
This requires plugins such as beat-synced effects to poll the timeline to
determine when beats are going to happen. Effects are required to detect
“edges” to demarcate musical events like a quarter note.
A pulse-based clock like one you might see in a modular synthesizer is
an example of a discrete musical clock. It sends out a “pulse” periodically,
238    ◾    Game Audio Programming 3
and pieces of the system use that pulse to trigger or otherwise manipulate
sound. Many times, using a discrete clock is a subtractive process. If the
initial pulse is at 16th note intervals, a clock divider in the chain might
take every fourth pulse, creating a quarter note pattern. An additional
clock divider could take every other quarter note, playing just the first
and third quarter notes in a measure or shifting to the second and fourth.
By chaining clock dividers and other logical modules, you can create all
kinds of musical patterns. Even with a discrete clock, though, you are by
no means limited to dividing the initial pulse (look up “clock multipliers”
for examples of this).
For this example, we’ll use a continuous clock, which looks something
like this:
Clock:
float tempo // in quarter notes per minute
float startTime // in seconds using the audio engine clock
bool playing
function Play():
startTime = currentEngineTime
playing = true
function Stop():
playing = false
// get the current time of the clock in bars (fractional)
function GetTimeBars():
if (!playing):
return 0
float now = currentEngineTime - startTime_
float timeQuarters = (tempo / 60) * timeSeconds
// stick to 4/4 time for this example
return timeQuarters / 4
// we’ll need this later for scheduling sampler plays
function BarsToEngineTime(float timeBars):
float quarters = bars * 4
float seconds = quarters / (tempo / 60)
return startTime_ + seconds
14.2.3  Sequencers
“Sequencer” is an overloaded term, even when you narrow it down to
musical uses. We could be talking about a MIDI sequencer tool like those
Improvisational Music   ◾    239
found in a DAW or a groove box step sequencer. And there are several
smaller kinds of sequencers that transform pitches, select pulses, and rese-
quence breakbeat samples. The general definition I like to use is that a
sequencer is something that processes control signals in a musical system,
much like how an effect processes an audio signal. A control signal could
be a pulse, a MIDI note, a knob, a chord, a sensor, or anything else that
could eventually manipulate a sound.
14.3 MUSICIAN RECIPES
Now that we have a concept of the underlying systems that enable real-
time composition, let’s think about how to build the whole thing. I like to
think of real-time music systems in terms of three layers:
•••The Conductor determines the overall shape of the music and gen-
erates control signals.
The Musicians process the control signals from the conductor.
The Instruments turn the control signals from the musicians into
sound.
Matching these layers up to the previous definitions, the Conductor would
be a clock and some logic for controlling the overall composition, the
Musicians would be sequencers, and an Instrument would be a sampler.
14.3.1 Designing the Conductor
We’ve taken care of the sampler instrument, and we’ll get to the musi-
cians shortly. Let’s briefly look at what our conductor will do. We know
that we’ll need timing info from the clock. We’ll also want to know what
notes to play for any tonal instruments, for which we can provide some
information about the key or chord changes in the music. Two pieces of
information we can use are the note number (or pitch) itself, as well as
how likely it should be to play. We could go with a direct probability value
for notes, but we’ll get a little more context if we introduce a concept of
“strength.” In music, there are notes in a chord or scale that can be played
very often. Those are typically the root note and some of the other chord
tones (such as the fifth). And then there are notes that belong in the scale
but will sound dissonant if played often or on strong beats such as the
downbeat of a measure. We can assign a strength value to notes and then
240    ◾    Game Audio Programming 3
use that in our musician logic for selecting notes to play. If we describe a
note like so:
Note:
int noteNumber // MIDI note number
float strength // 0-1, how “comfortable” or “strong” is this note?
And then describe a chord as a collection of notes with a position in bars:
Chord:
float posBars
Note[] chordNotes
Note[] scaleNotes
Using this information, we can have the conductor cycle through chord
changes and notify the musicians:
Conductor:
Clock clock
Chord[] chords // chords, sorted by position
float chordLengthBars // at what point do we loop?
Musician[] musicians // we’ll get to this in a minute
function Update():
if (!clock.playing):
return
float timeBars = clock.GetTimeBars()
// get the current chord
float chordTimeBars = mod(timeBars, chordLengthBars)
Chord currentChord
for each chord in chords:
if (chord.posBars > chordTimeBars):
break
currentChord = chord
// update the musicians
for each musician in musicians:
musician.SetChord(currentChord)
musician.UpdateNotes(timeBars)
14.3.2 Musician Design Considerations
So, what do these musicians do? What kind of logic do they contain?
Remember that a sequencer can be any kind of control processor. Following
that logic, we can combine any number of existing musical techniques to
Improvisational Music   ◾    241
create a complete, “intelligent” sequencer for each type of musician we
would like to emulate. How you put those together depends on your proj-
ect’s requirements. If you’re making a one-off music system for a game,
it might be more efficient to hand-code each musician. If you’re making
a tool for sound creatives, you could create a system of sequencer mod-
ules and let your users patch them together to design musicians. In this
section, we will talk about the former: programming bespoke musician
logic. After learning how to assemble musical logic in this way, it should
be straightforward to port these techniques to a modular system, but that
will be left as an exercise for the reader.
Our musician interface, as referenced by the conductor above, looks
like this:
Musician:
Clock clock // hang on to a reference so we can do conversions
Chord currentChord
float lastTimeBars
float intensity // 0.0-1.0, the amount of musical movement
function UpdateNotes(float posBars):
// to be implemented in each musician to follow
function SetChord(Chord chord):
currentChord = chord
We’ll build on this for each of the individual musicians, and some addi-
tional “homework” for extending each musician will be found at the end
of each section.
14.3.3 Funky Drummer
Many interactive compositions call for a rhythm section, so let’s make a
drumbeat generator. This “drummer” will output a beat based on a couple
of inputs—tempo and intensity. For the purposes of this example, we will
assume 4/4 timing.
A drumbeat can be conceptually broken down into the following:
•••The drums in a drum kit and their roles in constructing a beat.
Strong and weak beats and their effect on the “feel” when hit (or not
hit).
Embellishment, such as fills or shifted hits.
242    ◾    Game Audio Programming 3
FIGURE 14.1 A regular-time drum beat in 4/4.
FIGURE 14.2 A drum beat in 4/4 with notes added.
A very simple drum kit might include a kick drum, snare drum, and hi-
hat. The kick anchors the beat, so its most prominent hits usually end
up on strong beats, such as the downbeat of every measure. The snare
compliments the kick by providing a “back beat,” typically landing on the
weak beats of the measure (beats 2 and 4 in 4/4 time). The hi-hat fills
in space and, depending on the feel, can land pretty much anywhere in
the measure. Figure 14.1 shows one example of a regular-time drum beat
in 4/4.
We could just play this beat back over and over, but that wouldn’t be
very interesting. One way to vary a beat is to add and remove notes to
make it feel more or less “intense” or “busy,” as shown in Figure 14.2.
As the intensity input changes, we add or remove these embellishments.
Building on the musician interface above, a drummer that plays an embel-
lished beat could look like this:
Drummer:
Sampler kick, snare, hat
Note[] kickNotes, snareNotes, hatNotes
function UpdateNotes(float posBars):
// find out the positions in 16th notes
int last16th = floor(lastTimeBars * 16) % 16
int this16th = floor(timeBars * 16) % 16
Improvisational Music   ◾    243
// update the position
lastTimeBars = timeBars
// skip if we haven't advanced
if (last16th == this16th):
return
// find the hit time (in audio engine time)
int wholeBar = floor(timeBars)
float barFraction = this16th / 16
float hitTime = clock.BarsToEngineTime(wholeBar + barFraction)
// find the notes that are in this index
// and play them if they're at the right level
float minStrength = 1.0f - intensity
Note tmpNote = kickNotes[this16th]
if (tmpNote.strength >= minStrength):
kick.Play(tmpNote.noteNumber, hitTime)
tmpNote = snareNotes[this16th]
if (tmpNote.strength >= minStrength):
snare.Play(tmpNote.noteNumber, hitTime)
tmpNote = hatNotes[this16th]
if (tmpNote.strength >= minStrength):
hat.Play(tmpNote.noteNumber, hitTime)
Homework
• Add more instruments.
• Add fills that could play every few bars.
• Omit or add embellishments using randomness to add variation.
• Switch between half, normal, and double time feels based on intensity.
14.3.4 All About That Bass
It’s helpful to have a musician to anchor the key of the music and to pro-
vide a basis for the listener to recognize the improvisation of the other
tonal instruments. In modern Western music, a bass line typically serves
that purpose.
For each of the tonal instruments, let’s add some helpers in a subclass
of the existing musician:
TonalMusician:
Sampler sampler // we just need one for these
function GetNote(Note[] notes, float minStrength):
Note[] tmpNotes
244    ◾    Game Audio Programming 3
for each note in notes:
if (note.strength >= minStrength):
add note to tmpNotes
int idx = randomInt(0, tmpNotes.size)
return tmpNotes[i]
function GetChordNote(float minStrength):
return GetNote(currentChord.chordNotes, minStrength)
function GetScaleNote(float minStrength):
return GetNote(currentChord.scaleNotes, minStrength)
function PlayNote(int noteNumber, float timeBars):
float engineTime = clock.BarsToEngineTime(timeBars)
sampler.Play(noteNumber, engineTime)
Real bassists play around in the key quite a bit. But for the purpose of this
example, let’s assume the bass line should stick within the chord tones and
primarily the root note of the chord. The bass musician might look like this:
Bass:
function UpdateNotes(float timeBars):
// find out the positions in 16th notes
int last16th = floor(lastTimeBars * 16) % 16
int this16th = floor(timeBars * 16) % 16
lastTimeBars = timeBars
// skip if we haven’t advanced
if (last16th == this16th):
return
// find the note time (in bars)
int wholeBar = floor(timeBars)
float barFraction = this16th / 16.0f
float noteTime = wholeBar + barFraction
// play roots on the downbeat
if (this16th == 0):
PlayNote(GetChordNote(1.0f), noteTime)
return
// usually play strong tones on the other quarters
if (this16th % 4 == 0):
if (random0to1() < intensity):
PlayNote(GetChordNote(0.5f), noteTime)
return
Improvisational Music   ◾    245
// sometimes play on the 8ths
if (this16th % 2 == 0):
if (random0to1() < intensity - 0.25f):
PlayNote(GetChordNote(0.25f), noteTime)
return
// sometimes play on the 16ths
if (random0to1() < intensity - 0.5f):
PlayNote(GetChordNote(0), noteTime)
Homework
• Use scale tones for embellishments, instead of just chord tones.
• Add “memory” to provide a sense of repetition.
14.3.5 Spacey Chimes
Texture provides harmonic context and movement. While many times
you’ll hear a guitar or piano play full chords, another approach to add tex-
ture is to arpeggiate the chords. Simply put, an arpeggio is a sequence of
notes that move around in a chord. If you spent any time in music lessons,
you probably practiced running scales and arpeggios. If not, the typical
arpeggio goes up, then down a scale, skipping every other note, as shown
in the figure below.
So, let’s look at how to make an arpeggiator. Much of the logic from the
bass generator can be reused to create a chord arpeggiator like so:
Chimes:
Mode mode // Up, Down, Random
int sequenceIdx = 0
function UpdateNotes(float timeBars):
// set the step size based on intensity
int stepSize
if (intensity < 0.4f):
stepSize = 4
else if (intensity < 0.7f):
246    ◾    Game Audio Programming 3
stepSize = 8
else:
stepSize = 16
// find out the positions based on the step size
int lastStep = floor(lastTimeBars * stepSize) % stepSize
int thisStep = floor(timeBars * stepSize) % stepSize
lastTimeBars = timeBars
// skip if we haven’t advanced
if (lastStep == thisStep):
return
// find the note time (in bars)
int wholeBar = floor(timeBars)
float barFraction = thisStep / stepSize
float noteTime = wholeBar + barFraction
// get the index within the sequence
int numChordNotes = currentChord.chordNotes.length
int chordNoteIdx
switch (mode):
case Mode.Up:
sequenceIdx = (sequenceIdx + 1) % numChordNotes
chordNoteIdx = sequenceIdx
break
case Mode.Down:
sequenceIdx = (sequenceIdx + 1) % numChordNotes
chordNoteIdx = numChordNotes - (sequenceIdx + 1)
break
case Mode.Random:
chordNoteIdx = randomInt(0, numChordNotes)
break
int note = currentChord.chordNotes[chordNoteIdx].noteNumber
PlayNote(note, noteTime);
Homework
• Add more arpeggio modes, such as an up/down mode.
• Add optional scale runs in addition to chord arpeggios.
• Tie the mode to the intensity.
14.3.6 The Soloist
Now that we have a rhythmic and harmonic base, a melody will complete
our “band.” In jazz music, there are often pre-composed melodies in the
Improvisational Music   ◾    247
head provided to the musician. As the performance progresses, solo musi-
cians take turns reinterpreting these melodies or creating their own com-
pletely. The same strong and weak concepts regarding tones and rhythmic
intervals apply here. Playing a weak tone on a strong beat can create ten-
sion, whereas sticking to strong tones can ground the melody. Too much
tension, and the music can become challenging to listen to. Conversely,
too little tension can make the music feel bland and uninteresting.
Another consideration is repetition. You may have noticed that the bass
and chime instruments can meander and sometimes sound a bit too ran-
dom. This is often acceptable for those types of instruments, but with a
melody, too much wandering can leave the piece feeling without basis.
One way to solve this is by introducing some “memory” to the musician
logic. For example, in the first four bars, we could use similar stochas-
tic methods as the bass musician to generate a melody. After that melody
has been generated, we could store it and reuse it for another four bars to
create a repeating eight-bar phrase.
The solo instrument might then look like this:
Solo:
int recordBars = 4 // the length of the recording
int repeats = 1 // how many times we want to repeat the recording
int[] recordedMelody
float repeatStartBar
function UpdateNotes(float timeBars):
// find out the positions in 16th notes
int last16th = floor(lastTimeBars * 16) % 16
int this16th = floor(timeBars * 16) % 16
lastTimeBars = timeBars;
// skip if we haven't advanced
if (last16th == this16th):
return
// find the note time (in bars)
int wholeBar = floor(timeBars)
float barFraction = this16th / 16
float noteTime = wholeBar + barFraction
// get the index within the recording
int recordingIdx = this16th + (wholeBar % recordBars) * 16
// repeat the recording until the next time
// we should generate a melody
248    ◾    Game Audio Programming 3
float repeatEndBars = repeatStartBar_ + repeats * recordBars
if (timeBars < repeatEndBars):
int note = recordedMelody_[recordingIdx];
if (note != InvalidNoteNumber):
PlayNote(note, noteTime)
// if we’re recording, generate the melody
else:
if (recordingIdx == 0):
init recordedMelody with 16 * recordBars notes
int note = InvalidNoteNumber
// always play a strong note on the downbeat
if (this16th == 0):
note = GetScaleNote(1.0f)
// usually play a note on the quarters
else if (this16th % 4 == 0):
if (random0to1() < intensity):
note = GetScaleNote(0.5f)
// sometimes play a note on the eighths
else if (this16th % 2 == 0):
if (random0to1() < intensity - 0.25f):
note = GetScaleNote(0.25f)
// sometimes play a note on the 16ths
else if (random0to1() < intensity - 0.5f):
note = GetScaleNote(0.0f)
// record and play the note
recordedMelody_[recordingIdx] = note
if (note != InvalidNoteNumber):
PlayNote(note, noteTime)
// if we’re done recording, start repeating
int lastRecordingIdx = recordBars * 16 - 1
if (recordingIdx >= lastRecordingIdx):
repeatStartBar_ = Mathf.Ceil(timeBars)
Homework
• Modify the sampler and the solo musician to play sustained notes.
• Keep track of two recorded melodies to play an AABA sequence.
14.4 WRAPPING UP
There are so many ways to design interactive music that it can become
overwhelming, but the process becomes a little simpler by looking at how
Improvisational Music   ◾    249
humans make music on the spot. Jazz music provides a useful framework
for autonomous music, thanks to its focus on improvisation and well-
defined theory. Leaning on the composer, musician, instrument meta-
phor further grounds the concept such that we can visualize how the code
components come together. Hopefully, these concepts and examples help
you decide how to make your own improvised music systems.
