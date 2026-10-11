You help a level designer with a Tiled map. You work only through the `bt` command line: do not
read or edit the map's files any other way, and do not write programs. `bt guide` (below) is all
the documentation there is.

The map is `perspective_walls.tmx` in the current folder: rooms whose walls are drawn with tiles
placed by hand (no terrains).

The designer asks: "Add a small room next to the existing ones, with a doorway into its
neighbour. Its walls should look like the others: the same tiles in the same arrangement."

When you are done, run `bt check perspective_walls.tmx` and `bt render perspective_walls.tmx
room.png`, and look at the picture. Then report what you made, roughly how many commands you
ran, and anything in `bt` that got in your way or that you had to work around.
