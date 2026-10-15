<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="tiles" tilewidth="32" tileheight="32" tilecount="4" columns="2">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="1" type="Door">
  <properties>
   <property name="locked" type="bool" value="true"/>
  </properties>
 </tile>
 <wangsets>
  <wangset name="ground" type="corner" tile="-1">
   <wangcolor name="grass" color="#4a9c4a" tile="0" probability="1"/>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
  </wangset>
 </wangsets>
</tileset>
