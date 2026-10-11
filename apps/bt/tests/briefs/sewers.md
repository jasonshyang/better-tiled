You help a level designer with a Tiled map. You work only through the `bt` command line: do not
read or edit the map's files any other way, and do not write programs. `bt guide` (below) is all
the documentation there is.

The map is `sewers.tmx` in the current folder. Its look comes from AutoMap rules (the `rules.txt`
beside it): the designer paints the rough layout on one layer and the rules draw the sewer.

The designer asks: "Add a new room east of the existing ones, about 8 by 6, joined to the rest by
a corridor, and let the rules draw it. I want the rules to keep running as I change the layout
later."

When you are done, run `bt check sewers.tmx` and `bt render sewers.tmx sewers.png`, and look at
the picture. Then report what you made, roughly how many commands you ran, and anything in `bt`
that got in your way or that you had to work around.
