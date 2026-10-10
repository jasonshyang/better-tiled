<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.2" tiledversion="1.2.4" name="ground" tilewidth="32" tileheight="32" tilecount="5" columns="5">
 <image source="../../src/automap-tiles.png" width="160" height="32"/>
 <tile id="2" probability="0.5"/>
 <wangsets>
  <wangset name="Terrains" type="corner" tile="-1">
   <wangcolor name="grass" color="#ff0000" tile="0" probability="1"/>
   <wangcolor name="water" color="#00ff00" tile="-1" probability="1">
    <properties>
     <property name="deep" type="bool" value="true"/>
    </properties>
   </wangcolor>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="1" wangid="0,2,0,2,0,0,0,1"/>
   <wangtile tileid="2" wangid="0,2,0,2,0,2,0,2"/>
  </wangset>
 </wangsets>
</tileset>
