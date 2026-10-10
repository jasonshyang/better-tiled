(function(name,data){
 if(typeof onTileMapLoaded === 'undefined') {
  if(typeof TileMaps === 'undefined') TileMaps = {};
  TileMaps[name] = data;
 } else {
  onTileMapLoaded(name,data);
 }
 if(typeof module === 'object' && module && module.exports) {
  module.exports = data;
 }})("javascript",
{ "compressionlevel":-1,
 "height":1,
 "infinite":false,
 "layers":[
        {
         "data":[1, 0],
         "height":1,
         "id":1,
         "name":"ground",
         "opacity":1,
         "type":"tilelayer",
         "visible":true,
         "width":2,
         "x":0,
         "y":0
        }],
 "nextlayerid":2,
 "nextobjectid":1,
 "orientation":"orthogonal",
 "renderorder":"right-down",
 "tiledversion":"1.12.2",
 "tileheight":32,
 "tilesets":[
        {
         "columns":5,
         "firstgid":1,
         "image":"..\/..\/src\/automap-tiles.png",
         "imageheight":32,
         "imagewidth":160,
         "margin":0,
         "name":"rules",
         "spacing":0,
         "tilecount":5,
         "tileheight":32,
         "tilewidth":32
        }],
 "tilewidth":32,
 "type":"map",
 "version":"1.11",
 "width":2
});