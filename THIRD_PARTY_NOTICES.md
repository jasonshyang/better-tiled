# Third-party notices

better-tiled is licensed per crate; each crate's `Cargo.toml` names its license, and `README.md`
says which files hold the license texts. This file lists the code in the workspace that follows
or reproduces someone else's work, with the notices their licenses ask to be kept.

| Crate | Follows | License of the original |
|---|---|---|
| `crates/bt-utils` (`src/raster.rs`) | Alois Zingl's "Bresenham Curve Rasterizing Algorithms" | MIT |
| `crates/bt-core` (the grid geometry, the custom-type rules, the tileset arithmetic) | libtiled, Tiled's core library | BSD-2-Clause |
| `crates/bt-tmx` (the readers and writers of Tiled's formats) | libtiled's `MapReader`, `MapWriter` and variant converters | BSD-2-Clause |
| `crates/bt-tmx` (`src/deflate.rs`) | zlib's `deflate.c` and `trees.c` (an altered version) | zlib |
| `crates/bt-tmx` (`src/json/print.rs`) | the layout of Tiled's `JsonWriter` (`plugins/json/qjsonparser/json.cpp`) | permissive, below |

Tiled's editor (`src/tiled` in Tiled's repository) is licensed under the GNU GPL; no code
derived from it is in a crate licensed under MIT or Apache-2.0. The crates that port it say so in
their documentation and are licensed under the GPL.

## Bresenham Curve Rasterizing Algorithms — MIT License

<https://github.com/zingl/Bresenham>

Copyright (c) Alois Zingl

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT
OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## libtiled — BSD 2-Clause License

<https://github.com/mapeditor/tiled> (`src/libtiled`)

Copyright 2008-2026, Thorbjørn Lindeijer and libtiled contributors.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

   1. Redistributions of source code must retain the above copyright notice,
      this list of conditions and the following disclaimer.

   2. Redistributions in binary form must reproduce the above copyright
      notice, this list of conditions and the following disclaimer in the
      documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE CONTRIBUTORS ``AS IS'' AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO
EVENT SHALL THE CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## zlib — zlib License

<https://zlib.net>. `crates/bt-tmx/src/deflate.rs` is an altered version of zlib's source code
(ported to Rust and reduced to the code paths Tiled's compression uses), not the original
software.

Copyright (C) 1995-2026 Jean-loup Gailly and Mark Adler

This software is provided 'as-is', without any express or implied
warranty.  In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.

Jean-loup Gailly        Mark Adler
jloup@gzip.org          madler@alumni.caltech.edu

## Tiled's JSON writer

<https://github.com/mapeditor/tiled> (`src/plugins/json/qjsonparser/json.cpp`). The layout
`crates/bt-tmx/src/json/print.rs` reproduces is that writer's.

Copyright (c) 2010 Girish Ramakrishnan <girish@forwardbias.in>

Use, modification and distribution is allowed without limitation,
warranty, liability or support of any kind.
