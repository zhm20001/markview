# Markview

A native, read-only Markdown reader. It renders source text end to end on the
GPU, with no browser, WebView, JavaScript, or external TeX process.

## Language

**Fontdef**:
A named font declaration in a stylesheet, holding a candidate chain of family
names to try in order.
_Avoid_: Font definition (in prose), font slot

**Font family**:
A named typeface the machine can shape with, whether the system provides it or
the reader downloaded it.
_Avoid_: Font (alone), typeface

**Font role**:
One of six slots a reader picks a family for — serif, sans-serif, monospace,
and the same three for Han text. A role names exactly one fontdef of the
stylesheet in force.
_Avoid_: Font class, font kind

**Override**:
A stored family pick for one role, replacing that fontdef's own candidate
chain. Written to settings as a `fontdef-override`.
_Avoid_: Font preference, substitution

**Variant**:
The CJK convention in force — Simplified, Traditional, Japanese, or none. A
stylesheet carries one Han fontdef per convention and resolves the one the
variant selects; with none, no Han fontdef exists at all.
_Avoid_: Locale, language (for this concept)
