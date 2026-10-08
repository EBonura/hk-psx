# typetrees.tsv

Unity's built-in class type trees for the two engine versions the Windows
Hollow Knight install ships: 6000.0.61f1 (every level, sharedassets,
resources and globalgamemanagers file) and 6000.0.51f1 (`Resources/unity
default resources`). The player build strips type trees from its serialized
files, so a reader needs these from outside the game.

Coverage: every class id that appears in any serialized file of the install
(76 trees: 68 for 6000.0.61f1, 8 for 6000.0.51f1), not only the 248 files the
admitted scenes reach.

Source: the TPK type tree package that UnityPy 1.25.3 bundles as
`UnityPy/resources/lzma.tpk` (sha256
`0b2277765f0f7a6253df04426abd83d2bf37f8d1ad30542cddd2d81ae484741b`),
decoded with tpk_ar 0.2.4 through `UnityPy.helpers.Tpk.get_typetree_node`,
which picks the class version for the file's Unity version. The TPK format and
data come from AssetRipper's TypeTreeDumps; UnityPy and tpk_ar are MIT
licensed (Copyright (c) 2019-2026 K0lb3). The extraction was a one-off
read of that package; nothing here is generated at build time.

Format, one record per class:

    class <TAB> <unity version> <TAB> <class id> <TAB> <root type>
    <level> <TAB> <type> <TAB> <name> <TAB> <meta flag>      (one line per node, depth first)

Only the fields the reader uses are kept: level (to rebuild the tree), type,
name and the meta flag (bit 0x4000 aligns the stream after the field).
