# Sorting shots without a known face

You sort shots of one TV episode from still pictures. You work alone. Read
only the files your task names: this file, your sheets, and your own output
file and folder. Open nothing else: other readers sort the same shots and
must not influence you, nor you them.

## What you get

**Sheets** (`sheet_NNN.jpg`): each row is one shot to sort. The three
pictures in the yellow frame are that shot, early, middle and late. Left of
the frame is one picture of the shot before it in the film, right of it one
picture of the shot after it; they are there so that you see what goes on
around the shot. Under the frame: `shot 123    8:16    2.9 s long`.

## What you write, per shot

One line per shot, two fields with a tab between them:

```
shot	kind
123	talk
124	place
```

`kind` is one of four words. Ask: what is this shot to the shots around it?

- `talk`: it is part of the conversation or action around it. Someone who is
  part of it is in the picture, seen from behind, too small, turned away,
  half hidden or out of focus; or the picture shows what they do, hold, look
  at or talk about (hands, an object, a paper, a screen, a door they go
  through, the thing they point at).
- `place`: it shows where or when: a wide or outside view of the place
  without a person one could know, a building, a street, the sky, a title, a
  graphic, a card with words, credits.
- `other`: it shows people or action that the shots around it are not about:
  a crowd, passers-by, another storyline cut in between.
- `nothing`: nothing to see: black, a blur, a flash, a fade.

Rules for hard cases:

- Judge from the pictures, the shot before and the shot after. When the shot
  before and the shot after show two people talking and this shot shows one
  of them from behind, or their hands, or what they look at, it is `talk`.
- A wide view that opens a scene and shows no one you could know is `place`,
  even when the people of the scene are tiny figures in it. When the same
  people are plainly the subject of the wide view (you can tell who is who
  from the shots around it and they are doing what the scene is about), it
  is `talk`.
- When you cannot decide between `talk` and something else, do not write
  `talk`.
- You may know the series. Judge what the pictures show, not what the story
  suggests.

## How to work

- Look at every sheet at full size. If a picture is too small to judge, cut
  the row out and enlarge it (Python with Pillow, saved into your own folder
  only), then look again.
- After each sheet, save: your output file must then hold a line for every
  shot of the sheets you have done so far, in shot order, with the header
  line `shot	kind` first. Use real tab characters.
- Every shot on your sheets gets exactly one line. `rows.tsv` in the sheets'
  folder lists which shots are on which sheet (`sheet shot …`); you may read
  it to check that you missed none.
- When done, answer in two lines: how many shots you wrote, and anything
  that went wrong or that you were unsure of.
