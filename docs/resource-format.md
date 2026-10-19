# Discworld resource evidence

The supplied image has ISO volume label DISCWORLD, contains 189 files, and has SHA256 `5354f74582375b1f6350e33b4dfd5df6b2bcd79a30f471a9307e017235570e05`. The full inventory is in `evidence/iso-manifest.json`.

`/DISCWLD/DW.SCN` is 776188 bytes. Its first 5000 bytes have MD5 `70955425870c7720d6eebed903b2ef41`. Together with ENGLISH.SMP, these match the English DOS CD v2 detection entry. This is Tinsel V1, not the Discworld 2 engine.

The reference is ScummVM commit `96b42d42bc8533953bdd701db059cc33c839a9ff`. Relevant sources:

- [Release detection](https://github.com/scummvm/scummvm/blob/96b42d42bc8533953bdd701db059cc33c839a9ff/engines/tinsel/detection_tables.h)
- [Resource index and loading](https://github.com/scummvm/scummvm/blob/96b42d42bc8533953bdd701db059cc33c839a9ff/engines/tinsel/handle.cpp)
- [Chunk lookup](https://github.com/scummvm/scummvm/blob/96b42d42bc8533953bdd701db059cc33c839a9ff/engines/tinsel/scn.cpp)

## Resource index

The ISO identifier INDEX. normalizes to INDEX. It contains 20-byte records for this DOS release. Byte offsets 0 through 11 contain the resource filename, terminated by zero when shorter than 12 bytes. Offsets 12 through 15 contain a little-endian word: its low 24 bits are the file size and its high byte contains flags. Offsets 16 through 19 are a historical pointer field, ignored by the reference loader and this reader.

The flag byte uses bit 0 for preload, bit 1 for discardable, bit 2 for sound, bit 3 for graphics, bit 4 for compression, and bit 5 for loaded state. Resource identifiers depend on record order, so the parser preserves it. Compression is not yet implemented.

## Scene chunk structure

Each chunk starts with a little-endian 32-bit type and a little-endian 32-bit absolute offset of the next chunk header. Zero ends the chain. Payload starts eight bytes after the header. The reader reports spans between headers; these can include alignment padding and are not a substitute for validation within each payload decoder.

The parser currently requires forward, four-byte-aligned links within the resource and caps the chunk count. This is a deliberately narrow reader for the supplied DOS release. Broader format support requires additional evidence.

The supplied image passes this structural validation for all 149 index entries, covering 1732 chunk headers. Every indexed resource exists and matches its declared size; none sets the resource compression flag. `evidence/resource-manifest.json` records the resource order, flags, and chunk spans. This establishes structural consistency, not correctness of the payload decoders that remain to be implemented.

## Images and palettes

For this release, handles encode a resource index in the upper nine bits and a byte offset in the lower 23 bits. Image records occupy 16 bytes: signed 16-bit width, unsigned 16-bit height, signed 16-bit animation X and Y offsets, then 32-bit bitmap and palette handles. The image chunk is type 0x33340006.

Bitmap data is a row-major grid of 16-bit tile identifiers, rounded up to four pixels in each dimension. The bitmap resource's words at offsets 16 and 20 provide the tile dictionary byte offset and transparent dictionary adjustment. A nonnegative tile identifier selects an opaque 4 by 4 block. A high-bit identifier selects a transparent block after the dictionary adjustment; low bits zero skip the entire block. Within transparent blocks, pixel zero leaves the destination unchanged. Opaque zero must remain distinguishable, so the decoder stores coverage separately.

Palettes begin with a 32-bit color count followed by four bytes per RGB color. The standalone exporter places the first palette color at pixel index one and reserves zero for black, matching the initial foreground palette allocation. Runtime palette relocation and scene-specific palette updates are still required.

The title export was visually inspected at 320 by 200 pixels. All 66621 image records pass indexed tile decoding; evidence/image-audit.json records aggregate hashes per resource. This audit excludes palette correctness and reference pixel equivalence. References are the pinned [graphics.cpp](https://github.com/scummvm/scummvm/blob/96b42d42bc8533953bdd701db059cc33c839a9ff/engines/tinsel/graphics.cpp), [handle.cpp](https://github.com/scummvm/scummvm/blob/96b42d42bc8533953bdd701db059cc33c839a9ff/engines/tinsel/handle.cpp), and [palette.cpp](https://github.com/scummvm/scummvm/blob/96b42d42bc8533953bdd701db059cc33c839a9ff/engines/tinsel/palette.cpp).

## Reader limits

The ISO reader validates primary descriptors, mirrored endian fields, extent bounds, directory cycles, and normalized paths. It rejects multi-extent, extended-attribute, and interleaved records. It does not implement Joliet, Rock Ridge, or general-purpose extraction. The native inspector currently reads the entire image into memory; browser asset import will need a different access strategy to avoid unnecessary copies of the large speech archive.
