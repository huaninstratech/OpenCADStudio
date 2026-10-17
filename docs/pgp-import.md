# Importing AutoCAD command aliases (.pgp)

1. Run `ALIASEDIT` in OpenCADStudio.
2. Choose **Import PGP…** and select an AutoCAD or OpenCAD `.pgp` file.
3. Review the working rows. Supported imported aliases override matching
   existing aliases; other existing aliases are retained.
4. Choose **Apply** to activate and save the rows, or close and discard them.

There is no automatic discovery or loading of AutoCAD profiles. Importing only
changes the editor's working rows; it never changes the active table until
Apply. The source PGP is not modified.

## Format and compatibility

- UTF-8 (with or without BOM) and legacy Windows-1252 are accepted.
- `;` comments, whitespace padding and case-insensitive names are supported.
- The last definition of an alias wins, including user overrides at the end.
- Common Spanish command targets are translated, for example `COPIA` to
  `COPY`, `EMPALME` to `FILLET` and `SCP` to `UCS`. Alias keys are not translated.
- Only registered command targets are imported. Unsupported commands and
  unmapped translations are counted in the editor's summary and do not replace
  existing aliases. Full diagnostics are logged, rather than flooding the
  command line with errors. This does not implement missing AutoCAD/Civil 3D
  commands or add support for every AutoCAD language.
- External-program definitions are ignored and never executed.

## Synthetic example

This minimal example is authored for testing, not copied from an Autodesk PGP:

```text
; Last definition wins; imported aliases remain drafts until Apply.
E, *BORRA
E, *EMPALME
CC, *COPIA
SQ, *SCP
```

After importing, `E` resolves to `FILLET`, `CC` to `COPY`, and `SQ` to `UCS`.
No proprietary PGP or user profile is included with this feature.
