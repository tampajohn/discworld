# DOS V1 polygons

The reader follows Poly::nextPoly in the pinned ScummVM polygons.cpp. V1 records are 104 bytes: type; four X and four Y coordinates; tag X/Y/text; node X/Y; film; refer type and ID; two scales; reel and Z factor; node count; X-array, Y-array, and line-array offsets; script handle. All words are little-endian. Coordinates and coefficients are signed.

Disk types are 0 path, 1 node path, 2 block, 3 refer, 4 effect, 5 exit, 6 tag. These differ from the runtime polygon-type enum. Auxiliary offsets are relative to the start of the polygon table, not individual records. Each node-path segment stores ten signed line coefficients; the reader preserves them. Node and line arrays are checked against the containing chunk and cannot overlap the fixed table.

Run the complete archive audit:

```sh
cargo run --release -p discworld-tools -- audit-polygons "Terry Pratchett's Discworld (1995)(Psygnosis).iso"
```

evidence/polygon-audit.json records 2033 polygons, 1687 nodes, and 886 non-null script handles. All script handles were checked against bytecode chunks. The general script-prefix probe now includes polygon scripts. This establishes structural decoding, not hit testing, path adjacency, walking, scale interpolation, or event fidelity. Those runtime behaviors remain to be implemented before scenes with polygons can execute.
