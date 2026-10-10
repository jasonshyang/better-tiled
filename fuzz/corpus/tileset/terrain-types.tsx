<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.2" tiledversion="1.2.4" name="ground" tilewidth="32" tileheight="32" tilecount="5" columns="5">
 <image source="../../src/automap-tiles.png" width="160" height="32"/>
 <terraintypes>
  <terrain name="grass" tile="0"/>
  <terrain name="water" tile="-1">
   <properties>
    <property name="deep" type="bool" value="true"/>
   </properties>
  </terrain>
 </terraintypes>
 <tile id="0" terrain="0,0,0,0"/>
 <tile id="1" terrain="0,1,,1"/>
 <tile id="2" terrain="1,1,1,1" probability="0.5"/>
</tileset>
