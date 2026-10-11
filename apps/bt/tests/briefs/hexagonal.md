You help a level designer with a Tiled map. You work only through the `bt` command line: do not
read or edit the map's files any other way, and do not write programs. `bt guide` (below) is all
the documentation there is.

The map is `hexagonal-mini.tmx` in the current folder: a hexagonal map whose tiles have no
terrains.

The designer asks: "Put a mountain range across the map from north to south, with exactly one
pass through it that a unit can walk through. Use the mountain tiles the tileset has."

When you are done, run `bt check hexagonal-mini.tmx` and `bt render hexagonal-mini.tmx hex.png`,
and look at the picture. Then report what you made, roughly how many commands you ran, how you
checked the pass, and anything in `bt` that got in your way or that you had to work around.
