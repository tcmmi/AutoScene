# Reading shots: who is seen, who speaks

You label shots of one TV episode from still pictures. You work alone. Read
only the files your task names: the cast pages, your sheets, this file, and
your own output file and folder. Open nothing else under this folder: other
readers label the same shots and must not influence you, nor you them.

## What you get

- **Cast pages** (`cast_page_0.jpg` … `cast_page_2.jpg`): the people who have
  a label. Under each group of up to three face pictures stands the label,
  for example `michael` or `heavy_inmate`. Look at all three pages before you
  start, and look again whenever you are not sure who someone is.
- **Sheets** (`sheets/sheet_NNN.jpg`): each row is one shot, six pictures
  spread evenly over it from left to right. Under the row's first picture:
  `shot 123    8:16    2.9 s long    speech 2.8 s`. A long shot has two or
  three rows; the later ones say `shot 123 goes on` and belong to the same
  shot. `speech` is how many seconds of speech are heard during the shot
  (measured from the soundtrack; you cannot hear it).

## What you write, per shot

One line per shot, three fields with a tab between them:

```
shot	faces	speaks
123	veronica	veronica
124	home_man	home_man
125	veronica	-
126	home_man	-
127	veronica	-
```

**faces**: everyone whose face is seen in any picture of the shot, at least
in part. Labels with commas between them, no spaces; `-` when no face is
seen.

- Use the cast page's label when the face is that person.
- Write `other` (once, however many there are) when a face is seen that is
  not on the cast pages, or that you cannot match to a label with fair
  confidence. A wrong label is worse than `other`.
- A face counts when you can see it as a face: front, side, half hidden,
  partly outside the picture, small in the background if you can still tell
  it is a face. The back of a head is not a face. A crowd of far figures
  without visible faces is not a face.
- A face in a photograph, a newspaper or on a screen counts as seen.
- Someone who is in only one of the six pictures still counts.

**speaks**: who of the faces you listed speaks during the shot. `-` when
nobody you listed speaks.

- Only when the caption's `speech` is more than 0.0 s. With `speech 0.0 s`
  always write `-`.
- Mark a person when their mouth shows talking: open in some pictures, or
  changing its shape between pictures. Compare the six pictures.
- Do not mark a listener: someone shown with a closed, unmoving mouth while
  speech is heard is listening to a voice from outside the picture. Then
  write `-`. This is common: do not mark someone only because they are the
  one in the picture.
- Shouting, laughing with words, singing count as speaking. Chewing,
  breathing hard, a still smile do not.
- `other` may speak too (write `other` when an unlabelled face talks).
- Everyone you write under speaks must also stand under faces.
- When you cannot tell whether a mouth moves, leave the person out.

## Subtitle lines under a shot (only on sheets that have them)

Under a shot's pictures may stand the subtitle lines that are on screen
during it, in their order, cut from the film. `<<` before a line: it began
before this shot. `>>` after a line: it goes on after this shot. A shot with
no lines under it has none on screen.

- A line tells you that someone speaks at that time, and what is said. It
  does not tell you who, unless a name stands before the words.
- The rule stays the mouth: mark a person as speaking when the pictures show
  them talking.
- Use the words to tell a speaker from a listener. When the line is said TO
  the person in the picture (it names them, asks them, answers what they
  said in the shot before) and their mouth is closed, they listen: do not
  mark them.
- When the mouth cannot be read (too small, turned away, hidden) and the
  words can only be theirs, mark them. When you cannot tell, leave them out.
  A line under the shot is no reason by itself to mark whoever is in the
  picture.
- A name in capitals before the words (`OFFICER 2: Go.`) says who speaks
  them. Subtitles do that mostly when the speaker is not in the picture.
- Two lines that each begin with a dash are two speakers. Slanted letters
  are usually a voice that is not in the room: a telephone, a loudspeaker,
  a television, a thought, someone outside the picture.
- A line with `<<` or `>>` may belong mostly to the shot beside this one.
- The caption still counts: with `speech 0.0 s` write `-`, even when a line
  stands under the shot.

You may know the series. Use that only to recognise a face, and only with
the labels of the cast pages. Label what the pictures show, not what the
story suggests.

## How to work

- Look at every sheet at full size. If a face or a mouth is too small, cut
  the row out and enlarge it (Python with Pillow, saved into your own folder
  only), then look again before you decide.
- After each sheet, save: your output file must then hold a line for every
  shot of the sheets you have done so far, in shot order, with the header
  line `shot	faces	speaks` first. Use real tab characters.
- Every shot on your sheets gets exactly one line. `sheets/rows.tsv` lists
  which shots are on which sheet (`sheet shot …`); you may read it to check
  that you missed none.
- When done, answer in two lines: how many shots you wrote, and anything
  that went wrong (a sheet you could not read, shots you were unsure of).
