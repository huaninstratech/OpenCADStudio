# UCS input and UCSFOLLOW

## Direct point input

After running `UCS`, a click or typed coordinate can immediately supply the
new origin, followed by a point on the positive X axis and a point in the
positive XY plane. Explicit `3P` input continues to work. `Origin` / `O` still
changes only the origin with one point.

The interactive option prompt also accepts Spanish keywords, including
`Universal`, `Vista`, `Cara`, `Objeto`, `Origen`, `Guardar`, `Eliminar`,
and `3p` / `3 puntos`, as well as international English option keywords.

## UCSFOLLOW

- `UCSFOLLOW 0`: keep the camera orientation when the UCS changes.
- `UCSFOLLOW 1`: align the view immediately with the current UCS XY plane and
  follow later UCS changes. Re-entering `1` realigns after a manual orbit.
- Bare `UCSFOLLOW` prompts for the value; Enter keeps the current setting.
- `SETVAR UCSFOLLOW 0` and `SETVAR UCSFOLLOW 1` work too.

The value belongs to the model pane / floating viewport and uses the drawing's
existing VPORT/VIEWPORT fields, not a global application preference. Paper
space itself does not follow the UCS: enter a model viewport first. BEDIT's
special block-reframing path is intentionally left unchanged.

The immediate alignment on entering `1` is deliberate: users can define their
UCS first, then enable following without another UCS change.

A user-defined alias such as `UF,*UCSFOLLOW` can invoke this command. It is not
installed automatically. A PGP imported before this command was implemented
must be imported again (or edited manually) to add its previously skipped
alias; activating it still requires Apply.
