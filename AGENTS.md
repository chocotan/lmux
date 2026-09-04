# Muxlane Project Instructions

## Visual Design

- Use square corners throughout the application. Every UI surface and control must have a zero corner radius.
- Do not add GPUI `rounded_*` / `rounded(...)` styles, pill or capsule treatments, or non-zero CSS-style border radii.
- Buttons, inputs, dialogs, menus, popovers, badges, panels, tooltips, progress tracks, and selection surfaces must all remain rectangular.
- SVG frames and containers must not use `rx` or `ry`. Curved strokes that are intrinsic to a semantic icon are allowed, but they must not create a rounded container treatment.
- When touching existing UI, remove nearby rounded-corner styling instead of preserving or extending it.
- Before completing UI work, search the changed surface for `rounded`, SVG `rx` / `ry`, and pill/capsule styling, then verify that no rounded container remains.
