<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="water" tilewidth="32" tileheight="32" tilecount="5" columns="5">
 <image source="../../src/automap-tiles.png" width="160" height="32"/>
 <tile id="0">
  <properties>
   <property name="animation-delay0" type="int" value="10"/>
   <property name="animation-delay1" type="int" value="25"/>
   <property name="animation-frame0" type="int" value="0"/>
   <property name="animation-frame1" type="int" value="3"/>
  </properties>
  <animation>
   <frame tileid="0" duration="100"/>
   <frame tileid="3" duration="250"/>
  </animation>
 </tile>
</tileset>
