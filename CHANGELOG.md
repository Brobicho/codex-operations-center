# Changelog

## 0.1.6 - 2026-07-26

- Keep operators, infrastructure, and labels at a stable visual scale during preview frames.
- Add hover highlighting, native Kitty pointer shapes, and clickable activity rows.
- Show exact timestamps, project, tool, full command, and changed files in an activity inspector.
- Restore the intended RGB interface palette when the parent shell exports `NO_COLOR`.

## 0.1.5 - 2026-07-26

- Render responsive low-latency previews while dragging and zooming, then sharpen on release.
- Keep preview and full-resolution scene backgrounds cached independently.
- Add distinct activity colors and markers for commands, patches, lifecycle, web, and alerts.

## 0.1.4 - 2026-07-26

- Stop regenerating the HD framebuffer for unrelated terminal events and unchanged refreshes.
- Coalesce wheel and drag gestures into one final frame, ignoring contradictory wheel bounce.
- Send local HD frames through Kitty's temporary-file transport instead of flooding the TTY.
- Cache static scene layers, fast-path opaque pixels, and cap only the internal live framebuffer.
- Move Codex discovery and rollout parsing off the interactive render thread.
- Describe observed commands and changed filenames in the activity journal.
- Reduce rollout scanning and idle refresh pressure while retaining live task detection.

## 0.1.3 - 2026-07-26

- Ignore residual unmodified wheel events when the HD terminal window opens.
- Require Ctrl+wheel for deliberate mouse zoom; keyboard zoom remains unchanged.

## 0.1.2 - 2026-07-26

- Bound HD camera zoom so project rooms cannot be pushed outside the viewport.
- Apply wheel zoom only while the pointer is inside the 3D scene.
- Add `0` as an immediate camera and zoom reset.

## 0.1.1 - 2026-07-26

- Reject VTE builds that expose the Sixel preference without compiling `+SIXEL` support.
- Open the HD scene automatically in a pinned, self-contained terminal renderer when needed.
- Maximize the HD control-center window and retain Unicode/SSH/tmux fallbacks.
- Remove the bundled renderer during clean uninstallation.

## 0.1.0 - 2026-07-26

- Discover local Codex threads through `codex app-server`.
- Capture lifecycle events through reversible global hooks.
- Render an interactive perspective 3D operations scene.
- Support Kitty pixel graphics, True Color Unicode, and safe rendering profiles.
- Add mouse selection, camera movement, zoom, and live event panels.
- Add diagnostics, PNG snapshots, one-line installation, and clean uninstallation.
